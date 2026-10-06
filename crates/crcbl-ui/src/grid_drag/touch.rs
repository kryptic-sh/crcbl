//! A finger's half of the drag: long press to lift, drag, lift the finger to
//! drop.
//!
//! # Why a long press and not a press
//!
//! A press is how a pointer takes hold, because a mouse button down means
//! nothing else. A finger down means many things — the start of a scroll, a
//! tap, a swipe across the panel — and the one a drag cannot be told from
//! until it is over is the scroll. Holding still is what no other gesture
//! does, so a finger lifts an item only once it has stayed within
//! [`LONG_PRESS_SLOP`] of where it landed for [`LONG_PRESS`]; one that moves
//! further first is given up, for whatever else the game does with it.
//!
//! # Contacts arrive between frames
//!
//! [`GridDrag::touch`] takes each contact event as the shell reported it, the
//! way `crate::touch`'s controls take theirs, and the frame
//! ([`GridDrag::frame_with`]) is what advances the press by the time that
//! passed and asks the grid under the finger for something to lift. The
//! finger's primary contact also reaches a game as the pointer; a caller that
//! offers contacts here must not also hand the drag that contact's pointer, or
//! the pointer's press takes hold before the long press could.

use core::time::Duration;

use crcbl_core::input::{ContactId, TouchPhase};
use glam::Vec2;

use super::{GridDrag, Hand};

/// How long a finger has to stay put before it lifts what is under it.
///
/// UIKit's default for a long press, so a player who has held an item on one
/// phone's grid finds it lifts in the time they expect on this one.
pub const LONG_PRESS: Duration = Duration::from_millis(500);

/// How far a pressing finger may wander, in screen pixels, and still be held
/// still: UIKit's default allowance for a long press, which it measures in
/// points.
pub const LONG_PRESS_SLOP: f32 = 10.0;

/// The finger the drag is following.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Touch {
    pub(super) contact: ContactId,
    /// Where it is now.
    pub(super) at: Vec2,
    /// Where it landed: what the slop is measured from.
    from: Vec2,
    /// How long it has been down, as the frames counted it.
    held_for: Duration,
    /// How a carrying finger's contact ended, since the last frame.
    pub(super) end: Option<TouchEnd>,
}

/// How the contact carrying a drag ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TouchEnd {
    /// [`TouchPhase::Ended`]: the drag lands where the finger lifted.
    Lifted,
    /// [`TouchPhase::Cancelled`]: the system took the gesture, so the drag
    /// goes back where it started.
    Cancelled,
}

impl Touch {
    /// Counts `dt` more of the press, and answers whether it has now been
    /// held [`LONG_PRESS`].
    pub(super) fn ripen(&mut self, dt: Duration) -> bool {
        self.held_for = self.held_for.saturating_add(dt);
        self.end.is_none() && self.held_for >= LONG_PRESS
    }
}

impl<P> GridDrag<P> {
    /// Offers one contact event, as the shell reported it, in screen pixels.
    ///
    /// Answers whether the drag is following that contact: `true` for a
    /// finger landing while nothing is held and no other finger is followed,
    /// and for every later event of that contact — until a pressing finger
    /// moves past [`LONG_PRESS_SLOP`], which gives it up and answers `false`,
    /// so the caller can pass it on. A finger lifted before [`LONG_PRESS`] is
    /// given up too: it was a tap. One that has lifted something carries it
    /// until its contact ends, and the frame after that ends the drag — a
    /// drop where it lifted, or a cancel back to where it started.
    pub fn touch(&mut self, contact: ContactId, phase: TouchPhase, at: Vec2) -> bool {
        let carrying = self
            .held
            .as_ref()
            .is_some_and(|held| held.hand == Hand::Touch(contact));
        let followed = self.touch.as_mut().filter(|touch| touch.contact == contact);
        match (phase, followed) {
            (TouchPhase::Began, _) => {
                if self.touch.is_some() || self.held.is_some() {
                    return false;
                }
                self.touch = Some(Touch {
                    contact,
                    at,
                    from: at,
                    held_for: Duration::ZERO,
                    end: None,
                });
            }
            (_, None) => return false,
            (TouchPhase::Moved, Some(touch)) => {
                touch.at = at;
                if !carrying && at.distance(touch.from) > LONG_PRESS_SLOP {
                    self.touch = None;
                    return false;
                }
            }
            (TouchPhase::Ended | TouchPhase::Cancelled, Some(touch)) => {
                touch.at = at;
                if carrying {
                    touch.end = Some(if phase == TouchPhase::Ended {
                        TouchEnd::Lifted
                    } else {
                        TouchEnd::Cancelled
                    });
                } else {
                    self.touch = None;
                }
            }
        }
        true
    }
}
