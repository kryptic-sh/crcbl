//! The console panel: where the log, the prompt, the field, the **Send** button
//! and the completion rows sit, and what they draw.
//!
//! Debug-console decision 6 in `docs/notes/tooling.md`, on the element tree —
//! UI rung 7d2. [`ConsolePanel::layout`] begins the
//! frame, builds the tree and lays it out; [`ConsolePanel::point`] and
//! [`ConsolePanel::render`] only read what it built. So a test can ask where a
//! thing is without a draw list, and the pointer is hit-tested against the same
//! rectangles the frame was drawn from.
//!
//! # One build a frame
//!
//! The build is where the **Send** button latches its click and where the
//! field takes the frame's edits, so building twice would do both twice. That
//! is why the scale is chosen by `probe`'s arithmetic rather than by laying
//! the panel out once per candidate scale, and why
//! `the_scale_the_probe_chose_is_the_scale_the_tree_delivers` exists: the
//! arithmetic is a claim about the tree, and that test is what holds it to one.
//!
//! # What comes from where
//!
//! [`Menu`](crate::menu::Menu)'s split. Every **length** is [`ConsoleStyle`]'s
//! — a whole-number pixel-art scale times a base metric — and goes on the nodes
//! inline, because a stylesheet has no arithmetic to scale with. The
//! **structure** is `default.css`'s `console*` rules: the column, the log box's
//! clip and its flex-end packing, the field's row, the candidate list hanging
//! out of the flow. The colours [`ConsoleStyle`] owns go inline beside its
//! lengths; the two skins that change with a pointer — the **Send** button's
//! three states and the input's selection, caret and engaged border — are
//! `default.css`'s, because an inline declaration wins over every rule and so
//! could not have a `:hover` at all.

use glam::Vec2;

use crate::draw_list::DrawList;
use crate::edit::{Edit, Motion};
use crate::menu::MenuStyle;
use crate::style::{Declaration, SheetId, Sides};
use crate::text::FontAtlas;
use crate::tree::{
    AvailableSpace, Behavior, ClipboardRequest, Length, LengthAuto, NodeKey, Position, TextInput,
    Ui,
};
use crate::widget::{ButtonState, NATURAL_FONT_SIZE, PointerInput, UiState, WidgetId};

use super::keyboard::{KeyCap, KeyboardLayout, TouchKeyboard};
use super::{ConsoleStyle, LogView};

/// The share of the frame's height the console drops down over.
///
/// Source's drop-down, at a bit under half: enough that a stack trace or a
/// `help` listing is read without scrolling, and little enough that the game
/// behind it is still worth having on screen — which is the whole reason a
/// console is drawn over a running frame rather than pausing it.
pub const CONSOLE_HEIGHT_FRACTION: f32 = 0.45;

/// The most candidates the completion list offers at once.
///
/// A list longer than this is a prefix that has not been narrowed yet, and
/// `find` is the command for reading a long list.
pub const COMPLETION_ROWS: usize = 8;

/// What the input row is prefixed with — Source's prompt.
pub const PROMPT: &str = "] ";

/// The label on the button that submits the line.
pub const SEND_LABEL: &str = "SEND";

/// The ceiling the console's own [`WidgetId`]s sit below.
///
/// At the top of the range because a [`UiState`] is shared by every widget
/// driven through it and the ids are the caller's own: a game numbering its
/// buttons from zero never reaches this one, and a console that is given its
/// own [`UiState`] never has to.
///
/// **Nothing interacts under this id itself.** The **Send** button is a node of
/// the element tree and is hit-tested by the tree's own identity, not by a
/// [`WidgetId`]; what is left below here is the on-screen keyboard's block of
/// ids — see [`KEY_ID_BASE`](super::KEY_ID_BASE), which is measured down from
/// this figure.
pub const SEND_ID: WidgetId = WidgetId::MAX;

/// The framebuffer [`ConsolePanel::new`]'s warm-up build is laid out over.
///
/// Any size would do — the build exists for the field's identity and the first
/// real frame lays out over the framebuffer's own extent — so this is the one
/// the console's own tests open at.
const WARM_UP_EXTENT: Vec2 = Vec2::new(960.0, 720.0);

/// The fewest log rows a console is worth opening with.
///
/// The floor under [`ConsolePanel::layout`]'s scale choice, and the reason a
/// small window gets small glyphs: a panel showing two lines of a log is a
/// panel that has to be scrolled to read anything, and the fix for that is to
/// draw the text smaller rather than to drop the log.
pub const MINIMUM_LOG_ROWS: usize = 6;

/// The fewest columns the input line is worth typing into.
///
/// The other half of the scale choice. `anisotropic_filtering 16` is 24
/// columns, and a field narrower than the longest settings key and its value
/// makes the tree scroll the line sideways under the caret, which is a line
/// nobody can read whole.
pub const MINIMUM_FIELD_COLUMNS: usize = 24;

/// What one frame of pointer input at the console produced.
///
/// Returned by [`ConsolePanel::point`] rather than applied by it, so a tapped
/// key and a typed one reach the field through the caller's one editing path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleInput {
    /// The pointer did nothing the console has anything to say about.
    Nothing,
    /// **Send** submitted this line, which is what `Enter` would have.
    Submitted(String),
    /// The on-screen keyboard was tapped on this key.
    ///
    /// Never [`KeyCap::Shift`] or [`KeyCap::Symbols`], which change the
    /// keyboard's own layer and are swallowed by
    /// [`TouchKeyboard::point`](super::TouchKeyboard::point), and never
    /// [`KeyCap::Enter`], which comes back as [`ConsoleInput::Submitted`].
    Key(KeyCap),
}

/// The console's widgets and the state that outlives a frame.
///
/// Everything it draws is given to it: [`LogView::push_records`] takes the
/// records the ring handed over and [`ConsolePanel::set_completion`] takes the
/// candidates the registry answered with. The panel resolves no name, reads no
/// variable and knows no keycode.
///
/// **Not [`Clone`].** It owns the [`Ui`] its tree lives in — the node store
/// keyed by identity, each node's resolved style and Taffy cache — and a copy
/// of that is a second tree with the same keys, not a second console.
#[derive(Debug)]
pub struct ConsolePanel {
    /// The tree the panel is built, laid out and drawn in. One, kept between
    /// frames: that is what makes a node's identity — its focus, its
    /// engagement, its caret and its selection — survive the rebuild.
    ui: Ui,
    /// The line being typed. The tree's text input edits this `String`; the
    /// panel owns it, so [`ConsolePanel::line`] is a read of the value and not
    /// of a widget's copy of it.
    line: String,
    log: LogView,
    /// The token the candidates were matched from — its length is what is
    /// highlighted at the head of each of them.
    prefix: String,
    candidates: Vec<String>,
    /// The edits waiting for a frame the field is engaged on; see
    /// [`ConsolePanel::edit`].
    queued: Vec<Edit>,
    /// The **Send** button's appearance, as the last build resolved it.
    send: ButtonState,
    /// Whether that build latched a click on **Send**, for
    /// [`ConsolePanel::point`] to report.
    send_clicked: bool,
    /// The field's node, for engaging it and for reading its box back. `None`
    /// until the first build.
    field_key: Option<NodeKey>,
    /// Whether the tree reported the field engaged after the last build.
    editing: bool,
    /// The panel's own stylesheet — see [`scale_sheet`] — and the scale it was
    /// last written at, so a steady frame does not replace it.
    sheet: SheetId,
    sheet_scale: f32,
    /// The on-screen keyboard, which is only laid out and drawn while
    /// [`keyboard_shown`](ConsolePanel::keyboard_shown) is set.
    keyboard: TouchKeyboard,
    keyboard_shown: bool,
}

impl ConsolePanel {
    /// An empty panel: no lines, no typed text, no candidates, and no on-screen
    /// keyboard — **with its prompt already engaged**.
    ///
    /// # The constructor builds the tree once, and draws nothing
    ///
    /// A console prompt is the only thing on the panel that typing can go to,
    /// so the field is engaged from the moment the panel exists rather than
    /// from a click or an accept. [`Ui::engage`] needs a [`NodeKey`] and a key
    /// only comes out of a build, so this builds the tree once — at a nominal
    /// extent, over an empty log, with no pointer and no text
    /// input — for the field's identity alone. Nothing is emitted from it and
    /// the first real [`ConsolePanel::layout`] replaces every node of it.
    ///
    /// It is what makes the **first** frame the console is shown take that
    /// frame's edits: an input engaged on the frame the engagement began
    /// deliberately takes none, so a panel that engaged on its first laid-out
    /// frame would swallow whatever was typed on it.
    #[must_use]
    pub fn new() -> Self {
        let style = ConsoleStyle::pixel_art(1);
        let mut ui = Ui::new();
        let sheet = ui.add_stylesheet("crcbl-ui console", &scale_sheet(&style));
        let mut panel = Self {
            ui,
            line: String::new(),
            log: LogView::new(),
            prefix: String::new(),
            candidates: Vec::new(),
            queued: Vec::new(),
            send: ButtonState::Idle,
            send_clicked: false,
            field_key: None,
            editing: false,
            sheet,
            sheet_scale: style.scale,
            keyboard: TouchKeyboard::new(),
            keyboard_shown: false,
        };
        let atlas = FontAtlas::built_in();
        panel.ui.begin_frame(PointerInput::default());
        panel.ui.set_text_input(TextInput::default());
        let field = {
            let Self {
                ui,
                line,
                log,
                prefix,
                candidates,
                ..
            } = &mut panel;
            let built = build(
                ui,
                line,
                log,
                prefix,
                candidates,
                WARM_UP_EXTENT,
                &style,
                &atlas,
            );
            ui.layout(Vec2::ZERO, AvailableSpace::definite(WARM_UP_EXTENT), &atlas);
            built.field
        };
        panel.ui.engage(field);
        panel.field_key = Some(field);
        panel.editing = panel.ui.text_editing();
        panel
    }

    /// The on-screen keyboard, to read — what a test asserts a layer against.
    #[must_use]
    pub const fn keyboard(&self) -> &TouchKeyboard {
        &self.keyboard
    }

    /// Whether the on-screen keyboard is laid out and drawn at all.
    ///
    /// **Off by default**, because the keyboard is for a device that has no
    /// other one: a machine with keys would lose
    /// [`KEYBOARD_HEIGHT_FRACTION`](super::KEYBOARD_HEIGHT_FRACTION) of its
    /// frame to a control it will never press. Whose device this is, is the
    /// caller's question — `crcbl::debug_console::Console` answers it from the
    /// first contact the run reports.
    #[must_use]
    pub const fn keyboard_shown(&self) -> bool {
        self.keyboard_shown
    }

    /// Shows or hides the on-screen keyboard.
    pub const fn show_keyboard(&mut self, shown: bool) {
        self.keyboard_shown = shown;
    }

    /// The line being typed.
    #[must_use]
    pub fn line(&self) -> &str {
        &self.line
    }

    /// Replaces the line and puts the caret at its end.
    ///
    /// What a history recall and a completion fill both do: the caret goes
    /// where the typing would continue, which is after the text that arrived.
    /// The caret move is [queued](ConsolePanel::edit) rather than applied,
    /// because the caret is the tree's and the tree moves it on its next build.
    pub fn set_line(&mut self, text: &str) {
        self.line.clear();
        self.line.push_str(text);
        self.edit(Edit::Move {
            motion: Motion::End,
            select: false,
        });
    }

    /// Queues one edit for the field on the next frame: what a tapped
    /// on-screen key makes, so a tapped `q` and a typed `q` are one path.
    ///
    /// **Held until the field is engaged.** The tree takes edits only from an
    /// input engaged since an earlier frame, so a key pressed on the frame the
    /// console opened would otherwise be dropped. Queued edits are handed over
    /// in the order they arrived on the first frame the field takes any.
    pub fn edit(&mut self, edit: Edit) {
        self.queued.push(edit);
    }

    /// Whether the tree reports the field engaged — what the caller syncs the
    /// reserved `text` context on.
    ///
    /// The **tree's** answer, as of the last [`ConsolePanel::layout`]: false
    /// before the first one, and false for the frame a click elsewhere in the
    /// panel committed the field and the next build has not re-engaged it. A
    /// caller that wants "typing belongs to the console" rather than "the tree
    /// is editing this instant" should ask whether the console is open.
    #[must_use]
    pub const fn is_editing(&self) -> bool {
        self.editing
    }

    /// The log the panel shows.
    #[must_use]
    pub const fn log(&self) -> &LogView {
        &self.log
    }

    /// The log the panel shows, to push records into and to scroll.
    pub const fn log_mut(&mut self) -> &mut LogView {
        &mut self.log
    }

    /// The **Send** button's appearance, as this frame's build resolved it.
    #[must_use]
    pub const fn send_state(&self) -> ButtonState {
        self.send
    }

    /// The candidates the completion list is offering.
    #[must_use]
    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }

    /// The token the candidates were matched from.
    #[must_use]
    pub fn completion_prefix(&self) -> &str {
        &self.prefix
    }

    /// Offers `candidates`, each matched from `prefix`.
    ///
    /// `prefix` is a **length**, in effect: the panel highlights that many
    /// characters at the head of every candidate rather than looking for the
    /// prefix inside it, because the registry matches without regard to case
    /// and answers with the declared spelling — `R_AO` typed against
    /// `r_ao_view` matches four characters that are not the four that were
    /// typed.
    pub fn set_completion(&mut self, prefix: &str, candidates: &[&str]) {
        self.prefix.clear();
        self.prefix.push_str(prefix);
        self.candidates.clear();
        self.candidates
            .extend(candidates.iter().map(|name| (*name).to_owned()));
    }

    /// Drops the candidates — what an edit that is no longer a completion does.
    pub fn clear_completion(&mut self) {
        self.prefix.clear();
        self.candidates.clear();
    }

    /// Takes the typed line, empties the field and returns to the newest log
    /// lines.
    ///
    /// What `Enter` calls, and what a click on **Send** calls through
    /// [`ConsolePanel::point`], so the two cannot come to mean different
    /// things. A line with nothing but whitespace in it is not a command:
    /// the field is cleared and the answer is `None`.
    pub fn submit(&mut self) -> Option<String> {
        let line = std::mem::take(&mut self.line);
        self.clear_completion();
        // A command's own output lands at the bottom of the log, so a reader
        // who had scrolled back is put where the answer will appear.
        self.log.scroll_to_bottom();
        if line.trim().is_empty() {
            return None;
        }
        Some(line)
    }

    /// The clipboard requests this frame's field made, for the caller to carry
    /// to the platform — [`Ui::take_clipboard_requests`].
    pub fn take_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        self.ui.take_clipboard_requests()
    }

    /// What this frame's pointer produced: a line **Send** or the on-screen
    /// keyboard's return key submitted, or a key the keyboard was tapped on.
    /// Call it after [`ConsolePanel::layout`], with the same pointer.
    ///
    /// The **Send** button's click was resolved by the build — the tree
    /// hit-tests its own nodes — so this reports it rather than testing a
    /// rectangle a second time. The on-screen keyboard keeps its own hit test
    /// and its own [`UiState`] capture, which is what `ui` is for.
    ///
    /// The keyboard's key is handed back rather than applied here, so that a
    /// tapped `q` and a typed `q` reach the field down **one** path: the caller
    /// owns the completion cycle a keystroke drops, and a panel that edited the
    /// field behind its back would leave that cycle stale.
    pub fn point(
        &mut self,
        layout: &ConsoleLayout,
        ui: &mut UiState,
        pointer: PointerInput,
    ) -> ConsoleInput {
        if std::mem::take(&mut self.send_clicked) {
            return self
                .submit()
                .map_or(ConsoleInput::Nothing, ConsoleInput::Submitted);
        }
        // Only when it is showing: a keyboard that hit-tested while hidden
        // would claim taps over a rectangle nothing is drawn in.
        if !self.keyboard_shown {
            return ConsoleInput::Nothing;
        }
        match self.keyboard.point(layout.keyboard(), ui, pointer) {
            None => ConsoleInput::Nothing,
            // Through the same `submit` **Send** and `Enter` call, so a line
            // sent from the keyboard and one sent from the button leave the
            // field, the log's scroll and the completion in one state.
            Some(KeyCap::Enter) => self
                .submit()
                .map_or(ConsoleInput::Nothing, ConsoleInput::Submitted),
            Some(cap) => ConsoleInput::Key(cap),
        }
    }

    /// Begins the panel's frame and builds it over an `extent`-sized
    /// framebuffer at the largest scale that stays readable.
    ///
    /// The scale is a pure function of the extent: the largest whole number up
    /// to [`MenuStyle::MAX_SCALE`] whose panel still shows [`MINIMUM_LOG_ROWS`]
    /// rows of log and [`MINIMUM_FIELD_COLUMNS`] columns of input, and one when
    /// none of them do, and `probe` is that arithmetic. Whole numbers because the
    /// glyphs are a bitmap, and the two floors because a console that is bigger
    /// than it is useful is not a better console.
    ///
    /// Call it **once a frame** while the console is open: this is the only
    /// place the tree is built, so a second call would latch the **Send**
    /// button's click twice and take the field's edits twice.
    pub fn layout(
        &mut self,
        extent: (u32, u32),
        atlas: &FontAtlas,
        pointer: PointerInput,
        input: TextInput,
    ) -> ConsoleLayout {
        let screen = Vec2::new(extent.0 as f32, extent.1 as f32);
        let mut chosen = ConsoleStyle::pixel_art(1);
        for scale in 2..=MenuStyle::MAX_SCALE {
            let style = ConsoleStyle::pixel_art(scale);
            if fits(screen, atlas, &style) {
                chosen = style;
            } else {
                break;
            }
        }
        self.layout_with(extent, atlas, &chosen, pointer, input)
    }

    /// Lays the panel out at a style the caller chose — what a test asserting
    /// one scale calls.
    ///
    /// Hit-tests `pointer` against last frame's rectangles, takes `input`'s
    /// edits and clipboard answers into the field, keeps the field engaged, and
    /// lays the tree out.
    pub fn layout_with(
        &mut self,
        extent: (u32, u32),
        atlas: &FontAtlas,
        style: &ConsoleStyle,
        pointer: PointerInput,
        input: TextInput,
    ) -> ConsoleLayout {
        let screen = Vec2::new(extent.0 as f32, extent.1 as f32);
        // Before the frame begins, because a replaced sheet takes effect there
        // — and only on a scale change, because each one re-resolves the tree.
        if style.scale != self.sheet_scale {
            if let Err(errors) = self.ui.replace_stylesheet(self.sheet, &scale_sheet(style)) {
                crcbl_core::warn!("console: its own stylesheet did not parse: {errors:?}");
            } else {
                self.sheet_scale = style.scale;
            }
        }
        self.ui.begin_frame(pointer);

        // The queue drains only onto a frame the field will take edits on; see
        // `ConsolePanel::edit`. `begin_frame` has just resolved that: the field
        // takes edits while it was engaged before the frame began, and a click
        // elsewhere in the panel — on **Send**, say — has committed it by here.
        // The clock and the clipboard's answers go through whatever the field's
        // engagement is, because the caret's blink and an answer to a request an
        // earlier frame made are not edits.
        self.queued.extend(input.edits);
        let engaged = self
            .field_key
            .is_some_and(|key| self.ui.engaged() == Some(key));
        let edits = if engaged {
            std::mem::take(&mut self.queued)
        } else {
            Vec::new()
        };
        self.ui.set_text_input(TextInput {
            dt: input.dt,
            edits,
            clipboard: input.clipboard,
        });

        let built = {
            let Self {
                ui,
                line,
                log,
                prefix,
                candidates,
                ..
            } = self;
            let built = build(ui, line, log, prefix, candidates, screen, style, atlas);
            ui.layout(Vec2::ZERO, AvailableSpace::definite(screen), atlas);
            built
        };

        // **Engaged for as long as the console is open.** A console prompt
        // always takes typing, so a click on Send — or anywhere else in the
        // panel — that committed the field takes it back on the next frame.
        // `Ui::engage` engages now rather than next frame, so that frame
        // reports the field engaged rather than freshly begun, and the edits it
        // carries are taken rather than swallowed.
        if self.ui.engaged() != Some(built.field) {
            self.ui.engage(built.field);
        }
        self.field_key = Some(built.field);
        self.editing = self.ui.text_editing();
        self.send = built.send_state;
        self.send_clicked = built.send_clicked;

        let rect = |key| self.ui.rect(key).expect("laid out this frame");
        // Laid out only when it is showing, so a hidden keyboard claims no
        // pointer and costs no allocation on the frames nobody is typing.
        let keyboard = if self.keyboard_shown {
            self.keyboard.layout(screen)
        } else {
            KeyboardLayout::default()
        };
        ConsoleLayout {
            style: *style,
            screen,
            panel: rect(built.panel),
            log: rect(built.log),
            field: rect(built.field_well),
            prompt_pos: rect(built.prompt).0,
            text_pos: rect(built.field).0,
            input_right: rect(built.field).1.x,
            send: rect(built.send),
            completion: built.candidates.iter().copied().map(rect).collect(),
            keyboard,
        }
    }

    /// Draws the tree [`ConsolePanel::layout`] built, then the on-screen
    /// keyboard over it.
    ///
    /// The tree's own paint order: the panel's fill, the log's rows inside its
    /// clip, the field's well and what is on it, the **Send** button, and the
    /// candidate rows hanging below the panel. The keyboard is drawn **after**
    /// the candidates, which can reach it on a short frame — it is the half a
    /// finger presses, so it is the half that stays on top.
    ///
    /// The caret's blink is the tree's, off [`TextInput::dt`]; nothing here
    /// reads a clock.
    pub fn render(&self, dl: &mut DrawList, layout: &ConsoleLayout, atlas: &FontAtlas) {
        self.ui.emit(dl);
        if self.keyboard_shown {
            self.keyboard
                .render(dl, layout.keyboard(), atlas, layout.style());
        }
    }
}

impl Default for ConsolePanel {
    fn default() -> Self {
        Self::new()
    }
}

/// The nodes one build made, for reading a layout back — [`Menu`]'s
/// `BuiltMenu`.
///
/// [`Menu`]: crate::menu::Menu
struct Built {
    panel: NodeKey,
    log: NodeKey,
    /// The well holding the prompt and the input.
    field_well: NodeKey,
    /// The text input itself.
    field: NodeKey,
    prompt: NodeKey,
    send: NodeKey,
    candidates: Vec<NodeKey>,
    send_state: ButtonState,
    send_clicked: bool,
}

/// Builds the panel into `ui` at `style`: the tree [`ConsolePanel::layout_with`]
/// lays out, reads back and draws, one for all three.
///
/// A free function rather than a method because it borrows four of the panel's
/// fields at once beside the tree, and `line` mutably: the text input edits the
/// panel's own `String`.
#[expect(
    clippy::too_many_arguments,
    reason = "the panel's fields, split so the tree and the line can be borrowed apart"
)]
fn build(
    ui: &mut Ui,
    line: &mut String,
    log: &LogView,
    prefix: &str,
    candidates: &[String],
    screen: Vec2,
    style: &ConsoleStyle,
    atlas: &FontAtlas,
) -> Built {
    use Declaration as D;
    let px = LengthAuto::Px;
    let pad = style.padding;
    let row = style.row_height();
    let panel_height = (screen.y * CONSOLE_HEIGHT_FRACTION).round();

    let screen_style = [
        D::Width(px(screen.x)),
        D::Height(px(screen.y)),
        D::FontSize(style.text_size),
    ];
    let panel_style = [
        D::Width(LengthAuto::Percent(1.0)),
        D::Height(px(panel_height)),
        D::Padding(Sides::Top, Length::Px(pad.y)),
        D::Padding(Sides::Bottom, Length::Px(pad.y)),
        D::Padding(Sides::Left, Length::Px(pad.x)),
        D::Padding(Sides::Right, Length::Px(pad.x)),
        // The drop-down's leading edge, so the panel reads as a thing over the
        // frame rather than as a tint on the top of it. A border takes part in
        // layout, unlike the line this was drawn as before the tree, so it eats
        // its own width out of the panel rather than straddling its edge.
        D::BorderWidth(Sides::Bottom, style.scale),
        D::RowGap(Length::Px(pad.y)),
        D::Background(style.panel_color),
        D::BorderColor(style.border_color),
    ];
    let well_style = [
        D::Padding(Sides::Top, Length::Px(pad.y)),
        D::Padding(Sides::Bottom, Length::Px(pad.y)),
        D::Padding(Sides::Left, Length::Px(pad.x)),
        D::Padding(Sides::Right, Length::Px(pad.x)),
        D::BorderWidth(Sides::All, style.scale),
        D::Background(style.field_color),
        D::BorderColor(style.border_color),
    ];
    let send_style = [
        D::Padding(Sides::Top, Length::Px(pad.y)),
        D::Padding(Sides::Bottom, Length::Px(pad.y)),
        D::Padding(Sides::Left, Length::Px(pad.x)),
        D::Padding(Sides::Right, Length::Px(pad.x)),
        D::BorderWidth(Sides::All, style.scale),
        D::Margin(Sides::Left, px(pad.x)),
    ];

    // The rows the log offers the tree. More than the box can show, because
    // the box clips what does not fit and packs the rest against its bottom
    // edge — so the oldest of these are the ones that fall off the top, which
    // is what "newest at the bottom" is here. The wrap is the panel's content
    // width, which is the frame less the panel's padding.
    let advance = style.advance(atlas);
    let columns = if advance > 0.0 {
        ((screen.x - 2.0 * pad.x) / advance).max(0.0) as usize
    } else {
        0
    };
    let rows = if row > 0.0 {
        (panel_height / row).max(0.0) as usize + 1
    } else {
        0
    };
    let visible = log.visible_rows(rows, columns);

    let mut panel_key = None;
    let mut log_key = None;
    let mut well_key = None;
    let mut input_key = None;
    let mut prompt_key = None;
    let mut send_key = None;
    let mut rows_built = Vec::new();
    let mut send_state = ButtonState::Idle;
    let mut send_clicked = false;

    ui.block("console-screen", &screen_style, |ui| {
        panel_key = Some(
            ui.block("console", &panel_style, |ui| {
                log_key = Some(
                    ui.block(".console-log", &[], |ui| {
                        for (level, text) in &visible {
                            ui.span(
                                ".console-line",
                                text.as_str(),
                                &[D::Color(style.level_color(*level))],
                            );
                        }
                    })
                    .key,
                );
                ui.block(".console-row", &[], |ui| {
                    well_key = Some(
                        ui.block(".console-field", &well_style, |ui| {
                            prompt_key = Some(
                                ui.span(".console-prompt", PROMPT, &[D::Color(style.prompt_color)])
                                    .key,
                            );
                            input_key = Some(ui.text_input(".console-input", line).key);
                        })
                        .key,
                    );
                    let send =
                        ui.block_with("button.console-send", &send_style, Behavior::BUTTON, |ui| {
                            ui.span(".button-label", SEND_LABEL, &[]);
                        });
                    send_key = Some(send.key);
                    send_clicked = send.clicked;
                    send_state = if send.pressed {
                        ButtonState::Pressed
                    } else if send.hovered {
                        ButtonState::Hovered
                    } else {
                        ButtonState::Idle
                    };
                });
            })
            .key,
        );

        // The candidate list hangs below the panel, out of the flow, lined up
        // with the typed token rather than with the panel's edge — Source's.
        // Only rows that fit whole between the panel and the bottom of the
        // frame: it is drawn outside the panel, so nothing else stops one from
        // hanging off the screen.
        let fits = if row > 0.0 {
            ((screen.y - panel_height) / row).max(0.0) as usize
        } else {
            0
        };
        let count = candidates.len().min(COMPLETION_ROWS).min(fits);
        if count == 0 {
            return;
        }
        let left = (text_inset(atlas, style) - pad.x).max(0.0);
        let list_style = [
            D::Position(Position::Absolute),
            D::Inset(Sides::Top, px(panel_height)),
            D::Inset(Sides::Left, px(left)),
            D::MaxWidth(px((screen.x - left).max(0.0))),
            D::Background(style.completion_color),
        ];
        let candidate_style = [
            D::Height(px(row)),
            D::Padding(Sides::Left, Length::Px(pad.x)),
            D::Padding(Sides::Right, Length::Px(pad.x)),
        ];
        let matched = prefix.chars().count();
        ui.block(".console-completion", &list_style, |ui| {
            for (index, candidate) in candidates[..count].iter().enumerate() {
                let head: String = candidate.chars().take(matched).collect();
                let tail: String = candidate.chars().skip(matched).collect();
                rows_built.push(
                    ui.block_keyed(index, ".console-candidate", &candidate_style, |ui| {
                        if !head.is_empty() {
                            ui.span(
                                ".console-match",
                                head.as_str(),
                                &[D::Color(style.match_color)],
                            );
                        }
                        if !tail.is_empty() {
                            ui.span(
                                ".console-tail",
                                tail.as_str(),
                                &[D::Color(style.candidate_color)],
                            );
                        }
                    })
                    .key,
                );
            }
        });
    });
    Built {
        panel: panel_key.expect("the screen builds its panel"),
        log: log_key.expect("the panel builds its log"),
        field_well: well_key.expect("the input row builds its well"),
        field: input_key.expect("the well builds its input"),
        prompt: prompt_key.expect("the well builds its prompt"),
        send: send_key.expect("the input row builds its button"),
        candidates: rows_built,
        send_state,
        send_clicked,
    }
}

/// Where the typed line's em box starts, measured from the panel's left edge:
/// the panel's padding, then the field well's border and padding, then the
/// prompt.
///
/// What the candidate list is lined up with, and the one thing about the input
/// row's box that is needed **before** the tree lays it out.
/// `the_completion_rows_highlight_the_matched_head` asserts the list lands on
/// [`ConsoleLayout::text_pos`], which is read back off the laid-out tree, so
/// the two cannot drift.
fn text_inset(atlas: &FontAtlas, style: &ConsoleStyle) -> f32 {
    2.0f32.mul_add(
        style.padding.x,
        style.scale + atlas.text_width(PROMPT, style.text_size / NATURAL_FONT_SIZE),
    )
}

/// How many rows of log and columns of input a panel at `style` shows on a
/// `screen`-sized frame — [`ConsoleLayout::log_rows`] and
/// [`ConsoleLayout::field_columns`], worked out without laying a tree out.
///
/// **[`build`]'s box model, written as arithmetic.** The scale probe cannot lay
/// a tree out per candidate scale — there is one build a frame, and it latches
/// a click and consumes the frame's edits — so this is the claim the probe
/// chooses by, and `the_scale_the_probe_chose_is_the_scale_the_tree_delivers`
/// holds it to the tree it is a claim about, figure for figure.
///
/// Down the frame: the panel is [`CONSOLE_HEIGHT_FRACTION`] of it, with
/// `padding.y` above and below, a `scale`-wide bottom border, and a `padding.y`
/// gap between the log and the input row; the input row is one text row inside
/// the well's own `padding.y` and `scale` border. What is left is the log.
/// Across it: the panel's `padding.x` on both sides, then the **Send** button
/// and the `padding.x` margin before it, then the well's border and padding,
/// then the prompt. What is left is the input.
fn probe(screen: Vec2, atlas: &FontAtlas, style: &ConsoleStyle) -> (usize, usize) {
    let pad = style.padding;
    let row = style.row_height();
    let advance = style.advance(atlas);
    if row <= 0.0 || advance <= 0.0 {
        return (0, 0);
    }
    let glyphs = style.text_size / NATURAL_FONT_SIZE;

    let panel_height = (screen.y * CONSOLE_HEIGHT_FRACTION).round();
    let input_row = 2.0f32.mul_add(pad.y + style.scale, row);
    let log_height = panel_height - 3.0 * pad.y - style.scale - input_row;
    let rows = (log_height / row).max(0.0) as usize;

    let send = atlas.text_width(SEND_LABEL, glyphs) + 2.0 * (pad.x + style.scale);
    let well = screen.x - 3.0 * pad.x - send;
    let input = well - 2.0 * (pad.x + style.scale) - atlas.text_width(PROMPT, glyphs);
    let columns = (input / advance).max(0.0) as usize;

    (rows, columns)
}

/// Whether [`probe`] says a panel at `style` shows [`MINIMUM_LOG_ROWS`] rows of
/// log and [`MINIMUM_FIELD_COLUMNS`] columns of input.
fn fits(screen: Vec2, atlas: &FontAtlas, style: &ConsoleStyle) -> bool {
    let (rows, columns) = probe(screen, atlas, style);
    rows >= MINIMUM_LOG_ROWS && columns >= MINIMUM_FIELD_COLUMNS
}

/// The console's own stylesheet: the lengths of the tree text input's parts,
/// which are the panel's to scale and inline declarations cannot reach.
///
/// A widget's parts are built inside the widget, so the only way to give the
/// caret a width that grows with the panel is a rule; the sheet is rewritten
/// when — and only when — the chosen scale changes, because each change
/// re-resolves every node of the tree. The **colours** of those parts stay in
/// `default.css`: they do not scale, and this sheet is lengths only.
///
/// The caret's width is the only one. The selection block is placed and sized
/// by the tree after layout, from the boundaries of the glyphs it covers, and
/// the input's own padding and border are zeroed by `default.css` so the well
/// around it is its box.
fn scale_sheet(style: &ConsoleStyle) -> String {
    format!(
        "text-input.console-input > .text-input-caret {{ width: {}px; }}\n",
        style.caret_width,
    )
}

/// Where every part of the panel goes, for one frame at one size.
///
/// Read back off the tree [`ConsolePanel::layout`] built, and held rather than
/// recomputed between [`ConsolePanel::point`] and [`ConsolePanel::render`]: the
/// pointer must be tested against the rectangles the frame was actually drawn
/// from, and a second build is not something this panel does.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleLayout {
    style: ConsoleStyle,
    screen: Vec2,
    panel: (Vec2, Vec2),
    log: (Vec2, Vec2),
    field: (Vec2, Vec2),
    prompt_pos: Vec2,
    text_pos: Vec2,
    /// The input's own right edge, which is where its columns run out.
    input_right: f32,
    send: (Vec2, Vec2),
    completion: Vec<(Vec2, Vec2)>,
    keyboard: KeyboardLayout,
}

impl ConsoleLayout {
    /// The style everything here was laid out at.
    #[must_use]
    pub const fn style(&self) -> &ConsoleStyle {
        &self.style
    }

    /// The framebuffer this layout was made for.
    #[must_use]
    pub const fn screen(&self) -> Vec2 {
        self.screen
    }

    /// The panel itself: the top [`CONSOLE_HEIGHT_FRACTION`] of the frame.
    #[must_use]
    pub const fn panel(&self) -> (Vec2, Vec2) {
        self.panel
    }

    /// The box the log is drawn in. Its rows are clipped to it.
    #[must_use]
    pub const fn log(&self) -> (Vec2, Vec2) {
        self.log
    }

    /// The input row's box, behind the prompt and the typed line.
    #[must_use]
    pub const fn field(&self) -> (Vec2, Vec2) {
        self.field
    }

    /// The top-left of the [`PROMPT`]'s em box.
    #[must_use]
    pub const fn prompt_pos(&self) -> Vec2 {
        self.prompt_pos
    }

    /// The top-left of the typed line's em box, and so the caret's origin at
    /// the start of the line.
    ///
    /// The input's content box, unscrolled: a line longer than the box is
    /// scrolled under the caret by the tree, and the glyphs then start left of
    /// this and are clipped to the box.
    #[must_use]
    pub const fn text_pos(&self) -> Vec2 {
        self.text_pos
    }

    /// The **Send** button's rectangle.
    #[must_use]
    pub const fn send(&self) -> (Vec2, Vec2) {
        self.send
    }

    /// The candidate rows, top first, or empty when there are none to draw.
    #[must_use]
    pub fn completion(&self) -> &[(Vec2, Vec2)] {
        &self.completion
    }

    /// Where the on-screen keyboard's keys are, or a layout with no keys when
    /// it is not showing.
    #[must_use]
    pub const fn keyboard(&self) -> &KeyboardLayout {
        &self.keyboard
    }

    /// Whether the console is drawing anything over `point`.
    ///
    /// The panel **and** the on-screen keyboard, which is the whole reason this
    /// is one call rather than the caller testing the panel's rectangle: the
    /// keyboard is drawn outside the panel, along the frame's bottom edge, and a
    /// tap on it that fell through to the game would move the player every time
    /// a letter was typed.
    #[must_use]
    pub fn covers(&self, point: Vec2) -> bool {
        let inside = |(min, max): (Vec2, Vec2)| {
            point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
        };
        inside(self.panel) || (!self.keyboard.keys().is_empty() && self.keyboard.contains(point))
    }

    /// How many rows of log this layout shows.
    #[must_use]
    pub fn log_rows(&self) -> usize {
        LogView::rows_in(self.log, &self.style)
    }

    /// How many columns of text the input line holds.
    #[must_use]
    pub fn field_columns(&self, atlas: &FontAtlas) -> usize {
        LogView::columns_in(
            (self.text_pos, Vec2::new(self.input_right, self.text_pos.y)),
            atlas,
            &self.style,
        )
    }
}

#[cfg(test)]
mod tests;
