//! The engine's half of the debug console: the state a loop keeps, the keys it
//! reads, and the commands the engine itself owns.
//!
//! `docs/plan/52-debug-console.md` is the design. The three pieces below it were
//! built first and each knows nothing of the others — `crcbl_console` is the
//! registry and the parser, `crcbl_core::log::console` is the ring every log
//! record lands in, and `crcbl_ui::console` is a panel that draws values it is
//! handed. This module is where they meet:
//!
//! ```text
//! ShellEvent ─→ Console::observe ─→ TextField / History / Registry::complete
//!                     └ Enter ─→ Registry::execute ─→ log::console::print ─→ ring
//! frame ────→ Console::draw ─→ snapshot_since ─→ LogView ─→ DrawList
//! ```
//!
//! A line the console prints goes through the **log**, not into the panel, so
//! the terminal and the panel show the same records in the same order — plan
//! decision 4. The panel reads them back out of the ring on the next frame like
//! any other record, which is why there is no second path for the console's own
//! output to drift down.
//!
//! # What is the loop's, and what is here
//!
//! [`Console`] owns everything that survives a frame and nothing that needs the
//! loop: it never reaches a renderer, a clock or a mixer. A command that wants
//! one records the ask on [`EngineLink`] — [`crate::settings::ConsoleHost`]'s
//! third field — and [`Loop::frame`](crate::engine::Loop::frame) drains it where
//! the bundle is in hand, which is the arrangement
//! [`settings::Deferred`](crate::settings::Deferred) already had to take for a
//! settings write.

use std::any::Any;
use std::time::Duration;

use crcbl_console::{Context, Fault, History, Registry, Table};
use crcbl_core::input::{KeyCode, Modifiers, ScrollDelta};
use crcbl_input::{ActionMap, Binding};
use crcbl_shell::{ButtonState, ClipboardContent, ClipboardRequestId, ShellEvent};
use crcbl_ui::console::{ConsolePanel, caret_shown};
use crcbl_ui::{FontAtlas, PointerInput, UiState, draw_list::DrawList};

use crate::settings::ConsoleHost;

/// Cycles the panel's own level filter, while the console is open.
///
/// **Any key would do**, and that is the argument for this one: while the panel
/// is up the loop claims every key event, so nothing a game binds is at stake
/// and the choice costs a player nothing. `F2` sits beside the debug overlay's
/// `F3`, which is the other thing in this engine a developer presses to see
/// more.
///
/// It moves the *panel's* threshold — [`LogView::set_filter`] — and not the
/// logger's: "show me the debug lines I already have" is a different ask from
/// "start writing debug lines to the terminal", which is the `log` command.
///
/// [`LogView::set_filter`]: crcbl_ui::console::LogView::set_filter
pub const CONSOLE_LEVEL_KEY: KeyCode = KeyCode::F2;

/// Pastes the clipboard into the field, held with `Ctrl` or the platform's
/// `Meta`/`Command` key.
///
/// Plan 52's first follow-up, and it is one key rather than a command because
/// pasting is what a person does *while* typing a line. Both modifiers are
/// accepted on every platform: the engine has no per-platform shortcut table,
/// `Meta`+`V` is what a Mac keyboard sends and `Ctrl`+`V` is what every other
/// one does, and neither means anything else to the console.
///
/// **`Shift`+`Insert` is deliberately not a second spelling.** It is the X11
/// convention for the *primary selection*, which is a different clipboard from
/// the one [`Shell::clipboard_request`](crcbl_shell::Shell::clipboard_request)
/// reads, and binding it to this one would paste the wrong text on the one
/// platform whose users expect it.
pub const CONSOLE_PASTE_KEY: KeyCode = KeyCode::KeyV;

/// The modifiers [`CONSOLE_PASTE_KEY`] is read under.
const PASTE_MODIFIERS: Modifiers = Modifiers::CTRL.union(Modifiers::SUPER);

/// How many log lines one wheel detent scrolls.
///
/// [`ScrollDelta`] keeps detents and pixels apart and leaves the conversion to
/// the application — see [`Pending::scrolls`](crate::engine::Pending::scrolls) —
/// so this is that policy, for the console and for nothing else. Three lines a
/// detent is what a terminal emulator does.
pub const WHEEL_LINES: f32 = 3.0;

/// How many pixels of continuous scroll make one log line.
///
/// The other half of [`WHEEL_LINES`], for a touchpad. One row of the panel's own
/// text would be the exact answer and it is not knowable here — the row height
/// depends on the scale the layout chose — so this is the browser's own detent,
/// which is the number every page in the workspace is already scrolled by.
pub const WHEEL_PIXELS_PER_LINE: f64 = 53.0;

/// The levels [`CONSOLE_LEVEL_KEY`] cycles through, in order.
///
/// Every level, then `Off`, then round again — so the key both narrows the view
/// and gets back to showing everything without a second binding.
const LEVELS: [crcbl_core::log::LevelFilter; 6] = [
    crcbl_core::log::LevelFilter::Trace,
    crcbl_core::log::LevelFilter::Debug,
    crcbl_core::log::LevelFilter::Info,
    crcbl_core::log::LevelFilter::Warn,
    crcbl_core::log::LevelFilter::Error,
    crcbl_core::log::LevelFilter::Off,
];

/// What the console's commands ask of the loop, and what the loop last told
/// them.
///
/// The third field of [`ConsoleHost`], and the reason it exists is
/// [`settings::Deferred`](crate::settings::Deferred)'s: a
/// [`Binding`](crcbl_console::Binding) and a
/// [`ConCommand`](crcbl_console::ConCommand) both reach their host as
/// `&mut dyn Any`, and [`Any`] is implemented only for `'static` types — so a
/// command cannot hold a borrow of the loop it wants to pause. It records the
/// ask here and the loop takes it once a frame.
///
/// The two directions are deliberately asymmetric. A request is **taken**, so a
/// command that ran twice pauses once; the frame timing is **overwritten**, so
/// `fps` reads the newest frame rather than a queue of old ones.
#[derive(Debug, Default)]
pub struct EngineLink {
    /// The name the settings file is saved under, or `None` for a run that must
    /// not write one — a golden run, a headless test. See
    /// [`SettingsSource::None`](crate::engine::SettingsSource::None), which is
    /// the same rule stated where the file is read.
    pub(crate) app_name: Option<String>,
    /// `pause` asked the loop to toggle the simulation.
    pub(crate) pause: bool,
    /// `quit` asked the loop to stop.
    pub(crate) quit: bool,
    /// Frames a second, as the loop last measured it.
    pub(crate) fps: f32,
    /// The last frame's wall time, in milliseconds.
    pub(crate) frame_ms: f32,
    /// What `bind` and `unbind` asked of the game's action map, in the order
    /// they were typed.
    ///
    /// A queue rather than one slot, unlike [`Self::pause`]: `bind a KeyA; bind
    /// b KeyB` is one line and both halves of it were meant.
    pub(crate) binds: Vec<BindAsk>,
}

impl EngineLink {
    /// Nothing asked for, nothing measured, and nowhere to save.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            app_name: None,
            pause: false,
            quit: false,
            fps: 0.0,
            frame_ms: 0.0,
            binds: Vec::new(),
        }
    }

    /// The name `save` writes the settings file under, if this run has one.
    #[must_use]
    pub fn app_name(&self) -> Option<&str> {
        self.app_name.as_deref()
    }

    /// Whether `pause` was run since the loop last looked.
    pub const fn take_pause(&mut self) -> bool {
        std::mem::replace(&mut self.pause, false)
    }

    /// Whether `quit` was run since the loop last looked.
    pub const fn take_quit(&mut self) -> bool {
        std::mem::replace(&mut self.quit, false)
    }

    /// Tells `fps` what the last frame cost.
    pub const fn set_frame_timing(&mut self, rate: f32, frame_ms: f32) {
        self.fps = rate;
        self.frame_ms = frame_ms;
    }

    /// Every `bind`/`unbind` typed since the loop last looked.
    pub fn take_binds(&mut self) -> Vec<BindAsk> {
        std::mem::take(&mut self.binds)
    }
}

/// What one `bind` or `unbind` line asked of the game's action map.
///
/// Recorded rather than done, for [`EngineLink`]'s reason: the map is the
/// game's and a command reaches its host as `&mut dyn Any`, so the line is
/// carried to [`Loop::drain_binds`](crate::engine::Loop) and applied where the
/// game is in hand — the same arrangement `pause` and the settings writes take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindAsk {
    /// `bind` — every action and what drives it.
    List,
    /// `bind <action>` — one action and what drives it.
    Show(String),
    /// `bind <action> <key>` — that action, driven by that key and nothing
    /// else.
    Set {
        /// The action being rebound.
        action: String,
        /// The key it fires on from now on.
        key: KeyCode,
    },
    /// `unbind <action>` — that action, driven by nothing at all.
    Clear(String),
}

/// The [`EngineLink`] on the host a command was handed.
///
/// # Panics
///
/// If the host is not a [`ConsoleHost`]. Every registry the engine gathers is
/// run over one — [`Console::new`] is the only constructor — and the same
/// `expect` guards `crate::settings`' bindings for the same reason.
fn link(host: &mut dyn Any) -> &mut EngineLink {
    host.downcast_mut::<ConsoleHost>()
        .expect("the engine's console is only ever run over a `ConsoleHost`")
        .engine_mut()
}

crcbl_console::concommand! {
    /// Stop the simulation, or start it again.
    pub fn pause(cx, _args) {
        link(cx.host_mut()).pause = true;
        cx.print("pause toggled");
        Ok(())
    }
}

crcbl_console::concommand! {
    /// Stop the run and tear it down cleanly.
    pub fn quit(cx, _args) {
        link(cx.host_mut()).quit = true;
        cx.print("quitting");
        Ok(())
    }
}

crcbl_console::concommand! {
    /// Print the last frame's rate and wall time.
    pub fn fps(cx, _args) {
        let (rate, ms) = {
            let link = link(cx.host_mut());
            (link.fps, link.frame_ms)
        };
        cx.print(format!("{rate:.1} fps, {ms:.2} ms/frame"));
        Ok(())
    }
}

crcbl_console::concommand! {
    /// Show what drives an action, or drive it with one key instead: `bind jump Space`.
    pub fn bind(cx, args) {
        let ask = match args {
            [] => BindAsk::List,
            [action] => BindAsk::Show((*action).to_owned()),
            [action, key] => BindAsk::Set {
                action: (*action).to_owned(),
                key: key_named(key)?,
            },
            _ => {
                return Err(Fault::new(
                    "bind takes an action and one key, an action alone, or nothing at all",
                ));
            }
        };
        link(cx.host_mut()).binds.push(ask);
        Ok(())
    }
}

crcbl_console::concommand! {
    /// Leave an action with nothing driving it: `unbind jump`.
    pub fn unbind(cx, args) {
        let [action] = args else {
            return Err(Fault::new("unbind takes one action name"));
        };
        link(cx.host_mut())
            .binds
            .push(BindAsk::Clear((*action).to_owned()));
        Ok(())
    }
}

/// The key `name` spells, without regard to case.
///
/// The names are [`KeyCode::as_str`]'s — the W3C `code` spellings, which are
/// what a binding profile is written in — and the case is ignored for the
/// registry's own reason: the console matches a variable's name that way, and a
/// person typing `bind jump space` at a prompt has not made a mistake.
///
/// # Errors
///
/// A [`Fault`] naming three of the spellings, because "not a key" on its own
/// leaves someone guessing at the format rather than at the key.
fn key_named(name: &str) -> Result<KeyCode, Fault> {
    KeyCode::ALL
        .iter()
        .copied()
        .find(|key| key.as_str().eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            Fault::new(format!(
                "`{name}` is not a key — they are the `code` spellings, like `KeyF`, `Space` or `ArrowUp`"
            ))
        })
}

/// Carries out one [`BindAsk`] against the game's map, and prints the answer.
///
/// Here rather than in the loop because the reporting is the console's: what a
/// binding is called in a printed line is this module's business, and the loop's
/// half is only that it has the game in hand.
pub fn apply_bind(actions: &mut ActionMap, ask: &BindAsk) {
    match ask {
        BindAsk::List => {
            let names: Vec<String> = actions.action_names().map(str::to_owned).collect();
            if names.is_empty() {
                crcbl_core::log::console::print("this game declares no actions");
                return;
            }
            for name in &names {
                crcbl_core::log::console::print(&bindings_line(actions, name));
            }
        }
        BindAsk::Show(action) => {
            crcbl_core::log::console::print(&bindings_line(actions, action));
        }
        BindAsk::Set { action, key } => {
            // The whole list, not an addition: `bind` in Source replaces, and
            // an action that kept its old key as well would leave a player who
            // rebound away from a clash still holding the clash.
            match actions.rebind(action, vec![Binding::Key(*key)]) {
                Ok(()) => crcbl_core::log::console::print(&bindings_line(actions, action)),
                Err(error) => crcbl_core::log::console::print(&format!(
                    "{error} — `bind` alone lists the ones this game has"
                )),
            }
        }
        BindAsk::Clear(action) => match actions.rebind(action, Vec::new()) {
            Ok(()) => crcbl_core::log::console::print(&bindings_line(actions, action)),
            Err(error) => crcbl_core::log::console::print(&format!(
                "{error} — `bind` alone lists the ones this game has"
            )),
        },
    }
}

/// One action and everything that drives it, in the shape every `bind` line
/// prints.
fn bindings_line(actions: &ActionMap, action: &str) -> String {
    match actions.bindings(action) {
        None => format!("no action called `{action}` — `bind` alone lists them"),
        Some([]) => format!("{action} = nothing"),
        Some(bindings) => {
            let sources: Vec<String> = bindings.iter().map(binding_name).collect();
            format!("{action} = {}", sources.join(", "))
        }
    }
}

/// What one binding is called in a printed line.
///
/// A match rather than [`Binding`]'s `Debug`, so the line reads as something a
/// person typed: only [`Binding::Key`] can be typed back in, and the rest say
/// what the device is rather than what the variant is called.
fn binding_name(binding: &Binding) -> String {
    match binding {
        Binding::Key(key) => key.as_str().to_owned(),
        Binding::MouseButton(button) => format!("mouse {button:?}"),
        Binding::MouseMotion => "mouse motion".to_owned(),
        Binding::MouseScroll => "mouse wheel".to_owned(),
        Binding::PointerPosition { axis } => format!("pointer {axis:?}"),
        Binding::KeyAxis { negative, positive } => {
            format!("{}/{}", negative.as_str(), positive.as_str())
        }
        Binding::Virtual(id) => format!("on-screen `{id}`"),
        Binding::Wasd {
            up,
            down,
            left,
            right,
        } => format!(
            "{}{}{}{}",
            up.as_str(),
            left.as_str(),
            down.as_str(),
            right.as_str()
        ),
    }
}

/// The engine's own console state: the registry, the panel, the history and the
/// host every command and binding is run over.
///
/// One per [`Loop`](crate::engine::Loop), built at
/// [`Loop::new`](crate::engine::Loop::new) and drawn last in the frame.
#[derive(Debug)]
pub struct Console {
    panel: ConsolePanel,
    registry: Registry,
    host: ConsoleHost,
    history: History,
    ui: UiState,
    open: bool,
    /// How long the run has been going, which is all the caret's blink needs.
    elapsed: Duration,
    /// Rows the last drawn layout had, which is what a page scroll moves by.
    ///
    /// Read off the layout rather than assumed, so `PageUp` moves exactly one
    /// screen of whatever size the panel came out at.
    page: usize,
    /// The candidates `Tab` is cycling and where in them it is, or empty when
    /// the last key was not a `Tab`.
    cycle: Vec<String>,
    cycle_at: Option<usize>,
    /// The text before the token being completed, so a cycled candidate
    /// replaces the token and not the line.
    cycle_stem: String,
    /// The prefix every candidate shares, which is the head the panel
    /// highlights.
    cycle_prefix: String,
    /// [`CONSOLE_PASTE_KEY`] was pressed and the loop has not yet asked the
    /// shell for the clipboard.
    ///
    /// A request rather than a read for [`EngineLink`]'s reason once more: the
    /// shell is being pumped while the key arrives, so the console cannot call
    /// [`Shell::clipboard_request`](crcbl_shell::Shell::clipboard_request) from
    /// inside the pump's own closure.
    paste_wanted: bool,
    /// The clipboard read the loop issued and no [`ShellEvent::ClipboardData`]
    /// has answered yet.
    ///
    /// Matched by id, because exactly one event answers each request and a
    /// backend may answer several frames later — the seam's obligation 4. A
    /// second paste while one is outstanding replaces it: the newer press is the
    /// one the person is waiting on, and the older answer is then ignored rather
    /// than typed into the field behind it.
    awaiting_paste: Option<ClipboardRequestId>,
}

impl Console {
    /// A closed console over `tables`, run against `host`.
    ///
    /// The built-in commands are added by
    /// [`Registry::gather`](crcbl_console::Registry::gather) itself, so a caller
    /// passes only the crates' own tables.
    ///
    /// # Panics
    ///
    /// If two tables claim one name, naming both — plan decision 2 refuses a
    /// duplicate rather than resolving it, because either resolution leaves one
    /// crate reading a variable the console is not setting. It is a wiring
    /// mistake in the gather rather than anything a run can produce, and
    /// `crates/crcbl/tests/console_gather.rs` is what holds the gather to the
    /// crates that own a table.
    #[must_use]
    pub fn new(tables: &[Table], host: ConsoleHost) -> Self {
        let registry = Registry::gather(tables).unwrap_or_else(|duplicate| {
            panic!("the console's tables cannot be gathered: {duplicate}")
        });
        Self {
            panel: ConsolePanel::new(),
            registry,
            host,
            history: History::new(),
            ui: UiState::new(),
            open: false,
            elapsed: Duration::ZERO,
            page: 1,
            cycle: Vec::new(),
            cycle_at: None,
            cycle_stem: String::new(),
            cycle_prefix: String::new(),
            paste_wanted: false,
            awaiting_paste: None,
        }
    }

    /// Whether the panel is showing.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the panel, and puts its view at the newest line.
    pub fn open(&mut self) {
        self.open = true;
        self.panel.log_mut().scroll_to_bottom();
    }

    /// Hides the panel and drops any completion it was offering, and any paste
    /// that has not been answered.
    ///
    /// The read is not cancellable — a backend answers every request it
    /// accepted — so what is dropped here is the *expectation*: an answer that
    /// arrives after the panel is shut lands in no field, rather than in the
    /// line whoever opens the console next is typing.
    pub fn close(&mut self) {
        self.open = false;
        self.paste_wanted = false;
        self.awaiting_paste = None;
        self.clear_cycle();
    }

    /// Shows the panel if it was hidden, and hides it if it was showing.
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// The registry every command and variable was gathered into.
    #[must_use]
    pub const fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The panel, to read — what a test asserts the typed line against.
    #[must_use]
    pub const fn panel(&self) -> &ConsolePanel {
        &self.panel
    }

    /// The host every command and binding is run over, to drain.
    pub const fn host_mut(&mut self) -> &mut ConsoleHost {
        &mut self.host
    }

    /// Folds one event in while the console is open, and says whether the
    /// console claimed it.
    ///
    /// **Everything the keyboard produces is claimed while the panel is up**,
    /// including keys this does nothing with: a game that saw the letters being
    /// typed into the field would be played by the console. The reserved keys
    /// never reach here at all — [`Pending::observe`](crate::engine::Pending)
    /// takes them first — which is what leaves `F3`, `F11` and the console's own
    /// key working with the panel open.
    pub fn observe(&mut self, event: &ShellEvent) -> bool {
        if !self.open {
            return false;
        }
        match event {
            ShellEvent::TextCommit { text, .. } => {
                self.panel.field_mut().insert(text);
                self.clear_cycle();
                true
            }
            ShellEvent::Key {
                key_code: Some(code),
                state,
                modifiers,
                ..
            } => {
                // Repeats included: holding Backspace to clear a line is what a
                // text field is for, and the reserved keys — the ones a repeat
                // would toggle at the keyboard's rate — were claimed before
                // this.
                if matches!(state, ButtonState::Pressed) {
                    self.key(*code, *modifiers);
                }
                true
            }
            // The answer to this console's own paste, and to no other read: a
            // game that asked the clipboard for something of its own gets its
            // event back untouched.
            ShellEvent::ClipboardData {
                request, content, ..
            } if self.awaiting_paste == Some(*request) => {
                self.awaiting_paste = None;
                self.paste(content);
                true
            }
            // A key with no `key_code` is one no layout named; nothing here can
            // act on it, and the text it produced arrives as a `TextCommit`.
            ShellEvent::Key { .. } => true,
            _ => false,
        }
    }

    /// Scrolls the log by one wheel event.
    pub fn scroll(&mut self, delta: ScrollDelta) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a scroll of more lines than an i32 holds is a scroll the view clamps anyway"
        )]
        let lines = match delta {
            ScrollDelta::Lines { y, .. } => (y * WHEEL_LINES) as i32,
            ScrollDelta::Pixels { y, .. } => {
                (y / WHEEL_PIXELS_PER_LINE * f64::from(WHEEL_LINES)) as i32
            }
        };
        self.panel.log_mut().scroll_by(lines);
    }

    /// Runs one frame of pointer input against the panel, and executes a line
    /// the **Send** button submitted.
    ///
    /// Answers whether the pointer was **over the panel**, which is what the
    /// loop needs to know to keep the press away from a menu underneath: the
    /// console is drawn over the top of the frame, so a click that lands on it
    /// is not a click on whatever the panel is covering.
    pub fn point(&mut self, extent: (u32, u32), atlas: &FontAtlas, pointer: PointerInput) -> bool {
        if !self.open {
            return false;
        }
        let layout = self.panel.layout(extent, atlas);
        let submitted = {
            let Self { panel, ui, .. } = self;
            panel.point(&layout, ui, pointer)
        };
        if let Some(line) = submitted {
            self.run(&line);
        }
        let (min, max) = layout.panel();
        pointer.pos.x >= min.x
            && pointer.pos.x <= max.x
            && pointer.pos.y >= min.y
            && pointer.pos.y <= max.y
    }

    /// Draws the panel, after taking whatever the log ring has gained.
    ///
    /// `render_dt` advances the caret's blink and is folded in **whether or not
    /// the panel is showing**, so the caret is where the clock says it is on the
    /// frame the console opens rather than starting its blink over.
    pub fn draw(
        &mut self,
        dl: &mut DrawList,
        extent: (u32, u32),
        atlas: &FontAtlas,
        render_dt: Duration,
    ) {
        self.elapsed += render_dt;
        if !self.open {
            return;
        }
        // The cursor is what makes this a copy of the new lines rather than of
        // the whole ring — see `snapshot_since`. A console that has just opened
        // has a cursor of zero and so takes everything, which is what "the panel
        // shows the log" means on the first frame.
        let records = crcbl_core::log::console::snapshot_since(self.panel.log().cursor());
        self.panel.log_mut().push_records(&records);
        let layout = self.panel.layout(extent, atlas);
        self.page = layout.log_rows().max(1);
        self.panel
            .render(dl, &layout, atlas, caret_shown(self.elapsed));
    }

    /// Whether [`CONSOLE_PASTE_KEY`] was pressed since the loop last asked.
    ///
    /// The loop takes it, issues the read where the shell is in hand, and hands
    /// the id back through [`expect_paste`](Self::expect_paste).
    pub const fn take_paste_request(&mut self) -> bool {
        std::mem::replace(&mut self.paste_wanted, false)
    }

    /// The read the loop issued for the paste this console asked for.
    pub const fn expect_paste(&mut self, request: ClipboardRequestId) {
        self.awaiting_paste = Some(request);
    }

    /// Puts a clipboard answer into the field, or says why it could not.
    ///
    /// The text goes in through [`TextField::insert`], which drops control
    /// characters — so a copied newline joins the two lines rather than
    /// submitting the first, and a line pasted from a file arrives as one line.
    ///
    /// [`TextField::insert`]: crcbl_ui::console::TextField::insert
    fn paste(&mut self, content: &ClipboardContent) {
        match content {
            ClipboardContent::Bytes(bytes) => match core::str::from_utf8(bytes) {
                Ok(text) => {
                    self.panel.field_mut().insert(text);
                    self.clear_cycle();
                }
                // Refused rather than replaced character by character: a paste
                // that silently corrupts what was copied is worse than one that
                // does not happen — `ClipboardContent::text` makes the same
                // call for the same reason.
                Err(_) => {
                    crcbl_core::log::console::print("paste: the clipboard is not UTF-8 text");
                }
            },
            ClipboardContent::Empty => {
                crcbl_core::log::console::print("paste: the clipboard is empty");
            }
            ClipboardContent::Unavailable => {
                crcbl_core::log::console::print("paste: the clipboard could not be read");
            }
        }
    }

    /// One key press, while the console is open.
    fn key(&mut self, code: KeyCode, modifiers: Modifiers) {
        if code == CONSOLE_PASTE_KEY && modifiers.intersects(PASTE_MODIFIERS) {
            self.paste_wanted = true;
            return;
        }
        match code {
            KeyCode::Enter => {
                if let Some(line) = self.panel.submit() {
                    self.run(&line);
                }
                self.clear_cycle();
            }
            KeyCode::Tab => self.complete(),
            KeyCode::ArrowUp => {
                let current = self.panel.field().text().to_owned();
                if let Some(line) = self.history.up(&current).map(str::to_owned) {
                    self.panel.field_mut().set_text(&line);
                }
                self.clear_cycle();
            }
            KeyCode::ArrowDown => {
                if let Some(line) = self.history.down().map(str::to_owned) {
                    self.panel.field_mut().set_text(&line);
                }
                self.clear_cycle();
            }
            KeyCode::PageUp => self.panel.log_mut().scroll_by(page_step(self.page)),
            KeyCode::PageDown => self.panel.log_mut().scroll_by(-page_step(self.page)),
            CONSOLE_LEVEL_KEY => self.cycle_level(),
            _ => {
                let field = self.panel.field_mut();
                let moved = match code {
                    KeyCode::Backspace => field.backspace(),
                    KeyCode::Delete => field.delete(),
                    KeyCode::ArrowLeft => field.move_left(),
                    KeyCode::ArrowRight => field.move_right(),
                    KeyCode::Home => field.move_home(),
                    KeyCode::End => field.move_end(),
                    // Every other key is claimed and does nothing: the character
                    // it produced arrives as a `TextCommit` with the layout
                    // applied, which is the only thing that can type in a
                    // language whose letters are not on the keycodes.
                    _ => false,
                };
                if moved {
                    self.clear_cycle();
                }
            }
        }
    }

    /// Runs `line` through the registry and puts everything it printed in the
    /// log.
    ///
    /// The echoed line goes first, prefixed the way the prompt draws it, so the
    /// terminal shows the same exchange the panel does — plan decision 4's whole
    /// point. A fault is printed like any other line and leaves the state alone,
    /// which is [`Registry::execute`](crcbl_console::Registry::execute)'s own
    /// guarantee.
    fn run(&mut self, line: &str) {
        crcbl_core::log::console::print(&format!("{}{line}", crcbl_ui::console::PROMPT));
        self.history.push(line);
        let (lines, fault, clear) = {
            let Self { registry, host, .. } = self;
            let mut cx = Context::new(registry, host);
            let outcome = registry.execute(&mut cx, line);
            let clear = cx.clear_requested();
            (cx.into_lines(), outcome.err(), clear)
        };
        for printed in &lines {
            crcbl_core::log::console::print(printed);
        }
        if let Some(fault) = fault {
            crcbl_core::log::console::print(&fault.to_string());
        }
        if clear {
            // The **view**, not the ring: Source's `clear` empties the console
            // and not the file, which is what `request_clear` documents.
            self.panel.log_mut().clear();
        }
    }

    /// `Tab`: fill in the prefix every candidate shares, then cycle them.
    fn complete(&mut self) {
        if !self.cycle.is_empty() {
            let next = self.cycle_at.map_or(0, |at| (at + 1) % self.cycle.len());
            self.cycle_at = Some(next);
            let filled = format!("{}{}", self.cycle_stem, self.cycle[next]);
            self.panel.field_mut().set_text(&filled);
            self.show_candidates();
            return;
        }

        let text = self.panel.field().text().to_owned();
        let partial = completing(&text);
        let stem = text[..text.len() - partial.len()].to_owned();
        let completion = self.registry.complete(&text);
        if completion.candidates.is_empty() {
            self.panel.clear_completion();
            return;
        }
        let filled = format!("{stem}{}", completion.common);
        self.panel.field_mut().set_text(&filled);
        self.cycle_stem = stem;
        self.cycle_prefix = completion.common;
        // One candidate is a completion and not a cycle: a second `Tab` on it
        // would put the same word back.
        self.cycle = if completion.candidates.len() > 1 {
            completion
                .candidates
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        } else {
            Vec::new()
        };
        self.cycle_at = None;
        self.show_candidates();
    }

    /// Offers the candidates the cycle is holding to the panel.
    fn show_candidates(&mut self) {
        let candidates: Vec<&str> = self.cycle.iter().map(String::as_str).collect();
        if candidates.is_empty() {
            self.panel.clear_completion();
        } else {
            self.panel.set_completion(&self.cycle_prefix, &candidates);
        }
    }

    /// Drops the completion, which is what any edit that is not a `Tab` does.
    fn clear_cycle(&mut self) {
        self.cycle.clear();
        self.cycle_at = None;
        self.cycle_stem.clear();
        self.cycle_prefix.clear();
        self.panel.clear_completion();
    }

    /// Steps the panel's level threshold one along [`LEVELS`].
    fn cycle_level(&mut self) {
        let current = self.panel.log().filter();
        let at = LEVELS
            .iter()
            .position(|level| *level == current)
            .map_or(0, |at| (at + 1) % LEVELS.len());
        self.panel.log_mut().set_filter(LEVELS[at]);
        crcbl_core::log::console::print(&format!("console shows {} and above", LEVELS[at]));
    }
}

/// How far one page scrolls, clamped to what an `i32` holds.
fn page_step(rows: usize) -> i32 {
    i32::try_from(rows).unwrap_or(i32::MAX)
}

/// The token [`Registry::complete`](crcbl_console::Registry::complete) is
/// completing, as a suffix of `text`.
///
/// Spelled the same way that method splits its argument, and it has to be: the
/// caller replaces `text` minus this with what came back, so a different split
/// here would put the completion in the wrong place. Everything after the first
/// token is **one** value — `debug_view ambient occlusion` is one enum value —
/// which is why this does not stop at the last space.
fn completing(text: &str) -> &str {
    let trimmed = text.trim_start();
    match trimmed.split_once(char::is_whitespace) {
        None => trimmed,
        Some((_, rest)) => rest.trim_start(),
    }
}

/// Every crate's console table the engine gathers, named by the crate it came
/// from.
///
/// Plan decision 2's one seam. The crate name beside each table is what
/// `crates/crcbl/tests/console_gather.rs` reads: it walks the workspace
/// manifests for every crate that depends on `crcbl-console` and asserts each is
/// named here, so a crate that grows a table and is forgotten is a red test
/// rather than a set of commands nothing can reach.
///
/// The game's own table is **not** here — it arrives through
/// [`HostedGame::console_table`](crate::engine::HostedGame::console_table),
/// which is per-run rather than per-workspace.
#[must_use]
pub fn engine_tables() -> [(&'static str, Table); 2] {
    [
        ("crcbl-core", crcbl_core::console_table()),
        ("crcbl", crate::console_table()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_being_completed_is_the_whole_value_after_the_name() {
        assert_eq!(completing("r_a"), "r_a");
        assert_eq!(completing("  r_a"), "r_a");
        assert_eq!(completing("debug_view amb"), "amb");
        assert_eq!(
            completing("debug_view ambient occ"),
            "ambient occ",
            "an enum value may hold a space, so the token is everything after the name",
        );
        assert_eq!(completing("debug_view "), "");
    }
}
