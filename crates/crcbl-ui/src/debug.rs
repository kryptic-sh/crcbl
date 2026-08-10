//! The modular debug overlay: one panel, assembled from sections that the
//! systems being reported on contribute.
//!
//! `docs/plan/ROADMAP.md`'s standing requirement 1 and
//! `docs/plan/sample/00-samples-overview.md` rule 4 both say the same thing:
//! **one** panel that every sample switches on, where frame timing and FPS are
//! always present and every other module appears because the system it reports
//! on is present, not because the sample asked for it. That is what this module
//! is, and `docs/plan/07-ui-debug.md`'s "Debug tools" section is the list of
//! modules that will land on top of it.
//!
//! # The panel does not know what a system is
//!
//! [`DebugPanel`] holds [`DebugSection`]s, and a section is a title and a list
//! of `label: value` rows. Nothing here names a renderer, a network client or a
//! clock. A system contributes by implementing [`DebugModule`] — one method,
//! which fills a section it is handed — and the frame that wants the panel calls
//! [`DebugPanel::add`] once per system it actually has:
//!
//! ```text
//! overlay.record(frame_clock.render_dt());   // always, even while hidden
//! overlay.begin_frame();                     // clears, adds the frame section
//! if let Some(timings) = gpu.timings() { overlay.add(timings); }
//! overlay.render(&mut draw_list, screen, &atlas);
//! ```
//!
//! A sample with no connection never calls `add` with a network module and gets
//! a panel without one. That is the whole of the modularity claim, and breakout
//! and flappy — both `InMemoryTransport` — are the check on it: a panel that
//! could not render without a network module would be broken, and only a
//! connectionless sample proves it can.
//!
//! # It does nothing while it is off
//!
//! [`DebugPanel::add`] returns immediately when the panel is hidden, and
//! [`DebugPanel::render`] emits no commands, so a system's `debug_section` is
//! never called and no string is ever built. The only work a hidden overlay does
//! is [`FrameStats::record`], which is a push and two `Duration` adds — the
//! window has to stay warm or toggling the panel on would show two seconds of
//! nothing.
//!
//! Visible, the cost is one [`DrawList`] text command per row plus one
//! background rect. Section and row strings are reused across frames; the draw
//! commands are not, because [`DrawList::text`] takes an owned `String`.

use core::fmt;
use core::fmt::Write as _;
use core::time::Duration;
use std::collections::VecDeque;

use glam::Vec2;

use crate::draw_list::DrawList;
use crate::hud::Anchor;
use crate::text::{FontAtlas, LINE_HEIGHT};
use crate::widget::NATURAL_FONT_SIZE;

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

/// One `label: value` line in a [`DebugSection`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebugRow {
    /// What the number is.
    pub label: String,
    /// The number, already formatted — the panel does no formatting of its own.
    pub value: String,
}

/// One system's contribution to the panel: a title and its rows.
///
/// Rows are kept in a `Vec` that is reused rather than reallocated:
/// [`DebugSection::clear`] resets the length without dropping the strings, so a
/// module that writes the same rows every frame allocates on the first frame
/// only.
#[derive(Debug, Clone, Default)]
pub struct DebugSection {
    title: String,
    rows: Vec<DebugRow>,
    /// How many of `rows` are live. The rest are retained allocations.
    used: usize,
}

impl DebugSection {
    /// An empty section with the given title.
    #[must_use]
    pub fn new(title: impl AsRef<str>) -> Self {
        let mut section = Self::default();
        section.set_title(title.as_ref());
        section
    }

    /// Sets the heading this section renders under.
    pub fn set_title(&mut self, title: &str) {
        self.title.clear();
        self.title.push_str(title);
    }

    /// The heading.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Appends a row, formatting `value` into a string this section already
    /// owns.
    ///
    /// Takes [`fmt::Arguments`] rather than a `String` so the caller writes
    /// `section.row("fps", format_args!("{fps:.1}"))` and nothing allocates
    /// after the first frame.
    pub fn row(&mut self, label: &str, value: fmt::Arguments<'_>) {
        if self.used == self.rows.len() {
            self.rows.push(DebugRow::default());
        }
        let row = &mut self.rows[self.used];
        row.label.clear();
        row.label.push_str(label);
        row.value.clear();
        // Writing into a `String` cannot fail; the `Result` is `fmt`'s shape.
        let _ = row.value.write_fmt(value);
        self.used += 1;
    }

    /// Appends a row whose value is already a string.
    pub fn row_str(&mut self, label: &str, value: &str) {
        self.row(label, format_args!("{value}"));
    }

    /// The live rows.
    #[must_use]
    pub fn rows(&self) -> &[DebugRow] {
        &self.rows[..self.used]
    }

    /// Whether this section has no rows.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Drops the title and the rows, keeping their allocations.
    pub fn clear(&mut self) {
        self.title.clear();
        self.used = 0;
    }
}

/// A system that can describe itself in the debug panel.
///
/// The one method is handed a section that has already been cleared, and fills
/// in a title and rows. Implement it on the thing that owns the numbers — the
/// renderer's frame timings, a network client's counters — never on the panel.
pub trait DebugModule {
    /// Writes this system's title and rows into `out`.
    fn debug_section(&self, out: &mut DebugSection);
}

// ---------------------------------------------------------------------------
// Frame statistics
// ---------------------------------------------------------------------------

/// How many frames [`FrameStats`] averages over by default.
///
/// Two seconds at 60 Hz. Short enough that a change in frame rate shows up
/// while you are still looking at it, long enough that the number is readable —
/// an instantaneous FPS at 60 Hz is a blur of four-digit noise.
pub const DEFAULT_FRAME_WINDOW: usize = 120;

/// A rolling window of frame intervals, and the numbers read off it.
///
/// Fed from the frame clock's render delta — the wall-clock time between the
/// last two `update`s — so every number below traces to one measurement the
/// engine already takes.
///
/// # FPS is the reciprocal of the mean, not the mean of the reciprocals
///
/// Given frames of 10 ms and 30 ms, the mean of `1/dt` is `(100 + 33.3)/2 = 67`
/// FPS, which is faster than either frame. The window actually took 40 ms and
/// contained 2 frames, so it ran at 50 FPS, which is what
/// [`FrameStats::fps`] reports: `count / total`. The two agree only when every
/// frame is the same length, and they disagree in exactly the case a profiler
/// exists for.
#[derive(Debug, Clone)]
pub struct FrameStats {
    window: usize,
    samples: VecDeque<Duration>,
    /// Kept incrementally so [`FrameStats::record`] is O(1); `Duration` addition
    /// is exact, so this cannot drift away from the sum of `samples`.
    total: Duration,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameStats {
    /// A window of [`DEFAULT_FRAME_WINDOW`] frames.
    #[must_use]
    pub fn new() -> Self {
        Self::with_window(DEFAULT_FRAME_WINDOW)
    }

    /// A window of `window` frames, clamped to at least one.
    #[must_use]
    pub fn with_window(window: usize) -> Self {
        let window = window.max(1);
        Self {
            window,
            samples: VecDeque::with_capacity(window),
            total: Duration::ZERO,
        }
    }

    /// Records one frame interval, evicting the oldest when the window is full.
    ///
    /// A zero-length interval is **not** a frame and is dropped.
    /// [`FrameClock::update`] reports `Duration::ZERO` twice: on the very first
    /// update, which has no previous timestamp to subtract, and on any update
    /// whose timestamp went backwards. Neither is a frame that took no time, and
    /// keeping them would report a frame rate that never happened.
    ///
    /// [`FrameClock::update`]: https://docs.rs/crcbl-core
    pub fn record(&mut self, dt: Duration) {
        if dt.is_zero() {
            return;
        }
        if self.samples.len() == self.window
            && let Some(oldest) = self.samples.pop_front()
        {
            self.total -= oldest;
        }
        self.samples.push_back(dt);
        self.total += dt;
    }

    /// How many frames are in the window.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no frame has been recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The window's capacity in frames.
    #[must_use]
    pub const fn window(&self) -> usize {
        self.window
    }

    /// The wall-clock time the window covers.
    #[must_use]
    pub const fn total(&self) -> Duration {
        self.total
    }

    /// The mean frame interval, or zero while the window is empty.
    #[must_use]
    pub fn mean(&self) -> Duration {
        let count = u32::try_from(self.samples.len()).unwrap_or(u32::MAX);
        if count == 0 {
            Duration::ZERO
        } else {
            self.total / count
        }
    }

    /// Frames per second over the window: frames divided by the time they took.
    ///
    /// Zero while the window is empty, and zero if the window somehow covers no
    /// time at all — an infinite frame rate is not a more honest answer.
    #[must_use]
    pub fn fps(&self) -> f64 {
        let seconds = self.total.as_secs_f64();
        if self.samples.is_empty() || seconds <= 0.0 {
            0.0
        } else {
            self.samples.len() as f64 / seconds
        }
    }

    /// The most recent frame interval.
    #[must_use]
    pub fn last(&self) -> Option<Duration> {
        self.samples.back().copied()
    }

    /// The shortest frame in the window.
    #[must_use]
    pub fn best(&self) -> Option<Duration> {
        self.samples.iter().min().copied()
    }

    /// The longest frame in the window — the stall, if there was one.
    #[must_use]
    pub fn worst(&self) -> Option<Duration> {
        self.samples.iter().max().copied()
    }

    /// Forgets every sample.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.total = Duration::ZERO;
    }
}

/// Milliseconds, for display.
fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

impl DebugModule for FrameStats {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("frame");
        out.row("fps", format_args!("{:.1}", self.fps()));
        out.row("avg", format_args!("{:.2} ms", millis(self.mean())));
        out.row(
            "last",
            format_args!("{:.2} ms", millis(self.last().unwrap_or_default())),
        );
        out.row(
            "best",
            format_args!("{:.2} ms", millis(self.best().unwrap_or_default())),
        );
        out.row(
            "worst",
            format_args!("{:.2} ms", millis(self.worst().unwrap_or_default())),
        );
        out.row("window", format_args!("{}/{}", self.len(), self.window()));
    }
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

/// Colours and spacing for the debug panel.
///
/// Separate from [`Style`](crate::widget::Style), which is the *widget* theme: a
/// game restyling its buttons must not restyle the profiler out of legibility.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebugStyle {
    /// Backing rectangle behind the whole panel.
    pub bg: [f32; 4],
    /// Section headings.
    pub title: [f32; 4],
    /// Row labels.
    pub label: [f32; 4],
    /// Row values.
    pub value: [f32; 4],
    /// Text size in pixels.
    pub font_size: f32,
    /// Space between the backing rect and the text.
    pub padding: f32,
    /// Space between the label column and the value column.
    pub column_gap: f32,
    /// Space between one section and the next.
    pub section_gap: f32,
}

impl Default for DebugStyle {
    fn default() -> Self {
        Self {
            bg: [0.04, 0.04, 0.07, 0.82],
            title: [0.45, 0.85, 1.0, 1.0],
            label: [0.72, 0.72, 0.78, 1.0],
            value: [1.0, 1.0, 1.0, 1.0],
            font_size: NATURAL_FONT_SIZE,
            padding: 6.0,
            column_gap: 10.0,
            section_gap: 6.0,
        }
    }
}

/// The overlay's panel: whatever sections were added this frame, stacked.
///
/// Hidden by default. See the [module docs](self) for the frame shape and for
/// why nothing happens while it is hidden.
#[derive(Debug, Clone)]
pub struct DebugPanel {
    visible: bool,
    /// Which corner it is pinned to. Top-right by default, because both existing
    /// samples put their game HUD at the top left.
    pub anchor: Anchor,
    /// Inset from that corner, in pixels.
    pub offset: Vec2,
    /// Colours and spacing.
    pub style: DebugStyle,
    sections: Vec<DebugSection>,
    /// How many of `sections` were filled this frame; the rest are retained
    /// allocations from a frame that had more modules.
    used: usize,
}

impl Default for DebugPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugPanel {
    /// A hidden panel with no sections.
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: false,
            anchor: Anchor::TopRight,
            offset: Vec2::splat(8.0),
            style: DebugStyle::default(),
            sections: Vec::new(),
            used: 0,
        }
    }

    /// Whether the panel draws anything.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows or hides the panel.
    ///
    /// Hiding drops the sections gathered this frame, so a hidden panel cannot
    /// render stale numbers if it is shown again without a `begin_frame`.
    pub const fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible {
            self.used = 0;
        }
    }

    /// Flips [`DebugPanel::is_visible`]. What the toggle key calls.
    pub const fn toggle(&mut self) {
        self.set_visible(!self.visible);
    }

    /// Starts a frame: forgets the sections the last one gathered.
    pub const fn begin_frame(&mut self) {
        self.used = 0;
    }

    /// Adds one system's section, in the order added.
    ///
    /// A no-op while the panel is hidden — `module`'s `debug_section` is not
    /// called at all, so a system pays nothing for a panel nobody is looking at.
    pub fn add(&mut self, module: &dyn DebugModule) {
        if !self.visible {
            return;
        }
        if self.used == self.sections.len() {
            self.sections.push(DebugSection::default());
        }
        let section = &mut self.sections[self.used];
        section.clear();
        module.debug_section(section);
        self.used += 1;
    }

    /// The sections gathered this frame, in the order they were added.
    #[must_use]
    pub fn sections(&self) -> &[DebugSection] {
        &self.sections[..self.used]
    }

    /// The panel's full extent including its padding, or zero if it would draw
    /// nothing.
    #[must_use]
    pub fn size(&self, atlas: &FontAtlas) -> Vec2 {
        self.layout(atlas).map_or(Vec2::ZERO, |layout| layout.size)
    }

    /// Draws the panel into `dl`.
    ///
    /// Emits nothing at all when the panel is hidden or has no sections —
    /// including no background rect, so an overlay with nothing to say is
    /// invisible rather than an empty box.
    pub fn render(&self, dl: &mut DrawList, screen_size: Vec2, atlas: &FontAtlas) {
        let Some(layout) = self.layout(atlas) else {
            return;
        };
        let style = &self.style;
        let origin = self.anchor.position(screen_size, self.offset, layout.size);
        dl.rect(origin, origin + layout.size, style.bg);

        let mut cursor = origin + Vec2::splat(style.padding);
        for section in self.sections() {
            dl.text(cursor, section.title(), style.title, style.font_size);
            cursor.y += layout.line_height;
            for row in section.rows() {
                dl.text(cursor, row.label.as_str(), style.label, style.font_size);
                dl.text(
                    Vec2::new(cursor.x + layout.value_column, cursor.y),
                    row.value.as_str(),
                    style.value,
                    style.font_size,
                );
                cursor.y += layout.line_height;
            }
            cursor.y += style.section_gap;
        }
    }

    /// Measures the panel, or `None` if it would draw nothing.
    ///
    /// There is deliberately **no** `if !self.visible` here, though there was:
    /// it could not be reached and therefore could not be tested. Visibility is
    /// enforced at the two places that can actually observe it —
    /// [`DebugPanel::add`] refuses to gather while hidden, and
    /// [`DebugPanel::set_visible`] drops whatever was gathered — so a hidden
    /// panel has no sections by construction and this returns `None` on the
    /// emptiness check below. A third copy of the rule would be a guard that
    /// silently matches nothing.
    fn layout(&self, atlas: &FontAtlas) -> Option<PanelLayout> {
        let sections = self.sections();
        if sections.is_empty() {
            return None;
        }
        let style = &self.style;
        let scale = style.font_size / NATURAL_FONT_SIZE;
        let line_height = LINE_HEIGHT * scale;

        let mut label_width = 0.0f32;
        let mut value_width = 0.0f32;
        let mut title_width = 0.0f32;
        let mut lines = 0.0f32;
        for section in sections {
            title_width = title_width.max(atlas.text_width(section.title(), scale));
            lines += 1.0;
            for row in section.rows() {
                label_width = label_width.max(atlas.text_width(&row.label, scale));
                value_width = value_width.max(atlas.text_width(&row.value, scale));
                lines += 1.0;
            }
        }

        let value_column = label_width + style.column_gap;
        let content_width = title_width.max(value_column + value_width);
        let gaps = (sections.len() - 1) as f32 * style.section_gap;
        let content_height = lines * line_height + gaps;
        Some(PanelLayout {
            size: Vec2::new(content_width, content_height) + Vec2::splat(style.padding * 2.0),
            value_column,
            line_height,
        })
    }
}

/// What [`DebugPanel::layout`] worked out, shared by measuring and drawing.
#[derive(Debug, Clone, Copy)]
struct PanelLayout {
    /// Padding included.
    size: Vec2,
    /// Where the value column starts, relative to the label column.
    value_column: f32,
    line_height: f32,
}

// ---------------------------------------------------------------------------
// Overlay
// ---------------------------------------------------------------------------

/// The panel plus the one module every frame has: [`FrameStats`].
///
/// This is what a sample holds. Frame timing has no precondition and no
/// configuration — every sample has a frame — so it is wired in here rather than
/// added by each sample, which is the difference between "switching it on is one
/// thing" and four samples each remembering to add the same module.
///
/// Everything else is [`DebugOverlay::add`], called only by a sample that has
/// the system in question.
#[derive(Debug, Clone, Default)]
pub struct DebugOverlay {
    /// The panel. Public so a sample can move it, restyle it or read its
    /// sections.
    pub panel: DebugPanel,
    /// The rolling frame window, fed by [`DebugOverlay::record`].
    pub frame: FrameStats,
}

impl DebugOverlay {
    /// A hidden overlay.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An overlay that starts visible or not.
    ///
    /// Samples pass `cfg!(debug_assertions) || options.debug_overlay` — rule 4's
    /// "on by default in dev builds", with a flag for the other cases.
    #[must_use]
    pub fn with_visible(visible: bool) -> Self {
        let mut overlay = Self::new();
        overlay.panel.set_visible(visible);
        overlay
    }

    /// Whether the panel draws.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.panel.is_visible()
    }

    /// Shows or hides the panel.
    pub const fn set_visible(&mut self, visible: bool) {
        self.panel.set_visible(visible);
    }

    /// Flips the panel. What a sample's toggle key calls.
    pub const fn toggle(&mut self) {
        self.panel.toggle();
    }

    /// Records one frame interval. Call every frame, hidden or not.
    pub fn record(&mut self, dt: Duration) {
        self.frame.record(dt);
    }

    /// Starts the frame and adds the frame-timing section.
    pub fn begin_frame(&mut self) {
        self.panel.begin_frame();
        self.panel.add(&self.frame);
    }

    /// Adds a system's section, after [`DebugOverlay::begin_frame`].
    pub fn add(&mut self, module: &dyn DebugModule) {
        self.panel.add(module);
    }

    /// Draws the panel.
    pub fn render(&self, dl: &mut DrawList, screen_size: Vec2, atlas: &FontAtlas) {
        self.panel.render(dl, screen_size, atlas);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::DrawCommand;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn screen() -> Vec2 {
        Vec2::new(1280.0, 720.0)
    }

    /// Every `Text` command's string, in draw order.
    fn texts(dl: &DrawList) -> Vec<String> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// A module that says what it is told to, for testing composition.
    struct Fake {
        title: &'static str,
        rows: &'static [(&'static str, &'static str)],
    }

    impl DebugModule for Fake {
        fn debug_section(&self, out: &mut DebugSection) {
            out.set_title(self.title);
            for (label, value) in self.rows {
                out.row_str(label, value);
            }
        }
    }

    fn shown() -> DebugPanel {
        let mut panel = DebugPanel::new();
        panel.set_visible(true);
        panel.begin_frame();
        panel
    }

    // -- frame maths --------------------------------------------------------

    /// A known sequence in, known numbers out. 10 + 30 = 40 ms for 2 frames, so
    /// 50 FPS and a 20 ms mean — *not* the 67 FPS a mean of reciprocals gives.
    #[test]
    fn fps_is_frames_over_elapsed_not_the_mean_of_instantaneous_rates() {
        let mut stats = FrameStats::new();
        stats.record(ms(10));
        stats.record(ms(30));
        assert_eq!(stats.len(), 2);
        assert_eq!(stats.total(), ms(40));
        assert_eq!(stats.mean(), ms(20));
        assert!(
            (stats.fps() - 50.0).abs() < 1e-9,
            "expected 50 fps, got {}",
            stats.fps()
        );
        assert_eq!(stats.best(), Some(ms(10)));
        assert_eq!(stats.worst(), Some(ms(30)));
        assert_eq!(stats.last(), Some(ms(30)));
    }

    /// Sixteen frames of exactly 1/64 s: 64 FPS, and every reported number
    /// agrees because nothing varied.
    #[test]
    fn a_steady_sequence_reports_its_own_rate() {
        let mut stats = FrameStats::new();
        let step = Duration::from_nanos(15_625_000); // 1/64 s exactly
        for _ in 0..16 {
            stats.record(step);
        }
        assert_eq!(stats.mean(), step);
        assert!((stats.fps() - 64.0).abs() < 1e-9, "got {}", stats.fps());
        assert_eq!(stats.best(), Some(step));
        assert_eq!(stats.worst(), Some(step));
    }

    /// **The first frame.** `FrameClock::update` has no previous timestamp on
    /// its first call and reports `Duration::ZERO`; so does an update whose
    /// timestamp went backwards. Neither is a frame that took no time, and a
    /// window holding one would divide by zero.
    #[test]
    fn the_first_frames_zero_delta_is_not_a_sample_and_fps_is_not_infinite() {
        let mut stats = FrameStats::new();
        stats.record(Duration::ZERO);
        assert!(stats.is_empty(), "a zero-length interval is not a frame");
        assert_eq!(stats.fps(), 0.0);
        assert!(stats.fps().is_finite());
        assert_eq!(stats.mean(), Duration::ZERO);
        assert_eq!(stats.last(), None);

        // And the section it renders is finite too, rather than `inf` or `NaN`.
        let mut section = DebugSection::default();
        stats.debug_section(&mut section);
        for row in section.rows() {
            assert!(
                !row.value.contains("inf") && !row.value.contains("NaN"),
                "{}: {}",
                row.label,
                row.value
            );
        }
    }

    /// **A long stall.** One 500 ms frame among sixty 16 ms ones must show up as
    /// the worst frame and must drag the average, or a hitch is invisible.
    #[test]
    fn a_stall_shows_in_the_worst_frame_and_moves_the_average() {
        let mut stats = FrameStats::new();
        for _ in 0..60 {
            stats.record(ms(16));
        }
        let smooth = stats.mean();
        stats.record(ms(500));
        assert_eq!(stats.worst(), Some(ms(500)));
        assert_eq!(stats.best(), Some(ms(16)));
        assert_eq!(stats.last(), Some(ms(500)));
        // 60×16 + 500 = 1460 ms over 61 frames.
        assert_eq!(stats.total(), ms(1460));
        assert_eq!(stats.mean(), ms(1460) / 61);
        assert!(
            stats.mean() > smooth,
            "the stall must move the average: {smooth:?} → {:?}",
            stats.mean()
        );
        assert!(
            (stats.fps() - 61.0 / 1.46).abs() < 1e-9,
            "got {}",
            stats.fps()
        );
    }

    /// The window evicts, and the total it keeps incrementally stays exactly the
    /// sum of what is left — so a stall that has scrolled off stops counting.
    #[test]
    fn the_window_evicts_and_the_running_total_matches_the_samples() {
        let mut stats = FrameStats::with_window(4);
        stats.record(ms(500));
        for _ in 0..4 {
            stats.record(ms(10));
        }
        assert_eq!(stats.len(), 4, "the window holds four");
        assert_eq!(stats.total(), ms(40), "the stall has scrolled off");
        assert_eq!(stats.worst(), Some(ms(10)));
        assert!((stats.fps() - 100.0).abs() < 1e-9, "got {}", stats.fps());
    }

    /// A zero window would divide by zero on eviction; it is clamped to one.
    #[test]
    fn a_zero_window_is_clamped_to_one_frame() {
        let mut stats = FrameStats::with_window(0);
        assert_eq!(stats.window(), 1);
        stats.record(ms(10));
        stats.record(ms(20));
        assert_eq!(stats.len(), 1);
        assert_eq!(stats.total(), ms(20));
    }

    // -- modularity ---------------------------------------------------------

    /// **A panel with no module but the frame's renders.** This is breakout and
    /// flappy's case: no connection, so no network module, and the panel is
    /// still a panel. Asserting "it did not crash" would pass on a panel that
    /// drew nothing, so this asserts the frame numbers are actually in the draw
    /// list.
    #[test]
    fn a_panel_with_only_the_frame_module_draws_the_frame_numbers() {
        let atlas = FontAtlas::built_in();
        let mut overlay = DebugOverlay::with_visible(true);
        overlay.record(ms(20));
        overlay.record(ms(20));
        overlay.begin_frame();

        let mut dl = DrawList::new();
        overlay.render(&mut dl, screen(), &atlas);

        let drawn = texts(&dl);
        assert_eq!(
            drawn.first().map(String::as_str),
            Some("frame"),
            "the section heading: {drawn:?}"
        );
        assert!(drawn.iter().any(|t| t == "fps"), "{drawn:?}");
        assert!(
            drawn.iter().any(|t| t == "50.0"),
            "two 20 ms frames is 50 fps: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t == "20.00 ms"),
            "the mean must be drawn: {drawn:?}"
        );
        assert!(
            dl.commands()
                .iter()
                .any(|c| matches!(c, DrawCommand::Rect { .. })),
            "the panel draws its own background"
        );
        assert_eq!(overlay.panel.sections().len(), 1, "no network module");
    }

    /// Two modules render as two sections, in the order they were added, with
    /// every row of each.
    #[test]
    fn two_modules_render_both_sections_in_the_order_added() {
        let atlas = FontAtlas::built_in();
        let first = Fake {
            title: "alpha",
            rows: &[("a1", "one"), ("a2", "two")],
        };
        let second = Fake {
            title: "beta",
            rows: &[("b1", "three")],
        };

        let mut panel = shown();
        panel.add(&first);
        panel.add(&second);

        assert_eq!(
            panel
                .sections()
                .iter()
                .map(DebugSection::title)
                .collect::<Vec<_>>(),
            ["alpha", "beta"],
        );

        let mut dl = DrawList::new();
        panel.render(&mut dl, screen(), &atlas);
        assert_eq!(
            texts(&dl),
            ["alpha", "a1", "one", "a2", "two", "beta", "b1", "three"],
            "titles and rows, in order",
        );

        // And the reverse order is genuinely a different draw list.
        let mut swapped = shown();
        swapped.add(&second);
        swapped.add(&first);
        let mut other = DrawList::new();
        swapped.render(&mut other, screen(), &atlas);
        assert_eq!(
            texts(&other),
            ["beta", "b1", "three", "alpha", "a1", "one", "a2", "two"],
        );
    }

    /// The section count follows the modules added, so a frame with fewer
    /// modules than the last does not render the leftovers.
    #[test]
    fn a_frame_with_fewer_modules_does_not_render_the_previous_frames() {
        let atlas = FontAtlas::built_in();
        let first = Fake {
            title: "alpha",
            rows: &[("a1", "one")],
        };
        let second = Fake {
            title: "beta",
            rows: &[("b1", "two")],
        };

        let mut panel = shown();
        panel.add(&first);
        panel.add(&second);
        assert_eq!(panel.sections().len(), 2);

        panel.begin_frame();
        panel.add(&first);
        let mut dl = DrawList::new();
        panel.render(&mut dl, screen(), &atlas);
        let drawn = texts(&dl);
        assert_eq!(panel.sections().len(), 1);
        assert!(
            !drawn.iter().any(|t| t == "beta"),
            "the dropped module is gone: {drawn:?}"
        );
    }

    /// A panel that gathered nothing draws nothing — not an empty box.
    #[test]
    fn a_panel_with_no_sections_at_all_draws_nothing() {
        let atlas = FontAtlas::built_in();
        let panel = shown();
        let mut dl = DrawList::new();
        panel.render(&mut dl, screen(), &atlas);
        assert!(dl.is_empty(), "{:?}", dl.commands());
        assert_eq!(panel.size(&atlas), Vec2::ZERO);
    }

    // -- the toggle ---------------------------------------------------------

    /// **Toggling changes what is drawn.** Asserted on the draw list, not on
    /// `is_visible`: a toggle that flipped a bool nothing read would pass a
    /// bool assertion and fail the user.
    #[test]
    fn toggling_the_panel_changes_the_draw_list() {
        let atlas = FontAtlas::built_in();
        let module = Fake {
            title: "alpha",
            rows: &[("a1", "one")],
        };
        let mut panel = DebugPanel::new();

        let draw = |panel: &mut DebugPanel| {
            panel.begin_frame();
            panel.add(&module);
            let mut dl = DrawList::new();
            panel.render(&mut dl, screen(), &atlas);
            dl
        };

        // Off: no commands, and the module was never even asked.
        let off = draw(&mut panel);
        assert!(off.is_empty(), "hidden: {:?}", off.commands());
        assert!(panel.sections().is_empty(), "hidden panels gather nothing");

        // On: the module's rows.
        panel.toggle();
        let on = draw(&mut panel);
        assert!(texts(&on).iter().any(|t| t == "alpha"), "{:?}", texts(&on));

        // Off again: back to nothing.
        panel.toggle();
        let off_again = draw(&mut panel);
        assert!(off_again.is_empty(), "{:?}", off_again.commands());
    }

    /// Hiding drops what was gathered, so showing again without a `begin_frame`
    /// cannot render the numbers from whenever it was last visible.
    #[test]
    fn hiding_forgets_the_sections_it_had() {
        let atlas = FontAtlas::built_in();
        let mut panel = shown();
        panel.add(&Fake {
            title: "alpha",
            rows: &[("a1", "one")],
        });
        assert_eq!(panel.sections().len(), 1);

        panel.set_visible(false);
        panel.set_visible(true);
        assert!(panel.sections().is_empty());
        let mut dl = DrawList::new();
        panel.render(&mut dl, screen(), &atlas);
        assert!(dl.is_empty(), "{:?}", dl.commands());
    }

    // -- layout -------------------------------------------------------------

    /// **A resize.** The panel is anchored, so its rectangle has to follow the
    /// screen; a top-right panel that kept its old x runs off a narrowed window.
    #[test]
    fn the_panel_follows_the_screen_across_a_resize() {
        let atlas = FontAtlas::built_in();
        let mut panel = shown();
        panel.add(&Fake {
            title: "alpha",
            rows: &[("a1", "one")],
        });
        let size = panel.size(&atlas);

        let bounds = |screen: Vec2| {
            let mut dl = DrawList::new();
            panel.render(&mut dl, screen, &atlas);
            match dl.commands().first() {
                Some(DrawCommand::Rect { min, max, .. }) => (*min, *max),
                other => panic!("expected the panel background, got {other:?}"),
            }
        };

        for screen in [Vec2::new(1280.0, 720.0), Vec2::new(640.0, 360.0)] {
            let (min, max) = bounds(screen);
            assert_eq!(max - min, size, "the panel is the same size either way");
            assert_eq!(
                max,
                Vec2::new(screen.x - panel.offset.x, min.y + size.y),
                "the right edge stays {}px in from the right of {screen:?}",
                panel.offset.x,
            );
            assert!(
                min.x >= 0.0 && max.x <= screen.x,
                "off-screen: {min:?}..{max:?}"
            );
        }

        // The two screens really are different positions — otherwise the
        // assertions above would hold for a panel pinned to a constant.
        assert_ne!(
            bounds(Vec2::new(1280.0, 720.0)).0,
            bounds(Vec2::new(640.0, 360.0)).0
        );
    }

    /// A resize does not touch the frame window: the long frame a resize costs
    /// is a sample like any other, not a reason to forget the last two seconds.
    #[test]
    fn a_resize_does_not_reset_the_frame_window() {
        let mut overlay = DebugOverlay::with_visible(true);
        for _ in 0..10 {
            overlay.record(ms(16));
        }
        overlay.record(ms(120)); // the frame the swapchain was rebuilt on
        assert_eq!(overlay.frame.len(), 11);
        assert_eq!(overlay.frame.worst(), Some(ms(120)));
        assert_eq!(overlay.frame.best(), Some(ms(16)));
    }

    /// The value column clears the longest label, so nothing overlaps.
    #[test]
    fn the_value_column_starts_past_the_longest_label() {
        let atlas = FontAtlas::built_in();
        let mut panel = shown();
        panel.add(&Fake {
            title: "x",
            rows: &[("short", "1"), ("a-much-longer-label", "2")],
        });
        let mut dl = DrawList::new();
        panel.render(&mut dl, screen(), &atlas);

        let scale = panel.style.font_size / NATURAL_FONT_SIZE;
        let widest = atlas.text_width("a-much-longer-label", scale);
        let mut label_x = None;
        // The loop below only asserts inside its match arms, so a render that
        // emitted no text — or one whose labels stopped being these literals —
        // would run the body zero times and pass. The count is what makes the
        // arms load-bearing.
        let mut checked = 0;
        for command in dl.commands() {
            if let DrawCommand::Text { pos, text, .. } = command {
                match text.as_str() {
                    "short" | "a-much-longer-label" => label_x = Some(pos.x),
                    "1" | "2" => {
                        let start = label_x.expect("a label precedes its value");
                        assert!(
                            pos.x >= start + widest,
                            "value at {} overlaps a {widest}px label at {start}",
                            pos.x,
                        );
                        checked += 1;
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(
            checked,
            2,
            "both rows' values must have been placed and checked, in {:?}",
            dl.commands()
        );
    }

    /// Rows and titles are reused across frames rather than reallocated.
    #[test]
    fn a_section_reuses_its_row_allocations() {
        let mut section = DebugSection::new("frame");
        section.row("fps", format_args!("{:.1}", 59.94));
        section.row("avg", format_args!("{:.2} ms", 16.68));
        assert_eq!(section.rows().len(), 2);
        assert_eq!(section.rows()[0].value, "59.9");
        // The backing `Vec`'s *length*, not its capacity: capacity is 4 from the
        // first push either way, so a version of `clear` that threw the rows
        // away would compare equal and this test would check nothing.
        assert_eq!(section.rows.len(), 2, "two row slots exist");

        section.clear();
        assert!(section.is_empty(), "no live rows");
        assert_eq!(section.title(), "");
        assert_eq!(
            section.rows.len(),
            2,
            "clear keeps the slots — that is the whole point of it",
        );

        section.row("fps", format_args!("{:.1}", 30.0));
        assert_eq!(section.rows().len(), 1, "one live row");
        assert_eq!(section.rows()[0].value, "30.0");
        assert_eq!(section.rows.len(), 2, "and the second slot is still there");
    }

    #[test]
    fn the_overlay_starts_hidden_unless_asked() {
        assert!(!DebugOverlay::new().is_visible());
        assert!(DebugOverlay::with_visible(true).is_visible());
        let mut overlay = DebugOverlay::with_visible(true);
        overlay.toggle();
        assert!(!overlay.is_visible());
        overlay.set_visible(true);
        assert!(overlay.is_visible());
    }

    #[test]
    fn debug_format_covers_the_public_types() {
        let overlay = DebugOverlay::with_visible(true);
        let text = format!("{overlay:?}");
        assert!(text.contains("panel"), "{text}");
        assert!(text.contains("visible"), "{text}");
        assert_eq!(
            format!("{:?}", DebugRow::default()),
            "DebugRow { label: \"\", value: \"\" }"
        );
    }
}
