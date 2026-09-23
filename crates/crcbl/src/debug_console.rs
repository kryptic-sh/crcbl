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
//! ShellEvent ─→ Console::observe ─→ History / Registry::complete / the log view
//!                     └ Enter ─→ Registry::execute ─→ log::console::print ─→ ring
//!            ─→ TextPump::observe ─→ Edit ─→ TextInput ─→ Ui::text_input
//! contact ──→ Console::note_contact ─→ the panel draws its on-screen keyboard
//! frame ────→ Console::frame ─→ snapshot_since ─→ LogView ─→ ConsolePanel's tree
//!                     └ ConsoleInput ─→ Console::tapped / the same run
//! draw ─────→ Console::draw ─→ the tree that frame built ─→ DrawList
//! ```
//!
//! # The field is the tree's, and so is its clipboard
//!
//! The typed line is [`crcbl_ui::tree::Ui::text_input`] — rung 7c's widget — so
//! the console selects, moves by word, double-clicks a word and copies, cuts
//! and pastes, none of which its own field could do. The keys that edit reach
//! it as [`Edit`]s through [`crate::text_input::TextPump`], which the loop
//! owns; what is left here is the console's own vocabulary — `Enter`, `Tab`,
//! the history arrows, the page keys and the level key.
//!
//! **There is one clipboard path and it is the pump's.** `Ctrl`/`Cmd`+`V` is an
//! `Edit::Paste` like it is in any other field, the field asks through
//! [`Ui::take_clipboard_requests`](crcbl_ui::tree::Ui::take_clipboard_requests),
//! and [`TextPump::serve`](crate::text_input::TextPump::serve) carries it to the
//! shell and matches the answer back by request id. The console's own
//! `CONSOLE_PASTE_KEY` and the loop's `ask_for_paste` are gone with it.
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

use crcbl_console::{Context, Fault, History, Registry, Table};
use crcbl_core::input::{KeyCode, ScrollDelta};
use crcbl_input::{ActionMap, Binding};
use crcbl_shell::{ButtonState, ShellEvent};
use crcbl_ui::console::{ConsoleInput, ConsoleLayout, ConsolePanel, KeyCap};
use crcbl_ui::edit::Edit;
use crcbl_ui::tree::{ClipboardRequest, TextInput};
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
        Binding::Chord { modifier, key } => format!("{modifier:?}+{}", key.as_str()),
        Binding::Virtual(id) => format!("on-screen `{id}`"),
        Binding::PadButton(button) => format!("pad {button:?}"),
        Binding::PadStick { stick, .. } => format!("pad {stick:?} stick"),
        Binding::PadTrigger { trigger, .. } => format!("pad {trigger:?} trigger"),
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

/// One key press that reads or rewrites the whole line, waiting for the frame
/// that applies it. See [`Console::line_asks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineAsk {
    /// `Enter`: send what is in the field.
    Submit,
    /// `Tab`: fill in the common prefix, then cycle the candidates.
    Complete,
    /// The up arrow: the line before this one in the history.
    Older,
    /// The down arrow: the line after it.
    Newer,
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
    /// Where this frame's build put everything, for [`Console::draw`] to emit
    /// under and for the next frame's pointer to be tested against.
    ///
    /// The panel is an element tree, and a tree is built once a frame:
    /// [`Console::frame`] is that build, and `draw` only emits it.
    layout: Option<ConsoleLayout>,
    /// The keys that read or rewrite the whole line, in the order they were
    /// pressed, waiting for [`Console::frame`] to apply them.
    ///
    /// Deferred, and they have to be: a key arrives during the shell's pump,
    /// and the characters typed **in the same batch** have not reached the
    /// field yet — they are [`Edit`]s the tree applies when `frame` builds.
    /// Submitting or completing where the key lands would work on the line as
    /// it stood before whatever was typed with it, which is an empty line for
    /// anything that types a command and presses Enter in one go.
    ///
    /// A queue rather than one slot, for the reason `EngineLink`'s `binds` is
    /// one: two `Tab`s in one batch are a completion and then a cycle through
    /// it, and both were meant.
    line_asks: Vec<LineAsk>,
    /// The line as the last frame left it, for noticing that an edit changed
    /// it.
    ///
    /// **An edit drops the completion**, and the console cannot see one happen:
    /// a typed character is an [`Edit`] the *tree* applies, where the field's
    /// own `insert` used to be a call this module made. So the line is compared
    /// against what it was — after the deferred `Tab` has had its turn, so the
    /// completion's own fill is not read as an edit that cancels it.
    last_line: String,
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
            layout: None,
            line_asks: Vec::new(),
            last_line: String::new(),
            page: 1,
            cycle: Vec::new(),
            cycle_at: None,
            cycle_stem: String::new(),
            cycle_prefix: String::new(),
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

    /// Hides the panel and drops any completion it was offering, and what this
    /// frame laid out.
    ///
    /// A clipboard read the field asked for is **not** dropped here, and cannot
    /// be: a backend answers every request it accepted, and
    /// [`TextPump`](crate::text_input::TextPump) matches that answer to the node
    /// that asked. A shut panel's field is not engaged, so the answer reaches a
    /// field that refuses it rather than the line whoever opens the console next
    /// is typing.
    pub fn close(&mut self) {
        self.open = false;
        self.layout = None;
        self.line_asks.clear();
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

    /// Folds one event into the console's **own** vocabulary while the panel is
    /// up, and says whether it acted on it.
    ///
    /// What is left here after rung 7d2: `Enter`, `Tab`, the history arrows,
    /// the page keys and [`CONSOLE_LEVEL_KEY`]. Everything that edits the line —
    /// the letters, `Backspace`, `Delete`, the caret arrows, `Home` and `End`,
    /// and the clipboard shortcuts — is an [`Edit`] the loop's
    /// [`TextPump`](crate::text_input::TextPump) reads off the same events.
    ///
    /// **Claiming the keys is no longer this method's job**, and used to be.
    /// The console is in the context stack now: the loop's
    /// [`MenuPump`](crate::engine::MenuPump) withholds every key from the game
    /// while the panel is up and still feeds them to the loop's own map, so a
    /// key held into the console is heard to come up. The reserved keys never
    /// reach here at all — [`Pending::observe`](crate::engine::Pending) takes
    /// them first — which is what leaves `F3`, `F11` and the console's own key
    /// working with the panel open.
    pub fn observe(&mut self, event: &ShellEvent) -> bool {
        if !self.open {
            return false;
        }
        let ShellEvent::Key {
            key_code: Some(code),
            state: ButtonState::Pressed,
            ..
        } = event
        else {
            return false;
        };
        // Repeats included: holding `PageUp` to walk back through a `help`
        // listing is what a page key is for, and the reserved keys — the ones a
        // repeat would toggle at the keyboard's rate — were claimed before this.
        self.key(*code)
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

    /// Where the last frame put every part of the panel, or `None` for a
    /// console that has not been built — one that is shut, or open and not yet
    /// through a frame.
    ///
    /// What a caller hit-tests the on-screen keyboard's keys against, and what
    /// [`Console::draw`] emits under.
    #[must_use]
    pub const fn layout(&self) -> Option<&ConsoleLayout> {
        self.layout.as_ref()
    }

    /// Whether the console is drawing anything over `at`, as the **last**
    /// frame laid it out.
    ///
    /// The panel and the on-screen keyboard both, which
    /// [`ConsoleLayout::covers`](crcbl_ui::console::ConsoleLayout::covers) is
    /// the one call for: the keyboard is drawn along the frame's bottom edge,
    /// outside the panel, so a caller testing only the panel's rectangle would
    /// hand every key press on to the game underneath.
    ///
    /// **`false` while the console is shut**, and before the first frame of a
    /// run that opened it, so a closed console claims nothing anywhere.
    #[must_use]
    pub fn covers(&self, at: glam::Vec2) -> bool {
        self.open && self.layout.as_ref().is_some_and(|layout| layout.covers(at))
    }

    /// Runs the console's whole frame: takes what the log ring has gained,
    /// builds and lays out the panel's tree over `extent` with `pointer` and
    /// `input`, and applies what the pointer produced — a line **Send** or the
    /// on-screen keyboard's return key submitted, or a key the keyboard was
    /// tapped on.
    ///
    /// What it lays out is what [`Console::covers`] answers from on the next
    /// frame, and what [`Console::draw`] emits at the end of this one. A shut
    /// console lays out nothing and forgets what it had.
    ///
    /// # Once a frame, and in the input phase
    ///
    /// The panel is an element tree, so this is the one call that begins its
    /// frame and builds it: a second build would latch the **Send** button's
    /// click twice and take the field's edits twice. It runs in the input phase
    /// rather than beside the drawing so that a line submitted here still
    /// reaches [`Loop::drain_console`](crate::engine::Loop) on the frame it was
    /// sent.
    ///
    /// **A command's answer is held now and drawn next frame.** Every line runs
    /// after the spans were built, so the second pull below puts the answer in
    /// the view — where a caller reading [`Console::panel`] finds it at once —
    /// and the frame after this one is the one that shows it. The panel is
    /// redrawn every frame it is open, so that is one frame of a blink.
    pub fn frame(
        &mut self,
        extent: (u32, u32),
        atlas: &FontAtlas,
        pointer: PointerInput,
        input: TextInput,
    ) {
        if !self.open {
            self.layout = None;
            return;
        }
        // The cursor is what makes this a copy of the new lines rather than of
        // the whole ring — see `snapshot_since`. A console that has just opened
        // has a cursor of zero and so takes everything, which is what "the panel
        // shows the log" means on the first frame.
        let records = crcbl_core::log::console::snapshot_since(self.panel.log().cursor());
        self.panel.log_mut().push_records(&records);

        let layout = self.panel.layout(extent, atlas, pointer, input);
        self.page = layout.log_rows().max(1);
        // The build is where this batch's typing reached the field, so it is
        // also where an edit that cancels a completion becomes visible.
        if self.panel.line() != self.last_line {
            self.clear_cycle();
        }
        // **After the build, for `line_asks`' reason**, and after the check
        // above so that a `Tab` filling the line in is not then read as an
        // edit that drops the completion it just offered.
        for ask in std::mem::take(&mut self.line_asks) {
            self.apply(ask);
        }
        self.last_line.clear();
        self.last_line.push_str(self.panel.line());
        let produced = {
            let Self { panel, ui, .. } = self;
            panel.point(&layout, ui, pointer)
        };
        self.layout = Some(layout);
        match produced {
            ConsoleInput::Nothing => {}
            ConsoleInput::Submitted(line) => {
                self.run(&line);
                self.clear_cycle();
            }
            ConsoleInput::Key(cap) => self.tapped(cap),
        }
        // **A second pull, for whatever the lines above printed.** The spans
        // were built before any of them ran, so the panel *draws* a command's
        // answer on the next frame; the view holds it now, which is what a
        // reader of `Console::panel` is asking about and what stops the answer
        // being taken twice when the next frame pulls again.
        let answers = crcbl_core::log::console::snapshot_since(self.panel.log().cursor());
        self.panel.log_mut().push_records(&answers);
    }

    /// The clipboard offers and reads this frame's field asked for, for the
    /// loop to carry to the shell through
    /// [`TextPump::serve`](crate::text_input::TextPump::serve).
    ///
    /// Empty while the console is shut, because a shut console builds no field.
    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        self.panel.take_clipboard_requests()
    }

    /// Whether the panel's field is taking typed text: what the loop reads the
    /// shell's keys into [`Edit`]s for, and what it syncs the reserved `text`
    /// context on.
    ///
    /// **This is the panel being open, and not the tree's engagement.** A
    /// console prompt takes typing by construction — there is nothing else on
    /// the panel to type at — where the tree's `:engaged` is a fact about a
    /// build, and a build lags the key that opened the panel by a frame. Reading
    /// the tree here would drop the first character somebody typed, which is
    /// what the panel's own edit queue exists to stop. Use
    /// [`ConsolePanel::is_editing`](crcbl_ui::console::ConsolePanel::is_editing)
    /// for the tree's answer.
    #[must_use]
    pub const fn is_editing(&self) -> bool {
        self.open
    }

    /// A key the on-screen keyboard was tapped on, applied to the field.
    ///
    /// The same two edits [`Console::key`] makes for the physical `Backspace`
    /// and for a [`ShellEvent::TextCommit`], reached from the other input
    /// stream: a tapped `q` and a typed `q` have to leave the field, the
    /// history and the completion cycle in the same state, and the way to be
    /// sure of that is for both to end up here.
    fn tapped(&mut self, cap: KeyCap) {
        let edit = match cap {
            KeyCap::Type(character) => Edit::Insert(character.to_string()),
            KeyCap::Backspace => Edit::Backspace,
            // `TouchKeyboard::point` swallows the two layer keys and the panel
            // turns `Enter` into a submission, so none of the three arrives
            // here; they change the keyboard or the line, not the caret.
            KeyCap::Enter | KeyCap::Shift | KeyCap::Symbols => return,
        };
        self.panel.edit(edit);
        self.clear_cycle();
    }

    /// Notes that a contact has reached this run, which is what puts the
    /// on-screen keyboard on screen.
    ///
    /// **The first finger to land is the evidence**, not
    /// [`ShellCaps::TOUCH`](crcbl_shell::ShellCaps::TOUCH) — a desktop with a
    /// touchscreen sets that too, and a developer with a keyboard would lose
    /// [`KEYBOARD_HEIGHT_FRACTION`](crcbl_ui::console::KEYBOARD_HEIGHT_FRACTION)
    /// of the frame to keys they will never press.
    /// [`PauseControl`](crate::engine::PauseControl) gates its own button on
    /// the same evidence for the same reason, and neither moves a golden frame:
    /// a headless golden never touches glass.
    ///
    /// It never goes back. A run that has seen a finger is a run being played
    /// with one, and a keyboard that vanished the moment a mouse moved would be
    /// a keyboard that disappears under the thumb of anyone on a laptop with
    /// both.
    pub const fn note_contact(&mut self) {
        self.panel.show_keyboard(true);
    }

    /// Emits the panel [`Console::frame`] built this frame.
    ///
    /// Nothing at all when the console is shut, or before the first
    /// [`Console::frame`] of a run that opened it.
    pub fn draw(&mut self, dl: &mut DrawList, atlas: &FontAtlas) {
        if !self.open {
            return;
        }
        let Some(layout) = self.layout.as_ref() else {
            return;
        };
        self.panel.render(dl, layout, atlas);
    }

    /// One key press, while the console is open. Reports whether the console
    /// acted on it.
    ///
    /// Every key that **edits** the line is absent from this match, deliberately
    /// — `Backspace`, `Delete`, the caret arrows, `Home`, `End` and the
    /// clipboard shortcuts are [`Edit`]s the loop's text pump makes, applied by
    /// [`Ui::text_input`](crcbl_ui::tree::Ui::text_input) itself. Two paths onto
    /// one line would be two places for the caret to be.
    fn key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Enter => self.line_asks.push(LineAsk::Submit),
            KeyCode::Tab => self.line_asks.push(LineAsk::Complete),
            KeyCode::ArrowUp => self.line_asks.push(LineAsk::Older),
            KeyCode::ArrowDown => self.line_asks.push(LineAsk::Newer),
            KeyCode::PageUp => self.panel.log_mut().scroll_by(page_step(self.page)),
            KeyCode::PageDown => self.panel.log_mut().scroll_by(-page_step(self.page)),
            CONSOLE_LEVEL_KEY => self.cycle_level(),
            _ => return false,
        }
        true
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

    /// Runs this game's `autoexec.cfg` through this console, if there is one.
    ///
    /// [`Loop::new`](crate::engine::Loop::new) calls this once, before the first
    /// frame: `console_config::run_autoexec` is what decides whether there is a
    /// file to run at all, and this is the console half of it — the
    /// same registry, the same host and the same log every typed line goes
    /// through, so a variable an autoexec sets is set for the run and not for a
    /// context of its own.
    ///
    /// **No prompt line is echoed.** A typed line echoes one because somebody
    /// typed it and the terminal has to show the exchange; nobody typed this.
    /// Everything the file printed does reach the log, faults included: a
    /// start-up that half-applied says so in the panel and on stderr.
    pub fn run_autoexec(&mut self) -> crate::console_config::Autoexec {
        let (did, lines, clear) = {
            let Self { registry, host, .. } = self;
            let mut cx = Context::new(registry, host);
            let did = crate::console_config::run_autoexec(&mut cx);
            let clear = cx.clear_requested();
            (did, cx.into_lines(), clear)
        };
        for printed in &lines {
            crcbl_core::log::console::print(printed);
        }
        if clear {
            self.panel.log_mut().clear();
        }
        did
    }

    /// Carries out one deferred [`LineAsk`] against the line the build just
    /// left in the field.
    fn apply(&mut self, ask: LineAsk) {
        match ask {
            LineAsk::Submit => {
                if let Some(line) = self.panel.submit() {
                    self.run(&line);
                }
                self.clear_cycle();
            }
            LineAsk::Complete => self.complete(),
            LineAsk::Older => {
                let current = self.panel.line().to_owned();
                if let Some(line) = self.history.up(&current).map(str::to_owned) {
                    self.panel.set_line(&line);
                }
                self.clear_cycle();
            }
            LineAsk::Newer => {
                if let Some(line) = self.history.down().map(str::to_owned) {
                    self.panel.set_line(&line);
                }
                self.clear_cycle();
            }
        }
    }

    /// `Tab`: fill in the prefix every candidate shares, then cycle them.
    fn complete(&mut self) {
        if !self.cycle.is_empty() {
            let next = self.cycle_at.map_or(0, |at| (at + 1) % self.cycle.len());
            self.cycle_at = Some(next);
            let filled = format!("{}{}", self.cycle_stem, self.cycle[next]);
            self.panel.set_line(&filled);
            self.show_candidates();
            return;
        }

        let text = self.panel.line().to_owned();
        let partial = completing(&text);
        let stem = text[..text.len() - partial.len()].to_owned();
        let completion = self.registry.complete(&text);
        if completion.candidates.is_empty() {
            self.panel.clear_completion();
            return;
        }
        let filled = format!("{stem}{}", completion.common);
        self.panel.set_line(&filled);
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
pub fn engine_tables() -> [(&'static str, Table); 3] {
    [
        ("crcbl-core", crcbl_core::console_table()),
        ("crcbl-render", crcbl_render::console_table()),
        ("crcbl", crate::console_table()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chord prints as it is pressed, which is how `ui_prev`'s Shift+Tab
    /// reads in a `bind` listing.
    #[test]
    fn a_chord_prints_its_modifier_and_key() {
        let chord = Binding::Chord {
            modifier: crcbl_input::Modifier::Shift,
            key: KeyCode::Tab,
        };
        assert_eq!(binding_name(&chord), "Shift+Tab");
    }

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

    /// **A run with no settings file runs no autoexec through the console
    /// either**, and puts nothing in the log on its way to deciding that.
    ///
    /// The engine's own call, over a real [`Console`] rather than over a
    /// [`Context`] a check built: `Loop::new` reaches
    /// [`crate::console_config::run_autoexec`] only through this method, so a
    /// method that forwarded to the wrong thing — or forwarded and then printed
    /// anyway — would be caught nowhere else. The answer is asserted as well as
    /// the silence because a golden run's silence and a machine with no
    /// `autoexec.cfg` look identical from here.
    #[test]
    fn the_console_runs_no_autoexec_for_a_run_with_no_settings_file() {
        let logs = crcbl_core::log::capture();
        let tables: Vec<Table> = engine_tables()
            .into_iter()
            .map(|(_, table)| table)
            .collect();
        let host = ConsoleHost::new(crcbl_store::settings::SettingsStack::new());
        let mut console = Console::new(&tables, host);

        let did = console.run_autoexec();

        assert_eq!(
            did,
            crate::console_config::Autoexec::NoSettingsFile,
            "a console with nowhere to save must not read a config directory either",
        );
        let printed: Vec<String> = logs
            .records()
            .into_iter()
            .filter(|record| record.target == crcbl_core::log::console::CONSOLE_TARGET)
            .map(|record| record.message)
            .collect();
        assert!(printed.is_empty(), "{printed:?}");
    }
}
