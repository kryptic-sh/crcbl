//! **A document opening in the background**, and the line on screen that says
//! so.
//!
//! A re-export, a drop on the window and a shelf row each open a document
//! while another one is on screen. Reading and converting it, and above all
//! building its renderer — seconds of pipeline creation on a cold shader cache
//! — used to happen inside a frame, on the thread that owns the window, which
//! Win32 shows as a window that is not responding. Natively they now run on a
//! worker through `crate::gpu::Gpu::build_in_background` while the old
//! document keeps drawing, and [`Opening`] is what the frame remembers about
//! the one in flight: which door it came through, which decides what happens
//! when it lands, and the name to show meanwhile.
//!
//! The browser opens on the frame as before: a page has no thread that can
//! reach the device.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use crcbl::ui::{DebugStyle, NATURAL_FONT_SIZE};

/// Which door a document came through, which decides what landing means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    /// The document on screen, written again: the camera stays where the
    /// artist aimed it.
    ReExport,
    /// A file dropped on the window: framed afresh, and watched from now on.
    Dropped,
    /// A row on the shelf panel: framed and watched as a drop is.
    Shelf,
}

/// The document a worker is opening, from the frame that started it to the
/// frame it lands on.
#[derive(Debug)]
pub struct Opening {
    /// The door it came through.
    pub door: Door,
    /// What the line on screen calls it — the file's name.
    pub name: String,
    /// Seconds since it started, which is what moves the bar.
    pub secs: f64,
}

/// Seconds the bar's lit segment takes to cross the track once.
///
/// There is no fraction to show: the worker reports nothing until it is done,
/// so the bar says "still working" by moving, at a pace slow enough to read as
/// patience rather than alarm.
pub const SWEEP_SECS: f64 = 1.5;
/// The track's height in pixels, under the line of text.
const BAR_HEIGHT: f32 = 4.0;
/// The lit segment's share of the track's width.
const SEGMENT_SHARE: f32 = 0.25;
/// The track the segment crosses: the label colour, faint, so the moving part
/// is the one that reads.
const TRACK: [f32; 4] = [0.72, 0.72, 0.78, 0.25];
/// The gap between the panel and the bottom-left corner of the window.
const MARGIN: f32 = 12.0;

impl Opening {
    /// Starts the count for a document opening through `door`.
    #[must_use]
    pub const fn new(door: Door, name: String) -> Self {
        Self {
            door,
            name,
            secs: 0.0,
        }
    }

    /// The line on screen.
    #[must_use]
    pub fn line(&self) -> String {
        format!("Opening {}…", self.name)
    }

    /// Where the lit segment's left edge sits, as a share of the track it can
    /// travel: `0.0` at the left, `1.0` with the segment at the right end.
    #[must_use]
    pub fn sweep(&self) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        {
            (self.secs / SWEEP_SECS).fract() as f32
        }
    }

    /// Draws the panel at the bottom-left of a `screen`-sized window: the line,
    /// and under it a track with a segment that crosses it every
    /// [`SWEEP_SECS`]. In the debug panel's colours, as the listing is.
    pub fn render(&self, list: &mut DrawList, screen: Vec2, atlas: &FontAtlas) {
        let style = DebugStyle::default();
        let line = self.line();
        let text_width = atlas.text_width(&line, style.font_size / NATURAL_FONT_SIZE);
        let size = Vec2::new(
            text_width + 2.0 * style.padding,
            style.font_size + BAR_HEIGHT + 3.0 * style.padding,
        );
        let origin = Vec2::new(MARGIN, screen.y - MARGIN - size.y);
        list.rect(origin, origin + size, style.bg);
        let inner = origin + Vec2::splat(style.padding);
        list.text(inner, line, style.value, style.font_size);

        let track = Vec2::new(inner.x, inner.y + style.font_size + style.padding);
        let width = text_width;
        list.rect(track, track + Vec2::new(width, BAR_HEIGHT), TRACK);
        let segment = width * SEGMENT_SHARE;
        let left = track.x + (width - segment) * self.sweep();
        list.rect(
            Vec2::new(left, track.y),
            Vec2::new(left + segment, track.y + BAR_HEIGHT),
            style.graph,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_crosses_the_track_once_a_sweep_and_starts_again() {
        let mut opening = Opening::new(Door::Dropped, "chair.glb".into());
        assert_eq!(opening.line(), "Opening chair.glb…");
        assert!(
            opening.sweep().abs() < f32::EPSILON,
            "it starts at the left"
        );
        opening.secs = SWEEP_SECS / 2.0;
        assert!((opening.sweep() - 0.5).abs() < 1e-6, "{}", opening.sweep());
        opening.secs = SWEEP_SECS * 2.25;
        assert!(
            (opening.sweep() - 0.25).abs() < 1e-6,
            "a sweep that does not wrap leaves the segment parked at the right: {}",
            opening.sweep()
        );
    }

    #[test]
    fn the_panel_sits_in_the_bottom_left_corner_with_its_bar_moving() {
        let atlas = FontAtlas::built_in();
        let screen = Vec2::new(640.0, 480.0);
        let mut opening = Opening::new(Door::Shelf, "suzanne.glb".into());
        let mut early = DrawList::new();
        opening.render(&mut early, screen, &atlas);
        opening.secs = SWEEP_SECS / 2.0;
        let mut later = DrawList::new();
        opening.render(&mut later, screen, &atlas);

        let rects = |list: &DrawList| -> Vec<(Vec2, Vec2)> {
            list.commands()
                .iter()
                .filter_map(|command| match command {
                    crcbl::ui::draw_list::DrawCommand::Rect { min, max, .. } => Some((*min, *max)),
                    _ => None,
                })
                .collect()
        };
        let (panel, track, segment) = match rects(&early)[..] {
            [panel, track, segment] => (panel, track, segment),
            ref other => panic!("a panel, a track and a segment: {other:?}"),
        };
        assert!(
            (panel.0.x - MARGIN).abs() < f32::EPSILON
                && (panel.1.y - (screen.y - MARGIN)).abs() < 1e-3,
            "the panel is not in the bottom-left corner: {panel:?}"
        );
        assert!(
            track.0.y > panel.0.y && track.1.y <= panel.1.y,
            "the track is not inside the panel: {track:?} in {panel:?}"
        );
        assert!(
            (segment.0.x - track.0.x).abs() < f32::EPSILON,
            "a fresh open starts its segment at the left of the track"
        );
        let moved = rects(&later)[2];
        assert!(
            moved.0.x > segment.0.x,
            "half a sweep later the segment has not moved: {moved:?} against {segment:?}"
        );
    }
}
