//! The console panel: where the log, the prompt, the field, the **Send** button
//! and the completion rows sit, and what they draw.
//!
//! `docs/plan/52-debug-console.md` decision 6. The panel is laid out first and
//! drawn second — [`ConsolePanel::layout`] answers with every rectangle, and
//! [`ConsolePanel::render`] only fills them in — so a test can ask where a thing
//! is without a draw list, and the pointer is hit-tested against the same
//! rectangles the frame was drawn from.

use glam::Vec2;

use crate::draw_list::DrawList;
use crate::menu::MenuStyle;
use crate::text::FontAtlas;
use crate::widget::{Button, ButtonState, NATURAL_FONT_SIZE, PointerInput, UiState, WidgetId};

use super::{ConsoleStyle, LogView, TextField};

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

/// The [`WidgetId`] the **Send** button interacts under.
///
/// At the top of the range because a [`UiState`] is shared by every widget
/// driven through it and the ids are the caller's own: a game numbering its
/// buttons from zero never reaches this one, and a console that is given its
/// own [`UiState`] never has to.
pub const SEND_ID: WidgetId = WidgetId::MAX;

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
/// columns, and a field that cannot hold the longest settings key and its value
/// scrolls sideways — which this field, having no horizontal scroll, cannot do.
pub const MINIMUM_FIELD_COLUMNS: usize = 24;

/// The console's widgets and the state that outlives a frame.
///
/// Everything it draws is given to it: [`LogView::push_records`] takes the
/// records the ring handed over and [`ConsolePanel::set_completion`] takes the
/// candidates the registry answered with. The panel resolves no name, reads no
/// variable and knows no keycode.
#[derive(Debug, Clone)]
pub struct ConsolePanel {
    field: TextField,
    log: LogView,
    /// The token the candidates were matched from — its length is what is
    /// highlighted at the head of each of them.
    prefix: String,
    candidates: Vec<String>,
    /// The **Send** button's appearance, as the last [`ConsolePanel::point`]
    /// resolved it.
    send: ButtonState,
}

impl ConsolePanel {
    /// An empty panel: no lines, no typed text, no candidates.
    #[must_use]
    pub fn new() -> Self {
        Self {
            field: TextField::new(),
            log: LogView::new(),
            prefix: String::new(),
            candidates: Vec::new(),
            send: ButtonState::Idle,
        }
    }

    /// The line being typed.
    #[must_use]
    pub const fn field(&self) -> &TextField {
        &self.field
    }

    /// The line being typed, to edit — what a key press reaches.
    pub const fn field_mut(&mut self) -> &mut TextField {
        &mut self.field
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

    /// The **Send** button's appearance, as the last [`ConsolePanel::point`]
    /// resolved it.
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
        let line = self.field.text().to_owned();
        self.field.clear();
        self.clear_completion();
        // A command's own output lands at the bottom of the log, so a reader
        // who had scrolled back is put where the answer will appear.
        self.log.scroll_to_bottom();
        if line.trim().is_empty() {
            return None;
        }
        Some(line)
    }

    /// Runs one frame of pointer input against `layout`, and reports a line the
    /// **Send** button submitted.
    ///
    /// Press capture goes through `ui`, so a press that starts on the button and
    /// is released off it submits nothing — the rule every other clickable
    /// widget in this crate follows.
    pub fn point(
        &mut self,
        layout: &ConsoleLayout,
        ui: &mut UiState,
        pointer: PointerInput,
    ) -> Option<String> {
        let (min, max) = layout.send();
        let inside = pointer.pos.x >= min.x
            && pointer.pos.x <= max.x
            && pointer.pos.y >= min.y
            && pointer.pos.y <= max.y;
        let (state, clicked) = ui.interact(SEND_ID, inside, pointer.down, pointer.released);
        self.send = state;
        if clicked { self.submit() } else { None }
    }

    /// Lays the panel out over an `extent`-sized framebuffer, at the largest
    /// scale that leaves it readable.
    ///
    /// The scale is a pure function of the extent: the largest whole number up
    /// to [`MenuStyle::MAX_SCALE`] whose panel still shows [`MINIMUM_LOG_ROWS`]
    /// rows of log and [`MINIMUM_FIELD_COLUMNS`] columns of input, and one when
    /// none of them do. Whole numbers because the glyphs are a bitmap, and the
    /// two floors because a console that is bigger than it is useful is not a
    /// better console.
    #[must_use]
    pub fn layout(&self, extent: (u32, u32), atlas: &FontAtlas) -> ConsoleLayout {
        let mut chosen = ConsoleStyle::pixel_art(1);
        for scale in 2..=MenuStyle::MAX_SCALE {
            let style = ConsoleStyle::pixel_art(scale);
            let candidate = self.layout_with(extent, atlas, &style);
            if candidate.log_rows() >= MINIMUM_LOG_ROWS
                && candidate.field_columns(atlas) >= MINIMUM_FIELD_COLUMNS
            {
                chosen = style;
            } else {
                break;
            }
        }
        self.layout_with(extent, atlas, &chosen)
    }

    /// Lays the panel out at a style the caller chose.
    ///
    /// Every rectangle is clamped to be non-inverted, so a framebuffer too small
    /// to hold the input row produces a panel that draws nothing rather than one
    /// whose boxes are inside out.
    #[must_use]
    pub fn layout_with(
        &self,
        extent: (u32, u32),
        atlas: &FontAtlas,
        style: &ConsoleStyle,
    ) -> ConsoleLayout {
        let screen = Vec2::new(extent.0 as f32, extent.1 as f32);
        // Whole pixels, for the reason `Menu::layout_with` rounds its origin: a
        // row of an 8x13 bitmap font starting on a half pixel is a blurred row.
        let panel = (
            Vec2::ZERO,
            Vec2::new(screen.x, (screen.y * CONSOLE_HEIGHT_FRACTION).round()),
        );
        let pad = style.padding;
        let row = style.row_height();

        let send_size = send_button(style).size(atlas);
        let field_height = send_size.y.max(row + pad.y);
        let content_left = panel.0.x + pad.x;
        let content_right = (panel.1.x - pad.x).max(content_left);
        let field_max_y = (panel.1.y - pad.y).max(panel.0.y);
        let field_min_y = (field_max_y - field_height).max(panel.0.y);

        let send_min_x = (content_right - send_size.x).max(content_left);
        let send_min = Vec2::new(
            send_min_x,
            (field_min_y + (field_height - send_size.y) * 0.5).round(),
        );
        let send = (send_min, send_min + send_size);

        let field = (
            Vec2::new(content_left, field_min_y),
            Vec2::new((send_min_x - pad.x).max(content_left), field_max_y),
        );
        let text_y = (field_min_y + (field_height - row) * 0.5).round();
        let prompt_pos = Vec2::new(field.0.x + pad.x, text_y);
        let text_pos = Vec2::new(
            prompt_pos.x + atlas.text_width(PROMPT, style.text_size / NATURAL_FONT_SIZE),
            text_y,
        );

        let log_min = Vec2::new(content_left, panel.0.y + pad.y);
        let log = (
            log_min,
            Vec2::new(content_right, (field_min_y - pad.y).max(log_min.y)),
        );

        let completion = self.completion_rows(screen, panel.1.y, text_pos.x, atlas, style);

        ConsoleLayout {
            style: *style,
            screen,
            panel,
            log,
            field,
            prompt_pos,
            text_pos,
            send,
            completion,
        }
    }

    /// Draws the panel into `dl`, in back-to-front order.
    ///
    /// `caret_visible` is the blink, which the caller owns: see
    /// [`caret_shown`](super::caret_shown). Nothing here reads a clock, so a
    /// test draws both halves of the blink without waiting for either.
    pub fn render(
        &self,
        dl: &mut DrawList,
        layout: &ConsoleLayout,
        atlas: &FontAtlas,
        caret_visible: bool,
    ) {
        let style = layout.style();
        let (panel_min, panel_max) = layout.panel();
        dl.rect(panel_min, panel_max, style.panel_color);
        // The drop-down's leading edge, so the panel reads as a thing over the
        // frame rather than as a tint on the top of it.
        dl.line(
            Vec2::new(panel_min.x, panel_max.y),
            panel_max,
            style.scale,
            style.border_color,
        );

        self.log.render(dl, layout.log(), atlas, style);

        let (field_min, field_max) = layout.field();
        dl.rect(field_min, field_max, style.field_color);
        dl.rect_outline(field_min, field_max, style.scale, style.border_color);
        dl.text(
            layout.prompt_pos(),
            PROMPT,
            style.prompt_color,
            style.text_size,
        );
        self.field.render(
            dl,
            layout.text_pos(),
            atlas,
            &style.field_style(),
            layout.field_columns(atlas),
            caret_visible,
        );

        let (send_min, send_max) = layout.send();
        send_button(style)
            .with_fixed_size(send_max - send_min)
            .render(dl, send_min, atlas, &style.button, self.send);

        self.render_completion(dl, layout, atlas);
    }

    /// The candidate rows, hanging below the panel like Source's.
    fn completion_rows(
        &self,
        screen: Vec2,
        below: f32,
        left: f32,
        atlas: &FontAtlas,
        style: &ConsoleStyle,
    ) -> Vec<(Vec2, Vec2)> {
        let row = style.row_height();
        if self.candidates.is_empty() || row <= 0.0 {
            return Vec::new();
        }
        // Only rows that fit whole between the panel and the bottom of the
        // frame: the list is drawn outside the panel, so nothing else stops one
        // from hanging off the screen.
        let fits = ((screen.y - below) / row).max(0.0) as usize;
        let count = self.candidates.len().min(COMPLETION_ROWS).min(fits);
        if count == 0 {
            return Vec::new();
        }

        let scale = style.text_size / NATURAL_FONT_SIZE;
        let widest = self.candidates[..count]
            .iter()
            .map(|name| atlas.text_width(name, scale))
            .fold(0.0f32, f32::max);
        let min_x = (left - style.padding.x).max(0.0);
        let max_x = (min_x + widest + style.padding.x * 2.0).min(screen.x.max(min_x));
        (0..count)
            .map(|index| {
                let top = below + index as f32 * row;
                (Vec2::new(min_x, top), Vec2::new(max_x, top + row))
            })
            .collect()
    }

    /// Draws the candidate rows: one background, then each name with its
    /// matched head in [`ConsoleStyle::match_color`].
    fn render_completion(&self, dl: &mut DrawList, layout: &ConsoleLayout, atlas: &FontAtlas) {
        let rows = layout.completion();
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
            return;
        };
        let style = layout.style();
        dl.rect(first.0, last.1, style.completion_color);

        let scale = style.text_size / NATURAL_FONT_SIZE;
        let matched = self.prefix.chars().count();
        for (candidate, row) in self.candidates.iter().zip(rows) {
            let at = Vec2::new(layout.text_pos().x, row.0.y);
            let head: String = candidate.chars().take(matched).collect();
            let tail: String = candidate.chars().skip(matched).collect();
            if !head.is_empty() {
                dl.text(at, head.as_str(), style.match_color, style.text_size);
            }
            if !tail.is_empty() {
                dl.text(
                    Vec2::new(at.x + atlas.text_width(&head, scale), at.y),
                    tail,
                    style.candidate_color,
                    style.text_size,
                );
            }
        }
    }
}

impl Default for ConsolePanel {
    fn default() -> Self {
        Self::new()
    }
}

/// Where every part of the panel goes, for one frame at one size.
///
/// Held rather than recomputed between [`ConsolePanel::point`] and
/// [`ConsolePanel::render`]: the pointer must be tested against the rectangles
/// the frame was actually drawn from, and a second call could differ by a
/// resize.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleLayout {
    style: ConsoleStyle,
    screen: Vec2,
    panel: (Vec2, Vec2),
    log: (Vec2, Vec2),
    field: (Vec2, Vec2),
    prompt_pos: Vec2,
    text_pos: Vec2,
    send: (Vec2, Vec2),
    completion: Vec<(Vec2, Vec2)>,
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

    /// The box the log is drawn in.
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

    /// The top-left of the typed line's em box, and so the caret's origin.
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

    /// How many rows of log this layout shows.
    #[must_use]
    pub fn log_rows(&self) -> usize {
        LogView::rows_in(self.log, &self.style)
    }

    /// How many columns of text the input line holds.
    #[must_use]
    pub fn field_columns(&self, atlas: &FontAtlas) -> usize {
        let right = (self.field.1.x - self.style.padding.x).max(self.text_pos.x);
        LogView::columns_in(
            (self.text_pos, Vec2::new(right, self.text_pos.y)),
            atlas,
            &self.style,
        )
    }
}

/// The **Send** button, at the panel's own size and spacing.
fn send_button(style: &ConsoleStyle) -> Button {
    let mut button = Button::new(SEND_LABEL).with_size(style.text_size);
    button.padding = style.padding;
    button
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use crcbl_core::log::Level;
    use crcbl_core::log::console::Record;

    use super::*;
    use crate::draw_list::DrawCommand;

    /// The five extents every "it is on screen" test in this repository uses.
    const EXTENTS: [(u32, u32); 5] = [
        (960, 720),
        (800, 600),
        (1920, 1080),
        (1440, 400),
        (600, 900),
    ];

    fn atlas() -> FontAtlas {
        FontAtlas::built_in()
    }

    /// A panel with `lines` info records and `typed` in the field.
    fn panel(lines: &[&str], typed: &str) -> ConsolePanel {
        let mut panel = ConsolePanel::new();
        let records: Vec<Record> = lines
            .iter()
            .enumerate()
            .map(|(index, message)| Record {
                sequence: index as u64,
                level: Level::Info,
                target: crcbl_core::log::console::CONSOLE_TARGET.to_owned(),
                message: (*message).to_owned(),
                elapsed: Duration::ZERO,
            })
            .collect();
        panel.log_mut().push_records(&records);
        panel.field_mut().insert(typed);
        panel
    }

    /// Every text command a frame draws, in order.
    fn texts(dl: &DrawList) -> Vec<String> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn press_at(pos: Vec2) -> PointerInput {
        PointerInput {
            pos,
            down: true,
            released: false,
        }
    }

    fn release_at(pos: Vec2) -> PointerInput {
        PointerInput {
            pos,
            down: false,
            released: true,
        }
    }

    /// **The panel is the top slice of the frame and everything is inside it**,
    /// at every aspect ratio: the log above the input row, the input row above
    /// the panel's bottom edge, and the button at the right-hand end of it.
    #[test]
    fn the_panel_is_the_top_of_the_frame_and_holds_its_parts() {
        let atlas = atlas();
        let content = panel(&["one", "two"], "help");
        for extent in EXTENTS {
            let layout = content.layout(extent, &atlas);
            let (min, max) = layout.panel();
            assert_eq!(min, Vec2::ZERO, "{extent:?}: the panel left the top-left");
            assert_eq!(
                max.x, extent.0 as f32,
                "{extent:?}: the panel is not full width"
            );
            assert!(
                (max.y - (extent.1 as f32 * CONSOLE_HEIGHT_FRACTION).round()).abs() < 1e-3,
                "{extent:?}: the panel is {} tall, not {CONSOLE_HEIGHT_FRACTION} of the frame",
                max.y,
            );

            let (log_min, log_max) = layout.log();
            let (field_min, field_max) = layout.field();
            let (send_min, send_max) = layout.send();
            for (name, (part_min, part_max)) in [
                ("the log", (log_min, log_max)),
                ("the field", (field_min, field_max)),
                ("the button", (send_min, send_max)),
            ] {
                assert!(
                    part_min.x >= min.x
                        && part_min.y >= min.y
                        && part_max.x <= max.x
                        && part_max.y <= max.y,
                    "{extent:?}: {name} at {part_min:?}..{part_max:?} escapes the panel",
                );
                assert!(
                    part_max.x >= part_min.x && part_max.y >= part_min.y,
                    "{extent:?}: {name} is inside out",
                );
            }
            assert!(
                log_max.y <= field_min.y,
                "{extent:?}: the log runs into the input row",
            );
            assert!(
                field_max.x <= send_min.x,
                "{extent:?}: the field runs into the Send button",
            );
            assert!(
                send_max.x >= max.x - layout.style().padding.x * 2.0,
                "{extent:?}: the Send button is not at the right-hand edge",
            );
        }
    }

    /// **The prompt, the typed line and the button are one row**, and the line
    /// starts after the prompt rather than under it.
    #[test]
    fn the_prompt_the_line_and_the_button_share_the_input_row() {
        let atlas = atlas();
        let content = panel(&[], "antialiasing");
        let layout = content.layout((960, 720), &atlas);
        let style = layout.style();

        let prompt_width = atlas.text_width(PROMPT, style.text_size / NATURAL_FONT_SIZE);
        assert!(
            (layout.text_pos().x - layout.prompt_pos().x - prompt_width).abs() < 1e-3,
            "the typed line does not start one prompt past the prompt",
        );
        assert_eq!(layout.text_pos().y, layout.prompt_pos().y);

        let (field_min, field_max) = layout.field();
        assert!(
            layout.prompt_pos().y >= field_min.y
                && layout.prompt_pos().y + style.row_height() <= field_max.y + 1e-3,
            "the prompt is not inside the input row",
        );
        let (send_min, send_max) = layout.send();
        assert!(
            send_min.y >= field_min.y - 1e-3 && send_max.y <= field_max.y + 1e-3,
            "the button {send_min:?}..{send_max:?} is not on the input row \
             {field_min:?}..{field_max:?}",
        );
    }

    /// **A frame draws the log, the prompt, the typed line and the button** —
    /// in that order, and with a scrim and an input box behind them.
    #[test]
    fn a_frame_draws_the_log_the_prompt_the_line_and_the_button() {
        let atlas = atlas();
        let content = panel(&["first", "second"], "help fps");
        let layout = content.layout((960, 720), &atlas);
        let mut dl = DrawList::new();
        content.render(&mut dl, &layout, &atlas, true);

        assert_eq!(
            texts(&dl),
            ["first", "second", PROMPT, "help fps", SEND_LABEL],
        );
        assert!(
            matches!(
                dl.commands().first(),
                Some(DrawCommand::Rect { min, max, color })
                    if *min == layout.panel().0
                        && *max == layout.panel().1
                        && *color == layout.style().panel_color
            ),
            "the scrim is not the first thing drawn: {:?}",
            dl.commands().first(),
        );
    }

    /// **Every log line is drawn above the input row.** Without this the log
    /// could be laid out over the field and every other test here would pass —
    /// the text would still be in the panel.
    #[test]
    fn no_log_line_is_drawn_over_the_input_row() {
        let atlas = atlas();
        let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
        let content = panel(&borrowed, "");
        for extent in EXTENTS {
            let layout = content.layout(extent, &atlas);
            let style = layout.style();
            let mut dl = DrawList::new();
            content.render(&mut dl, &layout, &atlas, false);
            let drawn = dl
                .commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::Text { pos, text, .. } if text.starts_with("line ") => Some(*pos),
                    _ => None,
                })
                .count();
            assert!(drawn > 0, "{extent:?}: the panel drew no log at all");
            for command in dl.commands() {
                let DrawCommand::Text { pos, text, .. } = command else {
                    continue;
                };
                if !text.starts_with("line ") {
                    continue;
                }
                assert!(
                    pos.y + style.row_height() <= layout.field().0.y + 1e-3,
                    "{extent:?}: {text:?} at {pos:?} is drawn over the input row",
                );
                assert!(
                    pos.y >= layout.log().0.y - 1e-3,
                    "{extent:?}: {text:?} at {pos:?} is above the panel's log box",
                );
            }
        }
    }

    /// **A typed line longer than the input box stays inside it**, text and
    /// caret both. Nothing clips a draw list, so a field that drew its whole
    /// line would put glyphs over the **Send** button and off the panel.
    #[test]
    fn a_long_line_stays_inside_the_input_box() {
        let atlas = atlas();
        let extent = (960, 720);
        let long = "log warn,crcbl_vk=trace,crcbl_render=debug,crcbl_scene=trace,crcbl_ui=trace";
        let content = panel(&[], long);
        let layout = content.layout(extent, &atlas);
        let style = layout.style();
        assert!(
            long.chars().count() > layout.field_columns(&atlas),
            "the line is not longer than the box, so this proves nothing",
        );

        let mut dl = DrawList::new();
        content.render(&mut dl, &layout, &atlas, true);
        let right_edge = layout.field().1.x;
        let scale = style.text_size / NATURAL_FONT_SIZE;
        let mut drew_some_of_it = false;
        for command in dl.commands() {
            let (left, right) = match command {
                DrawCommand::Text { pos, text, .. } if long.contains(text.as_str()) => {
                    drew_some_of_it = true;
                    (pos.x, pos.x + atlas.text_width(text, scale))
                }
                DrawCommand::Rect { min, max, color } if *color == style.caret_color => {
                    (min.x, max.x)
                }
                _ => continue,
            };
            assert!(
                left >= layout.field().0.x - 1e-3 && right <= right_edge + 1e-3,
                "{left}..{right} escapes the input box {:?}..{right_edge}",
                layout.field().0.x,
            );
        }
        assert!(drew_some_of_it, "none of the typed line was drawn");
    }

    /// **`Enter` and the Send button submit the same line**, and both leave the
    /// field empty — the decision-6 requirement that the button is not a second
    /// path with its own behaviour.
    #[test]
    fn enter_and_the_send_button_submit_the_same_line() {
        let atlas = atlas();
        let extent = (960, 720);

        let mut typed = panel(&[], "antialiasing smaa");
        assert_eq!(typed.submit().as_deref(), Some("antialiasing smaa"));
        assert!(typed.field().is_empty(), "the field kept the sent line");
        assert_eq!(typed.submit(), None, "an empty field submitted a command");

        let mut clicked = panel(&[], "antialiasing smaa");
        let layout = clicked.layout(extent, &atlas);
        let (min, max) = layout.send();
        let on_button = (min + max) * 0.5;
        let mut ui = UiState::new();
        assert_eq!(clicked.point(&layout, &mut ui, press_at(on_button)), None);
        assert_eq!(
            clicked.send_state(),
            ButtonState::Pressed,
            "the press did not reach the button's art",
        );
        assert_eq!(
            clicked
                .point(&layout, &mut ui, release_at(on_button))
                .as_deref(),
            Some("antialiasing smaa"),
        );
        assert!(clicked.field().is_empty());
    }

    /// **Submitting a line puts the log back at its bottom**, whichever way it
    /// was sent — the answer lands there, and a reader who had scrolled back
    /// would otherwise be looking at old lines while it arrives.
    ///
    /// Neither `Enter` nor a click reads the scroll, so nothing else here would
    /// notice `submit` forgetting it: the panel would still send the line and
    /// still clear the field.
    #[test]
    fn a_submitted_line_returns_the_log_to_its_newest_lines() {
        let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut panel = panel(&lines, "help");
        panel.log_mut().scroll_by(10);
        assert_eq!(panel.log().scroll(), 10, "the view did not scroll back");

        assert_eq!(panel.submit().as_deref(), Some("help"));
        assert_eq!(
            panel.log().scroll(),
            0,
            "the log stayed scrolled back after a line was sent",
        );

        // And a blank submission is still a submission for this purpose: the
        // person pressed Enter to get back to the bottom, which a terminal also
        // does.
        panel.log_mut().scroll_by(10);
        assert_eq!(panel.submit(), None);
        assert_eq!(panel.log().scroll(), 0);
    }

    /// **A press that starts on the button and is released off it sends
    /// nothing**, and a press that never touches it sends nothing either.
    #[test]
    fn a_press_that_leaves_the_button_sends_nothing() {
        let atlas = atlas();
        let mut content = panel(&[], "quit");
        let layout = content.layout((960, 720), &atlas);
        let on_button = (layout.send().0 + layout.send().1) * 0.5;
        let elsewhere = layout.text_pos();
        let mut ui = UiState::new();

        content.point(&layout, &mut ui, press_at(on_button));
        assert_eq!(content.point(&layout, &mut ui, release_at(elsewhere)), None);
        assert_eq!(content.field().text(), "quit", "the line was sent anyway");

        content.point(&layout, &mut ui, press_at(elsewhere));
        assert_eq!(content.point(&layout, &mut ui, release_at(elsewhere)), None);
        assert_eq!(content.field().text(), "quit");
    }

    /// **The completion rows hang under the field with the matched head
    /// highlighted**, capped at [`COMPLETION_ROWS`], and lined up with the
    /// typed token rather than with the panel's edge.
    #[test]
    fn the_completion_rows_highlight_the_matched_head() {
        let atlas = atlas();
        let mut content = panel(&[], "r_a");
        content.set_completion("r_a", &["r_ao_view", "r_ambient"]);
        let layout = content.layout((960, 720), &atlas);
        let style = *layout.style();
        assert_eq!(layout.completion().len(), 2);

        let mut dl = DrawList::new();
        content.render(&mut dl, &layout, &atlas, false);
        let rows: Vec<(Vec2, String, [f32; 4])> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text {
                    pos, text, color, ..
                } => Some((*pos, text.clone(), *color)),
                _ => None,
            })
            .collect();
        // The prompt, the typed line and the button come first; what follows is
        // two candidates, each split into a matched head and a tail.
        let tail = &rows[rows.len() - 4..];
        assert_eq!(
            tail.iter()
                .map(|(_, text, color)| (text.as_str(), *color))
                .collect::<Vec<_>>(),
            [
                ("r_a", style.match_color),
                ("o_view", style.candidate_color),
                ("r_a", style.match_color),
                ("mbient", style.candidate_color),
            ],
        );
        assert!(
            (tail[1].0.x
                - tail[0].0.x
                - atlas.text_width("r_a", style.text_size / NATURAL_FONT_SIZE))
            .abs()
                < 1e-3,
            "the tail is not drawn one matched head to the right of it",
        );
        assert_eq!(
            tail[0].0.x,
            layout.text_pos().x,
            "the candidates do not line up with the typed token",
        );
        assert!(
            tail[2].0.y - tail[0].0.y >= style.row_height() - 1e-3,
            "the two candidates are on the same row",
        );
        for row in layout.completion() {
            assert!(
                row.0.y >= layout.panel().1.y - 1e-3,
                "a candidate row is drawn inside the panel, over the input",
            );
            assert!(
                row.1.y <= layout.screen().y + 1e-3,
                "a candidate row hangs off the bottom of the frame",
            );
        }
    }

    /// The list is capped, and it is capped by the frame as well as by
    /// [`COMPLETION_ROWS`] — a row that would hang off the bottom is not drawn,
    /// because nothing clips it.
    #[test]
    fn the_completion_list_is_capped_by_the_rows_and_by_the_frame() {
        let atlas = atlas();
        let names: Vec<String> = (0..COMPLETION_ROWS + 4)
            .map(|i| format!("cmd_{i}"))
            .collect();
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut content = panel(&[], "cmd_");
        content.set_completion("cmd_", &borrowed);

        let roomy = content.layout((960, 720), &atlas);
        assert_eq!(roomy.completion().len(), COMPLETION_ROWS);

        // A frame with almost nothing below the panel fits fewer rows than the
        // cap, and every one of them still ends inside the frame.
        let cramped = content.layout((960, 80), &atlas);
        assert!(
            cramped.completion().len() < COMPLETION_ROWS,
            "an 80-pixel frame offered {} rows below the panel",
            cramped.completion().len(),
        );
        for row in cramped.completion() {
            assert!(row.1.y <= cramped.screen().y + 1e-3);
        }

        content.clear_completion();
        assert!(
            content.layout((960, 720), &atlas).completion().is_empty(),
            "the candidates outlived the completion",
        );
    }

    /// **A bigger window gets a bigger console**, and every window gets one that
    /// shows the log rows and the input columns the floors ask for.
    #[test]
    fn a_bigger_window_gets_a_bigger_console() {
        let atlas = atlas();
        let content = panel(&["one"], "");
        let small = content.layout((640, 480), &atlas);
        let large = content.layout((3840, 2160), &atlas);
        assert!(
            large.style().scale > small.style().scale,
            "640x480 chose {} and 3840x2160 chose {}",
            small.style().scale,
            large.style().scale,
        );
        assert!(large.style().scale <= MenuStyle::MAX_SCALE as f32);

        for extent in EXTENTS {
            let layout = content.layout(extent, &atlas);
            assert!(
                layout.log_rows() >= MINIMUM_LOG_ROWS,
                "{extent:?}: {} log rows at scale {}",
                layout.log_rows(),
                layout.style().scale,
            );
            assert!(
                layout.field_columns(&atlas) >= MINIMUM_FIELD_COLUMNS,
                "{extent:?}: {} input columns at scale {}",
                layout.field_columns(&atlas),
                layout.style().scale,
            );
        }
    }

    /// A framebuffer too small for the console lays out without inverting a
    /// rectangle and draws no text over the frame.
    #[test]
    fn a_frame_with_no_room_lays_out_without_inverting_anything() {
        let atlas = atlas();
        let content = panel(&["one", "two"], "help");
        for extent in [(0, 0), (1, 1), (32, 24)] {
            let layout = content.layout(extent, &atlas);
            for (name, (min, max)) in [
                ("the panel", layout.panel()),
                ("the log", layout.log()),
                ("the field", layout.field()),
            ] {
                assert!(
                    max.x >= min.x && max.y >= min.y,
                    "{extent:?}: {name} is inside out at {min:?}..{max:?}",
                );
            }
            assert_eq!(layout.log_rows(), 0, "{extent:?}: a log row fitted");
            let mut dl = DrawList::new();
            content.render(&mut dl, &layout, &atlas, true);
            for command in dl.commands() {
                if let DrawCommand::Text { text, .. } = command {
                    assert_ne!(text, "one", "{extent:?}: a log line was drawn anyway");
                }
            }
        }
    }
}
