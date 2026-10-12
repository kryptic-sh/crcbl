//! A pointer a stick drives: for a screen built for a mouse — an inventory, a
//! map — on a pad.
//!
//! The stick's filtered direction moves the pointer at a fixed speed in
//! logical pixels, so it crosses a screen in the same time at any UI scale; a
//! caller whose pointer is in physical pixels converts at its boundary. The
//! pointer is the caller's: the same position the mouse writes, so the two
//! hand over without a jump.

use glam::Vec2;

use crate::draw_list::DrawList;

/// How far a cross arm reaches from the pointer, in logical pixels.
const ARM: f32 = 7.0;
/// The dark outline's thickness, under the light stroke.
const OUTLINE: f32 = 4.0;
/// The light stroke's thickness.
const STROKE: f32 = 2.0;

/// A stick-driven pointer and whether it has been driven since it was last
/// hidden.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickCursor {
    speed: f32,
    shown: bool,
}

impl StickCursor {
    /// A cursor moving `speed` logical pixels a second at full throw, hidden
    /// until it first moves. `None` for a speed that is not finite and
    /// positive.
    #[must_use]
    pub fn new(speed: f32) -> Option<Self> {
        (speed.is_finite() && speed > 0.0).then_some(Self {
            speed,
            shown: false,
        })
    }

    /// Moves `pointer` by `direction` (a filtered stick, inside the unit
    /// disc) for `dt` seconds, kept inside a screen `extent` logical pixels
    /// across, and shows the cursor. A pointer with no position yet starts at
    /// the screen's centre. A zero direction changes nothing.
    pub fn advance(&mut self, pointer: &mut Option<Vec2>, direction: Vec2, extent: Vec2, dt: f64) {
        if direction == Vec2::ZERO || !direction.is_finite() || !dt.is_finite() {
            return;
        }
        let last = (extent - Vec2::ONE).max(Vec2::ZERO);
        let from = pointer.unwrap_or(last * 0.5);
        *pointer = Some((from + direction * self.speed * dt as f32).clamp(Vec2::ZERO, last));
        self.shown = true;
    }

    /// Whether the stick has moved it since it was last hidden.
    #[must_use]
    pub const fn shown(&self) -> bool {
        self.shown
    }

    /// Shows it without moving the pointer — when a pad button acts on the
    /// pointer while the stick rests, say.
    pub const fn show(&mut self) {
        self.shown = true;
    }

    /// Hides it — when the mouse moves, say, and the system cursor is back.
    pub const fn hide(&mut self) {
        self.shown = false;
    }

    /// Draws a cross at `pointer`, light over dark so it reads on any
    /// background, if the cursor is shown.
    pub fn draw(&self, list: &mut DrawList, pointer: Option<Vec2>) {
        let Some(at) = pointer.filter(|_| self.shown) else {
            return;
        };
        for (thickness, color) in [(OUTLINE, [0.0, 0.0, 0.0, 1.0]), (STROKE, [1.0; 4])] {
            for axis in [Vec2::X, Vec2::Y] {
                list.line(at - axis * ARM, at + axis * ARM, thickness, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTENT: Vec2 = Vec2::new(960.0, 720.0);

    /// **It moves at its speed, starts at the centre, and stops at the
    /// edges.**
    #[test]
    fn the_cursor_moves_at_its_speed_and_stays_on_screen() {
        let mut cursor = StickCursor::new(700.0).unwrap();
        let mut pointer = None;
        cursor.advance(&mut pointer, Vec2::ZERO, EXTENT, 1.0);
        assert_eq!(pointer, None, "a resting stick does not conjure a pointer");
        assert!(!cursor.shown());
        cursor.advance(&mut pointer, Vec2::new(0.5, 0.0), EXTENT, 0.1);
        let centre = (EXTENT - Vec2::ONE) * 0.5;
        assert_eq!(pointer, Some(centre + Vec2::new(35.0, 0.0)));
        assert!(cursor.shown());
        cursor.advance(&mut pointer, Vec2::new(1.0, -1.0), EXTENT, 10.0);
        assert_eq!(pointer, Some(Vec2::new(EXTENT.x - 1.0, 0.0)));
    }

    /// **It draws only while shown**, and hiding it takes the cross away.
    #[test]
    fn the_cross_is_drawn_only_while_shown() {
        let mut cursor = StickCursor::new(700.0).unwrap();
        let mut pointer = Some(Vec2::splat(100.0));
        let mut hidden = DrawList::new();
        cursor.draw(&mut hidden, pointer);
        assert!(hidden.commands().is_empty());
        cursor.advance(&mut pointer, Vec2::X, EXTENT, 0.01);
        let mut drawn = DrawList::new();
        cursor.draw(&mut drawn, pointer);
        assert_eq!(drawn.commands().len(), 4, "two strokes of two arms");
        cursor.hide();
        let mut again = DrawList::new();
        cursor.draw(&mut again, pointer);
        assert!(again.commands().is_empty());
        let before = pointer;
        cursor.show();
        assert_eq!(pointer, before, "showing does not move it");
        let mut shown = DrawList::new();
        cursor.draw(&mut shown, pointer);
        assert_eq!(shown.commands().len(), 4, "a button can show it");
    }

    /// Bad speeds are refused, and a non-finite direction or step is ignored.
    #[test]
    fn bad_input_is_refused() {
        assert!(StickCursor::new(0.0).is_none());
        assert!(StickCursor::new(f32::INFINITY).is_none());
        let mut cursor = StickCursor::new(700.0).unwrap();
        let mut pointer = Some(Vec2::splat(10.0));
        cursor.advance(&mut pointer, Vec2::new(f32::NAN, 0.0), EXTENT, 0.1);
        cursor.advance(&mut pointer, Vec2::X, EXTENT, f64::NAN);
        assert_eq!(pointer, Some(Vec2::splat(10.0)));
        assert!(!cursor.shown());
    }
}
