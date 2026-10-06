//! Asking where an upright move would leave a character without making it:
//! [`CharacterController::preview_upright`] and the [`UprightPreview`] it
//! returns.

use glam::DVec3;

use super::{CharacterController, GroundContact, MoveOutcome, SlideContact, WorldReader};
use crate::collider::Capsule;
use crate::world::{OverlapQueries, QueryScratch};

/// Where a [`move_and_slide_into`] would leave a character, and what it would
/// meet on the way, from [`CharacterController::preview_upright`].
///
/// Every field is what the move itself would report or leave behind from the
/// same state, to the bit: the preview runs the move's own solve and only
/// skips writing the result anywhere.
///
/// [`move_and_slide_into`]: CharacterController::move_and_slide_into
#[derive(Debug, Clone, PartialEq)]
pub struct UprightPreview {
    /// What the move would return: the motion it would apply, the
    /// depenetration it would make first, and the walls, ceilings, step and
    /// slides it would meet.
    pub outcome: MoveOutcome,
    /// The capsule where the move would leave it: the controller's
    /// [`capsule`](CharacterController::capsule) after the move, which is
    /// also what the move would write to its
    /// [self collider](CharacterController::with_self_collider).
    pub capsule: Capsule,
    /// The ground the move would leave the character standing on, which the
    /// controller's [`ground`](CharacterController::ground) would then report.
    /// `None` when it would end the move unsupported — off a ledge, in the
    /// air, or asking to go up.
    pub ground: Option<GroundContact>,
    /// Every sweep of the slide that would meet something, in the order it
    /// would meet them: the list
    /// [`move_and_slide_into`](CharacterController::move_and_slide_into)
    /// would write, holding exactly
    /// [`outcome.slides`](MoveOutcome::slides) entries. See [`SlideContact`].
    pub contacts: Vec<SlideContact>,
}

impl CharacterController {
    /// Where [`move_and_slide_into`](Self::move_and_slide_into) by `motion`
    /// would leave this character, **without moving it or anything else**.
    ///
    /// # It touches nothing, and the signature is what says so
    ///
    /// The controller is borrowed shared, so its position, its ground and
    /// every other field are what they were. The world is reached only
    /// through an [`OverlapQueries`] view, which cannot write it: the
    /// controller's [self collider](Self::with_self_collider) stays where it
    /// is, as does every other collider, and the broadphase is neither
    /// refitted nor rebuilt — building the view is what brings a stale tree
    /// up to date, as the first query after any change would. Calling this
    /// any number of times changes no later query or move. `scratch` is only the buffers the view's queries
    /// work in; its contents mean nothing between calls and never change an
    /// answer.
    ///
    /// # It is the move's own solve
    ///
    /// The move is depenetration, the slide with its step-up, and the ground
    /// probe, then a write to the self collider. This runs the first three on
    /// a copy of the controller and skips the write, so it answers with the
    /// same self-exclusion, query mask, skin width, step and ground rules —
    /// the same code, not a second account of it. A move made afterwards from
    /// the same state, against the same world, returns this preview's
    /// [`outcome`](UprightPreview::outcome) and contacts exactly.
    ///
    /// # Previewing another shape
    ///
    /// The capsule previewed is this controller's own. To ask about a
    /// different one — a contact capsule a game grows by a margin of its
    /// choosing — preview from a controller built with that shape, bound to
    /// the same collider and mask:
    ///
    /// ```
    /// use crcbl_phys::{
    ///     BoxCollider, CharacterConfig, CharacterController, PhysicsWorld, QueryScratch,
    /// };
    /// use glam::DVec3;
    ///
    /// let mut world = PhysicsWorld::new();
    /// world.add_box(BoxCollider::new(DVec3::new(0.0, -1.0, 0.0), DVec3::new(50.0, 1.0, 50.0)));
    /// let config = CharacterConfig::default();
    /// let body = CharacterController::new(config, DVec3::new(0.0, 2.0, 0.0));
    /// let live = world.add_capsule(body.capsule());
    /// let body = body.with_self_collider(live);
    ///
    /// let grown = CharacterConfig { radius: config.radius + config.skin_width, ..config };
    /// let probe = CharacterController::new(grown, body.position())
    ///     .with_query_mask(body.query_mask())
    ///     .with_self_collider(live);
    /// let mut scratch = QueryScratch::new();
    /// let preview = probe.preview_upright(world.overlap_queries(), &mut scratch, DVec3::new(0.0, -1.5, 0.0));
    ///
    /// assert!(preview.outcome.grounded);
    /// assert_eq!(preview.capsule.radius, grown.radius);
    /// assert_eq!(world.aabb_of(live), Some(body.capsule().aabb()));
    /// ```
    ///
    /// A controller built with [`CharacterController::new`] starts
    /// unsupported, as every new controller does, which suits a preview of
    /// motion through the air. A preview from the live controller itself
    /// starts from the ground it stands on.
    #[must_use]
    pub fn preview_upright(
        &self,
        world: OverlapQueries<'_>,
        scratch: &mut QueryScratch,
        motion: DVec3,
    ) -> UprightPreview {
        // Every field but the depenetration buffer, which holds nothing
        // between moves and is not worth copying.
        let mut solver = Self {
            config: self.config,
            position: self.position,
            ground: self.ground,
            self_collider: self.self_collider,
            query_mask: self.query_mask,
            contacts: Vec::new(),
        };
        let mut contacts = Vec::new();
        let outcome = solver.solve_upright(
            &mut WorldReader {
                view: world,
                scratch,
            },
            motion,
            Some(&mut contacts),
        );
        UprightPreview {
            outcome,
            capsule: solver.capsule(),
            ground: solver.ground,
            contacts,
        }
    }
}
