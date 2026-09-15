//! Where an overlay panel sits on the screen: [`Anchor`].
//!
//! All that is left of the pre-layout HUD skeleton. Its `Hud` and `HudPanel`
//! had no caller and are gone now that [`crate::tree`] lays panels out; the
//! debug overlay still anchors itself against a corner with this.

use glam::Vec2;

/// Which corner (or the centre) of the screen a panel is positioned against.
///
/// In every arm the panel's `offset` is an **inset from that anchor**, and the
/// panel's content grows away from it: a `TopRight` panel's right edge sits
/// `offset.x` in from the right of the screen. `Center` centres the panel and
/// then applies `offset` as a nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Inset from the top-left corner.
    TopLeft,
    /// Inset from the top-right corner.
    TopRight,
    /// Inset from the bottom-left corner.
    BottomLeft,
    /// Inset from the bottom-right corner.
    BottomRight,
    /// Centred, then nudged by `offset`.
    Center,
}

impl Anchor {
    /// The top-left pixel position of a `content`-sized box inset by `offset`
    /// from this anchor on a `screen_size` screen.
    ///
    /// The content extent is needed because `offset` is an *inset*: a
    /// right-anchored box's right edge is `offset.x` in from the right of the
    /// screen, so its left edge — what this returns — depends on how wide it is.
    /// Returning `screen.x - offset.x` as the left edge, as the retired
    /// `HudPanel` once did, runs a right-anchored panel straight off the
    /// screen.
    #[must_use]
    pub fn position(self, screen_size: Vec2, offset: Vec2, content: Vec2) -> Vec2 {
        let far = screen_size - offset - content;
        match self {
            Self::TopLeft => offset,
            Self::TopRight => Vec2::new(far.x, offset.y),
            Self::BottomLeft => Vec2::new(offset.x, far.y),
            Self::BottomRight => far,
            Self::Center => (screen_size - content) * 0.5 + offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Vec2 {
        Vec2::new(800.0, 600.0)
    }

    /// The bug: `offset` meant "inset" for `TopLeft` but "left edge" for
    /// `TopRight`, so a right-anchored panel ran off the screen, and `Center`
    /// halved the offset instead of applying it.
    #[test]
    fn every_anchor_insets_the_panel_by_offset() {
        let content = Vec2::new(100.0, 40.0);
        let offset = Vec2::splat(10.0);
        let size = screen();
        let at = |anchor: Anchor| anchor.position(size, offset, content);

        assert_eq!(at(Anchor::TopLeft), Vec2::new(10.0, 10.0));
        // Right edge sits 10px in from the right: 800 - 10 - 100 = 690.
        assert_eq!(at(Anchor::TopRight), Vec2::new(690.0, 10.0));
        // Bottom edge sits 10px up from the bottom: 600 - 10 - 40 = 550.
        assert_eq!(at(Anchor::BottomLeft), Vec2::new(10.0, 550.0));
        assert_eq!(at(Anchor::BottomRight), Vec2::new(690.0, 550.0));
        // Centred, then nudged: (800-100)/2 + 10, (600-40)/2 + 10.
        assert_eq!(at(Anchor::Center), Vec2::new(360.0, 290.0));
    }

    /// Every anchored panel must be fully on-screen for any offset that fits.
    #[test]
    fn anchored_panels_stay_on_screen() {
        let content = Vec2::new(120.0, 60.0);
        let size = screen();
        for anchor in [
            Anchor::TopLeft,
            Anchor::TopRight,
            Anchor::BottomLeft,
            Anchor::BottomRight,
        ] {
            let pos = anchor.position(size, Vec2::splat(8.0), content);
            assert!(pos.x >= 0.0 && pos.y >= 0.0, "{anchor:?} → {pos:?}");
            let far = pos + content;
            assert!(far.x <= size.x && far.y <= size.y, "{anchor:?} → {far:?}");
        }
    }
}
