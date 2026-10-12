//! Where a corpse could stand up: the clearance half of a revival handoff.
//!
//! [`Ragdoll::rise_point`] answers one question — if this body rose here,
//! where would a standing character's capsule be, and does it fit — and
//! moves nothing to answer it: no body, no collider and no character. Whether
//! to rise, the get-up animation and the moment the authoritative character
//! is placed at the answer are the game's (`docs/plan/35-ragdolls.md` keeps
//! powered get-up out of scope).
//!
//! The point is chosen under the body nearest the skeleton root — the hips of
//! a humanoid, which a get-up pivots about — and found by the character
//! controller's own solve ([`CharacterController::preview_upright`]): a
//! standing capsule with its feet at the hips' height, pushed out of whatever
//! it starts inside and dropped to the ground. A push is a relocation, so it
//! is reported and capped ([`RiseQuery::max_shift`]) rather than taken
//! silently: a corpse against a wall rises a hand's width out from it, and one
//! wedged under a vehicle does not rise metres away.

use crcbl_phys::{CharacterConfig, CharacterController, PhysicsSystem, QueryFilter, QueryScratch};
use glam::DVec3;

use crate::Ragdoll;

/// What a rise is checked against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiseQuery {
    /// The standing character: its capsule, and the slope and step it can
    /// stand on.
    pub character: CharacterConfig,
    /// The query layers the standing capsule meets. **Leave the corpse's own
    /// layers out** ([`Ragdoll::set_query_layers`]): its bodies lie where the
    /// character would stand, and on a mask that sees them the character is
    /// stood on top of its own corpse.
    pub query_mask: u32,
    /// The farthest, in metres across the ground, the standing capsule may
    /// end from the hips: how far a rise may relocate the body.
    pub max_shift: f64,
}

/// Why a corpse cannot rise where it lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RiseError {
    /// A body of the ragdoll is no longer in the system.
    BodyGone,
    /// No ground the character can stand on lies within a standing height
    /// below the hips.
    NoGround,
    /// Standing up would move the character `shift` metres across the
    /// ground, past [`RiseQuery::max_shift`].
    Crowded {
        /// How far the solve pushed the capsule from the hips.
        shift: f64,
    },
    /// Where the solve left the standing capsule, it still overlaps
    /// something: a ceiling too low to stand under.
    Blocked,
}

impl core::fmt::Display for RiseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BodyGone => write!(f, "a body of the ragdoll is gone"),
            Self::NoGround => write!(f, "there is no ground to stand on below the body"),
            Self::Crowded { shift } => {
                write!(f, "standing would move the body {shift} m, past the cap")
            }
            Self::Blocked => write!(f, "a standing character does not fit where the body lies"),
        }
    }
}

impl std::error::Error for RiseError {}

impl Ragdoll {
    /// Where a character standing up from this corpse would stand: the
    /// centre of its capsule, as [`CharacterController::new`] takes a
    /// position — see the module docs for how it is chosen. Nothing in
    /// `physics` moves; `&mut` is only what building its query tree takes.
    ///
    /// # Errors
    ///
    /// [`RiseError`]: a body gone, no ground, a push past the cap, or no room
    /// to stand.
    pub fn rise_point(
        &self,
        physics: &mut PhysicsSystem,
        query: &RiseQuery,
    ) -> Result<DVec3, RiseError> {
        let root = (0..self.entities.len())
            .min_by_key(|&body| self.skeleton_joints[body])
            .ok_or(RiseError::BodyGone)?;
        let hips = physics
            .transform(self.entities[root])
            .ok_or(RiseError::BodyGone)?
            .position;
        let config = query.character;
        let standing = config.radius + config.half_height;
        let probe = CharacterController::new(config, hips + DVec3::Y * standing)
            .with_query_mask(query.query_mask);
        let mut scratch = QueryScratch::new();
        let preview = probe.preview_upright(
            physics.world_mut().overlap_queries(),
            &mut scratch,
            DVec3::NEG_Y * (2.0 * standing),
        );
        if !preview.outcome.grounded {
            return Err(RiseError::NoGround);
        }
        let centre = preview.capsule.centre;
        let shift = (centre - hips).with_y(0.0).length();
        if shift > query.max_shift {
            return Err(RiseError::Crowded { shift });
        }
        let mut overlaps = Vec::new();
        physics
            .world_mut()
            .overlap_queries()
            .capsule_penetrations_filtered_into(
                &preview.capsule,
                QueryFilter::masked(query.query_mask),
                &mut scratch,
                &mut overlaps,
            );
        if overlaps
            .iter()
            .any(|(_, penetration)| penetration.depth > config.skin_width)
        {
            return Err(RiseError::Blocked);
        }
        Ok(centre)
    }
}
