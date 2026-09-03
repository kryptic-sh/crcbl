//! The console's view of the log: the lines it holds, where it is scrolled to,
//! and how they are drawn.
//!
//! `docs/plan/52-debug-console.md` decision 6. The view is fed [`Record`]s and
//! nothing else — it does not read the ring, which is why a test here can hold
//! a log with no logger installed.
//!
//! # What it draws is narrower than what it holds
//!
//! The view takes every record the ring gives it and **draws two kinds**: the
//! console's own output, whatever level it carries, and engine records at
//! [`LevelFilter::Warn`] or worse. The engine's info-level commentary is held
//! and not drawn, so the panel reads as a command prompt with its answers
//! rather than as a second terminal. [`LogView::set_filter`] widens it, and
//! nothing is lost in the meantime because the lines are already here.

use std::collections::VecDeque;

use glam::Vec2;

use crcbl_core::log::console::{CONSOLE_RING_LINES, CONSOLE_TARGET, Record};
use crcbl_core::log::{Level, LevelFilter};

use crate::draw_list::DrawList;
use crate::text::FontAtlas;

use super::ConsoleStyle;

/// One line the view holds: the text to draw and the level that colours it.
///
/// The level rather than the level's *name*: the panel draws no `INFO` column,
/// because a console is read at a glance and a colour is read faster than a
/// word — and because the five names would cost six columns of a panel that
/// wraps at the window's width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// The level the record was logged at.
    pub level: Level,
    /// The text, as [`LogView::push_records`] rendered it.
    pub text: String,
    /// Whether the console itself produced this line ([`CONSOLE_TARGET`]) — an
    /// echoed command, the answer to it, `help`.
    ///
    /// **Kept because the level cannot say it.** Command output is logged at
    /// [`Level::Info`], the same level as the engine's own running commentary,
    /// so a view that hid info to be quiet would hide the answer to what
    /// somebody just typed. This is what [`LogView::render`] tests instead, and
    /// it is why a filter of [`LevelFilter::Warn`] still shows a typed
    /// command's reply.
    pub from_console: bool,
}

/// The lines the console shows, newest at the bottom.
///
/// Bounded at [`CONSOLE_RING_LINES`], the same figure the ring itself holds, so
/// the panel scrolls back exactly as far as there is log to scroll back
/// through: a shallower view would hide lines the ring still has and a deeper
/// one would keep lines nothing can refill after a `clear`.
#[derive(Debug, Clone)]
pub struct LogView {
    /// Oldest first.
    lines: VecDeque<LogLine>,
    /// The sequence to ask the ring for next — see [`LogView::cursor`].
    cursor: u64,
    /// How many lines are hidden **below** the bottom of the view. Zero is the
    /// newest line at the bottom, which is where a console sits.
    scroll: usize,
    /// The most verbose level the view draws for lines the **engine** logged.
    /// The console's own output ignores it — see [`LogLine::from_console`].
    filter: LevelFilter,
}

impl LogView {
    /// An empty view, scrolled to the newest line, showing every level.
    ///
    /// [`LevelFilter::Trace`] rather than the logger's own filter: the ring
    /// holds records the terminal's filter refused, and the panel's filter is
    /// the one that decides whether they are drawn. Narrowing it hides lines;
    /// it never prints any.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            cursor: 0,
            scroll: 0,
            filter: LevelFilter::Warn,
        }
    }

    /// Appends every record, oldest first, and moves the cursor past them.
    ///
    /// The rendered text is the record's message, with its target in front of
    /// it unless the console itself printed the line
    /// ([`CONSOLE_TARGET`]) — an echoed command and the answer to it read as
    /// what was typed, the way Source's console does, and everything else says
    /// which module said it. The elapsed seconds stay the terminal's: the panel
    /// is at most a hundred-odd columns wide and the ordering is the ring's
    /// sequence, so a timestamp per line would cost a tenth of the width to
    /// repeat what the order already says.
    pub fn push_records(&mut self, records: &[Record]) {
        for record in records {
            self.cursor = self.cursor.max(record.sequence.saturating_add(1));
            self.push(LogLine {
                level: record.level,
                text: line_text(record),
                from_console: record.target == CONSOLE_TARGET,
            });
        }
    }

    /// Appends one line, dropping the oldest when the view is full.
    fn push(&mut self, line: LogLine) {
        if self.lines.len() >= CONSOLE_RING_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
        if self.scroll > 0 {
            // Scrolled back, so the reader is looking at something: counted from
            // the end, everything it can see moved one line further from the
            // end when this one arrived. Following it keeps the text still
            // under a log that is filling up.
            self.scroll = (self.scroll + 1).min(self.lines.len().saturating_sub(1));
        }
    }

    /// The sequence to pass [`snapshot_since`] on the next frame.
    ///
    /// One past the newest record this view has been given, so a caller that
    /// keeps a view across frames copies the lines that arrived since it last
    /// looked rather than the whole ring.
    ///
    /// [`snapshot_since`]: crcbl_core::log::console::snapshot_since
    #[must_use]
    pub const fn cursor(&self) -> u64 {
        self.cursor
    }

    /// The lines the view holds, oldest first and before the level filter — so
    /// this yields the engine's info lines that [`render`](Self::render) hides.
    pub fn lines(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// How many lines the view holds, before the level filter.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the view holds no lines at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Empties the view — what `clear` does.
    ///
    /// **The cursor is kept**, so the lines already taken from the ring are not
    /// taken again on the next frame: `clear` empties the console, not the log,
    /// which is why there is no `clear_ring` for it to call.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
    }

    /// How many lines are hidden below the bottom of the view.
    #[must_use]
    pub const fn scroll(&self) -> usize {
        self.scroll
    }

    /// Scrolls by `lines`, positive going back through the log.
    ///
    /// Clamped so the oldest line the view holds is the furthest it goes, and
    /// so the newest is always the near end — a wheel spun past either end
    /// stops rather than winding up a number that has to be spun back.
    pub fn scroll_by(&mut self, lines: i32) {
        let furthest = self.lines.len().saturating_sub(1) as i64;
        let at = i64::from(lines).saturating_add(self.scroll as i64);
        self.scroll = at.clamp(0, furthest) as usize;
    }

    /// Scrolls back to the newest line.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
    }

    /// The most verbose level the view draws for engine lines.
    ///
    /// The console's own output is drawn whatever this says — see
    /// [`LogLine::from_console`] for why the level alone cannot decide it.
    #[must_use]
    pub const fn filter(&self) -> LevelFilter {
        self.filter
    }

    /// Sets the most verbose level the view draws for engine lines, and returns
    /// to the newest line.
    ///
    /// **[`LevelFilter::Warn`] is what a view starts at**: the panel is where
    /// somebody types a command and reads its answer, and an engine that logs
    /// its running commentary at info scrolls that answer off the screen before
    /// it can be read. So the default shows what a person asked for and what
    /// went wrong, and nothing else. Widening this to
    /// [`Info`](LevelFilter::Info) brings the commentary back — the lines were
    /// held the whole time, as the paragraph below says.
    ///
    /// The panel's own filter, and **not** the logger's: this hides lines that
    /// were logged, where `log <filter>` decides what is logged at all. Turning
    /// this one down and then back up shows the lines that were there the whole
    /// time; turning the logger's down loses them.
    pub fn set_filter(&mut self, filter: LevelFilter) {
        self.filter = filter;
        self.scroll = 0;
    }

    /// How many rows of text fit in `rect`.
    #[must_use]
    pub fn rows_in(rect: (Vec2, Vec2), style: &ConsoleStyle) -> usize {
        let height = rect.1.y - rect.0.y;
        let row = style.row_height();
        if row <= 0.0 || height < row {
            return 0;
        }
        (height / row) as usize
    }

    /// How many columns of text fit in `rect`.
    #[must_use]
    pub fn columns_in(rect: (Vec2, Vec2), atlas: &FontAtlas, style: &ConsoleStyle) -> usize {
        let width = rect.1.x - rect.0.x;
        let advance = style.advance(atlas);
        if advance <= 0.0 || width < advance {
            return 0;
        }
        (width / advance) as usize
    }

    /// Draws the lines that fit in `rect`, newest at its bottom edge.
    ///
    /// Two things the caller does not have to do, because [`DrawList`] has no
    /// clip rectangle and cannot be given one:
    ///
    /// * **Whole rows outside the rectangle are dropped**, not clipped. A row
    ///   that would only half fit at the top is not drawn at all, so nothing
    ///   ever crosses the panel's edge into the frame behind it.
    /// * **A line longer than the rectangle wraps** at the column count, which
    ///   the monospace atlas makes exact, and its continuation rows follow it in
    ///   order.
    pub fn render(
        &self,
        dl: &mut DrawList,
        rect: (Vec2, Vec2),
        atlas: &FontAtlas,
        style: &ConsoleStyle,
    ) {
        let rows_fit = Self::rows_in(rect, style);
        let columns = Self::columns_in(rect, atlas, style);
        if rows_fit == 0 || columns == 0 {
            return;
        }

        let visible: Vec<&LogLine> = self
            .lines
            .iter()
            .filter(|line| line.from_console || line.level.to_level_filter() <= self.filter)
            .collect();
        let scroll = self.scroll.min(visible.len().saturating_sub(1));
        let end = visible.len() - scroll;

        // Newest first while the rows are gathered, so the wrap of a line only
        // half on screen costs the rows it actually shows, then reversed into
        // reading order.
        let mut rows: Vec<(Level, String)> = Vec::with_capacity(rows_fit);
        'gather: for line in visible[..end].iter().rev() {
            for chunk in wrap(&line.text, columns).into_iter().rev() {
                if rows.len() == rows_fit {
                    break 'gather;
                }
                rows.push((line.level, chunk));
            }
        }
        rows.reverse();

        let row_height = style.row_height();
        let top = rect.1.y - rows.len() as f32 * row_height;
        for (index, (level, text)) in rows.iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            dl.text(
                Vec2::new(rect.0.x, top + index as f32 * row_height),
                text.as_str(),
                style.level_color(*level),
                style.text_size,
            );
        }
    }
}

impl Default for LogView {
    fn default() -> Self {
        Self::new()
    }
}

/// The text one record is drawn as.
fn line_text(record: &Record) -> String {
    if record.target == CONSOLE_TARGET {
        record.message.clone()
    } else {
        format!("[{}] {}", record.target, record.message)
    }
}

/// `text` cut into runs of at most `columns` characters, split at its newlines
/// first.
///
/// Always at least one row, so a blank log line takes a blank row rather than
/// disappearing — a run of them is how a program's output is paragraphed and
/// swallowing them re-flows somebody's careful spacing.
fn wrap(text: &str, columns: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut count = 0;
        for c in line.chars() {
            row.push(c);
            count += 1;
            if count == columns {
                rows.push(std::mem::take(&mut row));
                count = 0;
            }
        }
        if !row.is_empty() || count == 0 {
            rows.push(row);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use super::*;
    use crate::draw_list::DrawCommand;

    fn atlas() -> FontAtlas {
        FontAtlas::built_in()
    }

    fn style() -> ConsoleStyle {
        ConsoleStyle::pixel_art(1)
    }

    /// One record, as the ring would hand it over.
    fn record(sequence: u64, level: Level, target: &str, message: &str) -> Record {
        Record {
            sequence,
            level,
            target: target.to_owned(),
            message: message.to_owned(),
            elapsed: Duration::from_millis(sequence),
        }
    }

    /// `count` info records from `first`, numbered in their message.
    fn records(first: u64, count: u64) -> Vec<Record> {
        (first..first + count)
            .map(|i| record(i, Level::Info, "crcbl::demo", &format!("line {i}")))
            .collect()
    }

    /// A rectangle `rows` rows tall and `columns` columns wide at `style`.
    fn rect_of(
        rows: usize,
        columns: usize,
        atlas: &FontAtlas,
        style: &ConsoleStyle,
    ) -> (Vec2, Vec2) {
        let min = Vec2::new(10.0, 20.0);
        (
            min,
            min + Vec2::new(
                columns as f32 * style.advance(atlas),
                rows as f32 * style.row_height(),
            ),
        )
    }

    /// The text commands a view draws into `rect`, in the order they were
    /// emitted — top row first.
    fn drawn(view: &LogView, rect: (Vec2, Vec2), style: &ConsoleStyle) -> Vec<String> {
        let mut dl = DrawList::new();
        view.render(&mut dl, rect, &atlas(), style);
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The cursor is one past the newest record taken**, which is what makes
    /// a per-frame `snapshot_since` copy the new lines and not the whole ring.
    #[test]
    fn the_cursor_follows_the_records_that_were_taken() {
        let mut view = LogView::new();
        assert_eq!(view.cursor(), 0, "an empty view asks for everything");
        view.push_records(&records(0, 3));
        assert_eq!(view.cursor(), 3);
        view.push_records(&records(3, 2));
        assert_eq!(view.cursor(), 5);
        view.push_records(&[]);
        assert_eq!(view.cursor(), 5, "an empty snapshot moved the cursor");

        // And `clear` empties the view without asking the ring for the lines it
        // just dropped — the ring is the run's, not the panel's.
        view.clear();
        assert!(view.is_empty());
        assert_eq!(view.cursor(), 5);
    }

    /// **A console line reads as what was typed; every other line names the
    /// module that said it.**
    #[test]
    fn a_console_line_carries_no_target_and_every_other_line_does() {
        let mut view = LogView::new();
        view.push_records(&[
            record(0, Level::Info, CONSOLE_TARGET, "] antialiasing"),
            record(1, Level::Warn, "crcbl_vk::device", "no device"),
        ]);
        let lines: Vec<&LogLine> = view.lines().collect();
        assert_eq!(lines[0].text, "] antialiasing");
        assert_eq!(lines[0].level, Level::Info);
        assert_eq!(lines[1].text, "[crcbl_vk::device] no device");
        assert_eq!(lines[1].level, Level::Warn);
    }

    /// **The view is bounded at the ring's own depth**, so a run that logs
    /// without limit costs the panel a fixed amount of memory and the oldest
    /// lines go first.
    #[test]
    fn the_view_holds_no_more_lines_than_the_ring_does() {
        let mut view = LogView::new();
        view.push_records(&records(0, CONSOLE_RING_LINES as u64 + 10));
        assert_eq!(view.len(), CONSOLE_RING_LINES);
        let oldest = view.lines().next().expect("a line");
        assert_eq!(oldest.text, "[crcbl::demo] line 10");
    }

    /// **The newest line is at the bottom edge and stays there** as lines
    /// arrive — the property that makes a console readable while a run logs.
    #[test]
    fn the_newest_line_sits_on_the_bottom_edge_whatever_else_arrives() {
        let atlas = atlas();
        let style = style();
        let rect = rect_of(4, 40, &atlas, &style);
        let mut view = LogView::new();
        // Layout, not filtering: these are engine info lines, which the
        // quiet default hides — see `set_filter`.
        view.set_filter(LevelFilter::Trace);
        let mut bottoms = Vec::new();
        for step in 0..6 {
            view.push_records(&records(step, 1));
            let mut dl = DrawList::new();
            view.render(&mut dl, rect, &atlas, &style);
            let last = dl
                .commands()
                .iter()
                .filter_map(|command| match command {
                    DrawCommand::Text { pos, text, .. } => Some((*pos, text.clone())),
                    _ => None,
                })
                .next_back()
                .expect("a line is drawn");
            assert_eq!(last.1, format!("[crcbl::demo] line {step}"));
            bottoms.push(last.0.y);
        }
        for (index, y) in bottoms.iter().enumerate() {
            assert!(
                (y + style.row_height() - rect.1.y).abs() < 1e-3,
                "after {index} lines the newest sat at {y}, not on the bottom edge \
                 {}",
                rect.1.y,
            );
        }
    }

    /// **A row that would not fit whole is not drawn at all**, because the draw
    /// list has no clip rectangle: the alternative is glyphs crossing the
    /// panel's edge into the frame behind it.
    #[test]
    fn only_whole_rows_that_fit_are_drawn() {
        let atlas = atlas();
        let style = style();
        let mut view = LogView::new();
        // Layout, not filtering: these are engine info lines, which the
        // quiet default hides — see `set_filter`.
        view.set_filter(LevelFilter::Trace);
        view.push_records(&records(0, 10));

        let three = rect_of(3, 40, &atlas, &style);
        assert_eq!(
            drawn(&view, three, &style),
            [
                "[crcbl::demo] line 7",
                "[crcbl::demo] line 8",
                "[crcbl::demo] line 9",
            ],
        );

        // Half a row taller shows the same three, not three and a half.
        let ragged = (three.0, three.1 + Vec2::new(0.0, style.row_height() * 0.5));
        assert_eq!(drawn(&view, ragged, &style).len(), 3);
        // And a rectangle with no room at all draws nothing rather than one row
        // hanging off the top of it.
        let none = (
            three.0,
            three.0 + Vec2::new(400.0, style.row_height() * 0.9),
        );
        assert!(drawn(&view, none, &style).is_empty());
    }

    /// **A line longer than the panel wraps at the column count**, in order,
    /// and the wrap is exact because the atlas is monospace.
    #[test]
    fn a_long_line_wraps_at_the_column_count() {
        let atlas = atlas();
        let style = style();
        let columns = 12;
        let rect = rect_of(4, columns, &atlas, &style);
        let mut view = LogView::new();
        view.push_records(&[record(
            0,
            Level::Info,
            CONSOLE_TARGET,
            "abcdefghijklmnopqrstuvwxyz",
        )]);

        let rows = drawn(&view, rect, &style);
        assert_eq!(rows, ["abcdefghijkl", "mnopqrstuvwx", "yz"]);
        for row in &rows {
            assert!(
                atlas.text_width(row, 1.0) <= rect.1.x - rect.0.x + 1e-3,
                "{row:?} is wider than the rectangle it was wrapped for",
            );
        }

        // A narrower rectangle wraps the same line into more rows, so the wrap
        // is the panel's width and not a fixed number baked in anywhere.
        let narrow = rect_of(8, 6, &atlas, &style);
        assert_eq!(
            drawn(&view, narrow, &style),
            ["abcdef", "ghijkl", "mnopqr", "stuvwx", "yz"],
        );
    }

    /// A record's own newlines break rows, and a blank one keeps its row.
    #[test]
    fn a_records_newlines_are_rows_and_a_blank_row_is_kept() {
        let atlas = atlas();
        let style = style();
        let rect = rect_of(6, 40, &atlas, &style);
        let mut view = LogView::new();
        view.push_records(&[record(0, Level::Info, CONSOLE_TARGET, "one\n\ntwo")]);

        // The blank row emits no text command of its own, so what is asserted
        // is where the two that do land: a row apart, not touching.
        let mut dl = DrawList::new();
        view.render(&mut dl, rect, &atlas, &style);
        let placed: Vec<(Vec2, String)> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { pos, text, .. } => Some((*pos, text.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].1, "one");
        assert_eq!(placed[1].1, "two");
        assert!(
            (placed[1].0.y - placed[0].0.y - style.row_height() * 2.0).abs() < 1e-3,
            "the blank row between them was swallowed",
        );
    }

    /// **Scrolling back walks the log**, stops at the oldest line, and the text
    /// on screen does not move when a new line arrives behind it.
    #[test]
    fn scrolling_back_holds_still_while_the_log_fills_up() {
        let atlas = atlas();
        let style = style();
        let rect = rect_of(3, 40, &atlas, &style);
        let mut view = LogView::new();
        // Layout, not filtering: these are engine info lines, which the
        // quiet default hides — see `set_filter`.
        view.set_filter(LevelFilter::Trace);
        view.push_records(&records(0, 10));

        view.scroll_by(4);
        assert_eq!(view.scroll(), 4);
        let held = drawn(&view, rect, &style);
        assert_eq!(
            held,
            [
                "[crcbl::demo] line 3",
                "[crcbl::demo] line 4",
                "[crcbl::demo] line 5",
            ],
        );

        view.push_records(&records(10, 3));
        assert_eq!(
            drawn(&view, rect, &style),
            held,
            "three lines arriving dragged the reader's place with them",
        );

        // The ends hold: back past the oldest line and forward past the newest
        // both stop rather than winding up a number.
        view.scroll_by(1_000);
        assert_eq!(view.scroll(), view.len() - 1);
        assert_eq!(
            drawn(&view, rect, &style)[0],
            "[crcbl::demo] line 0",
            "scrolling to the end did not land on the oldest line",
        );
        view.scroll_by(-1_000);
        assert_eq!(view.scroll(), 0);
        assert_eq!(
            drawn(&view, rect, &style).last().expect("a line"),
            "[crcbl::demo] line 12",
        );
    }

    /// **The panel's own filter hides lines without losing them**, which is the
    /// whole difference between it and `log <filter>`.
    #[test]
    fn the_level_filter_hides_engine_lines_and_gives_them_back() {
        let atlas = atlas();
        let style = style();
        let rect = rect_of(6, 40, &atlas, &style);
        let mut view = LogView::new();
        view.push_records(&[
            record(0, Level::Error, "crcbl_vk::device", "boom"),
            record(1, Level::Info, "crcbl::demo", "started"),
            record(2, Level::Debug, "crcbl::demo", "detail"),
        ]);
        let all = [
            "[crcbl_vk::device] boom",
            "[crcbl::demo] started",
            "[crcbl::demo] detail",
        ];

        // The default is quiet: the failure is drawn and the commentary is not.
        assert_eq!(view.filter(), LevelFilter::Warn);
        assert_eq!(drawn(&view, rect, &style), ["[crcbl_vk::device] boom"]);
        assert_eq!(view.len(), 3, "the filter dropped lines instead of hiding");

        view.set_filter(LevelFilter::Trace);
        assert_eq!(drawn(&view, rect, &style), all);

        view.set_filter(LevelFilter::Off);
        assert!(drawn(&view, rect, &style).is_empty());

        view.set_filter(LevelFilter::Trace);
        assert_eq!(drawn(&view, rect, &style), all);
    }

    /// **The answer to a typed command survives the quiet default**, which is
    /// the whole reason a line remembers who produced it.
    ///
    /// `crcbl_core::log::console::print` logs at [`Level::Info`] — the same
    /// level as the engine's running commentary — so a view that decided on the
    /// level alone would hide what somebody just asked for. The engine's info
    /// line beside it is the control: same level, same view, not drawn.
    #[test]
    fn a_typed_commands_answer_is_drawn_where_the_engines_chatter_is_not() {
        let atlas = atlas();
        let style = style();
        let rect = rect_of(6, 40, &atlas, &style);
        let mut view = LogView::new();
        view.push_records(&[
            record(0, Level::Info, CONSOLE_TARGET, "] r_ssao_intensity"),
            record(1, Level::Info, CONSOLE_TARGET, "r_ssao_intensity 1"),
            record(2, Level::Info, "crcbl::engine", "shell: first configure"),
        ]);

        assert_eq!(view.filter(), LevelFilter::Warn);
        assert_eq!(
            drawn(&view, rect, &style),
            ["] r_ssao_intensity", "r_ssao_intensity 1"],
            "the console's own output is drawn whatever the filter says, and the \
             engine's info line at the same level is not"
        );

        // Off is off for the engine's lines and still not for the console's:
        // a prompt that answered nothing would be a broken prompt.
        view.set_filter(LevelFilter::Off);
        assert_eq!(
            drawn(&view, rect, &style),
            ["] r_ssao_intensity", "r_ssao_intensity 1"]
        );
    }

    /// Every line is drawn in its own level's colour, so the level is readable
    /// with no level name on screen.
    #[test]
    fn each_line_is_drawn_in_its_levels_colour() {
        let atlas = atlas();
        let style = style();
        let rect = rect_of(6, 40, &atlas, &style);
        let mut view = LogView::new();
        for (index, level) in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ]
        .into_iter()
        .enumerate()
        {
            view.push_records(&[record(index as u64, level, CONSOLE_TARGET, "line")]);
        }

        let mut dl = DrawList::new();
        view.render(&mut dl, rect, &atlas, &style);
        let colors: Vec<[f32; 4]> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(
            colors,
            [
                style.level_color(Level::Error),
                style.level_color(Level::Warn),
                style.level_color(Level::Info),
                style.level_color(Level::Debug),
                style.level_color(Level::Trace),
            ],
        );
    }

    /// A degenerate rectangle draws nothing rather than dividing by a zero row
    /// height or wrapping into columns of no width.
    #[test]
    fn a_rectangle_with_no_room_in_it_draws_nothing() {
        let atlas = atlas();
        let style = style();
        let mut view = LogView::new();
        view.push_records(&records(0, 4));
        for rect in [
            (Vec2::ZERO, Vec2::ZERO),
            (Vec2::new(10.0, 10.0), Vec2::new(9.0, 9.0)),
            (Vec2::ZERO, Vec2::new(1000.0, 0.0)),
            (Vec2::ZERO, Vec2::new(0.0, 1000.0)),
        ] {
            assert!(
                LogView::rows_in(rect, &style) == 0
                    || LogView::columns_in(rect, &atlas, &style) == 0,
                "{rect:?} reported room it does not have",
            );
            assert!(
                drawn(&view, rect, &style).is_empty(),
                "{rect:?} drew a line it had no room for",
            );
        }
    }
}
