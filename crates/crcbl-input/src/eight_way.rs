//! A 2-D axis as the four digital directions a game walking at one speed asks
//! for — [`eight_way`].
//!
//! A game whose wire form carries four direction bits, and whose simulation
//! normalises the direction it reads, has nowhere to put a stick's magnitude.
//! For that game reducing the analog value to eight sectors is the honest
//! reading rather than a loss, and a stick, a d-pad and `WASD` summed into one
//! [`ActionKind::Axis2`](crate::ActionKind::Axis2) all come out as the same
//! four bits. `apps/horde` and `apps/puppet` both read their `move` action
//! through it, which is why it is here rather than in either.
//!
//! Not [`Cardinal`](crate::Cardinal): that is a menu's four-way reading, where
//! a diagonal has to pick one neighbour; a character walks the diagonal.

/// Where one of the eight directions ends and the next begins: `sin(π/8)`, the
/// component a unit vector has at 22.5° off an axis.
///
/// Applied to the **normalised** direction, so the eight sectors are 45° wide
/// each and a diagonal is no harder to hold than a cardinal.
/// `the_eight_sectors_are_the_angle_they_claim` checks this against `f32::sin`,
/// because a transcribed constant is a transcription until something computes
/// it.
pub const EIGHT_WAY_SECTOR: f32 = 0.382_683_43;

/// The four digital directions the deflection `(x, y)` (+Y up) asks for, as
/// `(up, down, left, right)` — a diagonal sets two.
///
/// Two thresholds, and they measure different things: `dead_zone` is about
/// *how far* the deflection reaches and rejects the middle of the stick,
/// [`EIGHT_WAY_SECTOR`] is about *which way* it points and splits the rest into
/// eight equal sectors. Folding them into one would make a stick pushed gently
/// north-east ask for nothing while the same push due north asked for a walk.
///
/// ```
/// use crcbl_input::eight_way;
///
/// assert_eq!(eight_way(0.0, 1.0, 0.25), (true, false, false, false));
/// assert_eq!(eight_way(0.7, 0.7, 0.25), (true, false, false, true));
/// assert_eq!(eight_way(0.1, 0.0, 0.25), (false, false, false, false));
/// ```
#[must_use]
pub fn eight_way(x: f32, y: f32, dead_zone: f32) -> (bool, bool, bool, bool) {
    let length = x.hypot(y);
    if length < dead_zone {
        return (false, false, false, false);
    }
    let (x, y) = (x / length, y / length);
    (
        y >= EIGHT_WAY_SECTOR,
        y <= -EIGHT_WAY_SECTOR,
        x <= -EIGHT_WAY_SECTOR,
        x >= EIGHT_WAY_SECTOR,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dead zone the tests below read against: any value inside the unit
    /// disc does.
    const DEAD_ZONE: f32 = 0.25;

    /// [`EIGHT_WAY_SECTOR`] is the angle it claims to be, computed rather than
    /// eyeballed.
    ///
    /// A transcribed constant is a transcription until something checks it,
    /// and nothing else would notice a digit dropped from the middle of it:
    /// the eight sectors would simply stop being equal.
    #[test]
    fn the_eight_sectors_are_the_angle_they_claim() {
        let want = (std::f32::consts::PI / 8.0).sin();
        assert!(
            (EIGHT_WAY_SECTOR - want).abs() < 1e-6,
            "EIGHT_WAY_SECTOR is {EIGHT_WAY_SECTOR}, sin(π/8) is {want}",
        );

        // Either side of the boundary between "due east" and "north-east", at
        // full deflection. One degree in from each side, so the check is about
        // the split and not about a float landing exactly on it.
        let at = |degrees: f32| {
            let radians = degrees.to_radians();
            eight_way(radians.cos(), radians.sin(), DEAD_ZONE)
        };
        assert_eq!(at(21.5), (false, false, false, true), "east");
        assert_eq!(at(23.5), (true, false, false, true), "north-east");
        assert_eq!(at(66.5), (true, false, false, true), "still north-east");
        assert_eq!(at(68.5), (true, false, false, false), "north");
        assert_eq!(at(180.0), (false, false, true, false), "west");
        assert_eq!(at(-90.0), (false, true, false, false), "south");
    }

    /// A deflection inside the dead zone asks for nothing, and a hair outside
    /// it does — or the dead zone is the whole stick.
    #[test]
    fn a_deflection_inside_the_dead_zone_asks_for_nothing() {
        assert_eq!(eight_way(0.0, 0.0, DEAD_ZONE), (false, false, false, false));
        assert_eq!(
            eight_way(DEAD_ZONE * 0.99, 0.0, DEAD_ZONE),
            (false, false, false, false),
        );
        assert_eq!(
            eight_way(DEAD_ZONE * 1.01, 0.0, DEAD_ZONE),
            (false, false, false, true),
        );
    }
}
