//! Which of the shadow atlas's maps a frame redraws, out of the ones whose
//! contents have gone out of date.
//!
//! `docs/plan/45-shadows.md`'s cadence rung. [`super::Selection`] decides which
//! lights hold tiles and how large each tile is; [`crate::forward`] decides
//! which of those maps the image no longer holds. What is left is the question
//! this module answers: **a frame that would have to redraw everything redraws
//! some of it now and the rest on a later frame**, so a scene where every light
//! moves at once cannot spike a frame with a whole geometry pass per map.
//!
//! # A group, not a tile
//!
//! The unit is what the renderer draws together: a cascade, or a light slot's
//! whole run of tiles. A point light's [`super::POINT_FACES`] faces cull once
//! against the light's sphere and draw that one visible set through six
//! matrices, so redrawing three of them and holding three is not a saving —
//! it is the same cull and the same six visible sets with half the draws. There
//! are [`GROUPS`] of them, and [`Group::faces`] is what each costs in tiles.
//!
//! # Keyed to the frame index, and to nothing else
//!
//! [`Cadence::schedule`] is a pure function of the frame index, the groups that
//! want redrawing and the two limits. No clock, no iteration order and no
//! address is in it, so a golden blessed at a stated frame is the same golden on
//! every run, every driver and every backend — which is the property the plan
//! asks for by name and the one this whole rung would be worthless without.
//!
//! # The ladder is the one the tile size already walks
//!
//! A group's [`Group::tier`] is the quadtree level `super::tile_level` gave
//! its light — or, for a cascade, which cascade it is. There is deliberately no
//! second measure of how much a light matters: `super::coverage` decides the
//! ranking and the tile size, and the tier is read off the answer it already
//! gave. A map worth a whole root cell is redrawn every frame, one worth a
//! quarter of a cell every second frame, and each halving below that doubles the
//! period again.
//!
//! # What it costs, and why it ships off
//!
//! A held map is a map drawn for where its light and the camera *were*. The
//! shadow lags by up to its period, which on a still or slowly-moving light is
//! invisible and on a fast one is a shadow trailing its caster. That is a
//! quality decision per tier rather than a fact about the engine, so
//! [`r_shadow_cadence`] and [`r_shadow_faces`] both default to what ships
//! today — every map that needs redrawing is redrawn, in the frame it went out
//! of date — and a frame nobody has touched the console on is the frame every
//! golden was blessed at.

use super::{CASCADES, LIGHT_SLOTS, TILE_LEVELS, TILES};

/// Groups the shadow atlas is drawn in: one per cascade and one per light slot.
///
/// [`super::CASCADES`] then [`super::LIGHT_SLOTS`], which is the order
/// `crate::forward`'s shadow culls are indexed in — a group *is* a cull, and the
/// two would have to agree even if this were written twice.
pub const GROUPS: usize = CASCADES + LIGHT_SLOTS;

/// The coarsest tier any group can be on, which is the ladder's own length.
///
/// A light's tier is its quadtree level and a cascade's is its index, so this is
/// the larger of the two ladders — and `Cadence::period` clamps to it so a
/// tier arriving from outside cannot shift a period past what a `u32` holds.
const TIERS: usize = if TILE_LEVELS > CASCADES {
    TILE_LEVELS
} else {
    CASCADES
};

crcbl_console::convar! {
    /// Frames a shadow map may be held for before it is redrawn: 1 ships.
    pub static r_shadow_cadence: i64 in 1 ..= 8 = 1;
}

crcbl_console::convar! {
    /// Shadow maps a frame may redraw: the whole atlas ships.
    pub static r_shadow_faces: i64 in 1 ..= 16 = 16;
}

const _: () = assert!(
    TILES == 16,
    "r_shadow_faces' range is the atlas's tile count spelled out, because a \
     console variable's bounds are part of its declaration and cannot be an \
     expression; an atlas of a different size needs that literal moved with it"
);

/// One group of the atlas that wants redrawing, and what redrawing it costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    /// Tiles the group draws: one for a cascade, and
    /// [`super::tile_span`] of its light for a light slot.
    ///
    /// What [`r_shadow_faces`] is spent in, and the reason the budget is a count
    /// of *faces* rather than of groups: a point light's cube is
    /// [`super::POINT_FACES`] whole geometry passes and a spot's cone is one.
    pub faces: usize,
    /// Which rung of the ladder this group sits on — 0 is redrawn every frame,
    /// and each rung below doubles the period.
    ///
    /// A light's is the quadtree level [`super::Assignment::level`] holds, so
    /// `super::coverage` decides it exactly as it decides the tile's size. A
    /// cascade's is its own index: the near cascade fills most of the screen and
    /// is fitted to the eye every frame, and the far ones cover ground a texel
    /// of which is metres across.
    pub tier: usize,
    /// Whether the map the image holds for this group describes a region that
    /// has since moved out from under it, rather than merely being out of date.
    ///
    /// **The reset the plan asks for, per group.** A light that jumped further
    /// than its own radius, or a cascade whose eye moved further than the
    /// cascade's own sphere reaches, holds texels about somewhere else — which
    /// is worse than a shadow that lags, and is what makes this bypass the
    /// period. It does **not** bypass [`Cadence::faces`]: a frame that redrew
    /// past its budget would have no budget. Forced groups are offered it
    /// first, and one that still does not fit holds another frame.
    pub forced: bool,
}

/// The two limits a frame's redraws are spent under.
///
/// Read off the console with [`Cadence::from_console`] once a frame, so the
/// value cannot move between the decision and the pass that acts on it — which
/// is the same freeze `crate::forward` gives its render effects and for the same
/// reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    /// The longest any group may go unredrawn, in frames — [`r_shadow_cadence`].
    ///
    /// One is the whole ladder switched off: every period clamps to one, every
    /// group that wants redrawing is due, and the schedule is what shipped
    /// before this module existed.
    pub hold: u32,
    /// Tiles a frame may redraw — [`r_shadow_faces`].
    pub faces: usize,
}

impl Cadence {
    /// Every group that wants redrawing gets it, this frame.
    ///
    /// What the console's defaults resolve to, and what `crate::forward` passes
    /// on a frame that has reset — see [`Cadence::schedule`]'s note on a map
    /// whose own region has moved.
    pub const EVERY_FRAME: Self = Self {
        hold: 1,
        faces: TILES,
    };

    /// The pair as the console holds them.
    ///
    /// Clamped through the variables' own ranges rather than trusted, on
    /// `crate::ssao::slice_count`'s terms: the range is the console's guard and
    /// this is the one that holds if a range ever moves.
    #[must_use]
    pub fn from_console() -> Self {
        let hold = r_shadow_cadence.get_i64().clamp(1, 8);
        let faces = r_shadow_faces.get_i64().clamp(1, TILES as i64);
        Self {
            hold: u32::try_from(hold).unwrap_or(1),
            faces: usize::try_from(faces).unwrap_or(TILES),
        }
    }

    /// How many frames apart two redraws of a group on `tier` are.
    ///
    /// The ladder doubles per rung and [`Cadence::hold`] caps it, so a hold of
    /// one is every group every frame and a hold of four lets the two coarsest
    /// rungs share a period.
    fn period(self, tier: usize) -> u32 {
        let tier = tier.min(TIERS - 1);
        let rung = 1u32 << tier;
        rung.min(self.hold.max(1))
    }

    /// Which of `wanted`'s groups this frame redraws.
    ///
    /// `wanted[group]` is [`Some`] where the image no longer holds what the
    /// frame would draw into that group, and [`None`] where it does — a free
    /// slot, a frame with shadows off, or a map whose inputs have not moved
    /// since it was drawn. So this decides *when* an out-of-date map is
    /// redrawn and never whether an up-to-date one is.
    ///
    /// # Two gates, in this order
    ///
    /// * **The cadence.** A group is due on the frames where
    ///   `(frame + group) % period` is zero. The group index is in the modulus
    ///   so two groups on one tier fall on different frames rather than
    ///   arriving together — which is what makes the budget below reachable
    ///   rather than a cliff every period.
    /// * **The budget.** The due groups are admitted until [`Cadence::faces`]
    ///   tiles are spoken for. A group that does not fit is skipped and the walk
    ///   continues, so a cascade behind a point light's cube is still redrawn —
    ///   the same rule [`super::Selection::update`] follows when a run of tiles
    ///   will not fit, and for the same reason. A frame that admitted **nothing**
    ///   takes the first candidate anyway: a budget smaller than one map is not
    ///   a budget, it is a map nothing ever draws.
    ///
    /// # The tier decides how often a map asks, not who is served
    ///
    /// The budget is spent in the group index **rotated by the frame** and in
    /// nothing else — deliberately not in tier order, which would starve every
    /// coarse map outright: a tier-0 group is due on every frame, so a budget
    /// that served tiers in order would be spent before a tier-1 group was ever
    /// reached, for ever. Rotating instead serves the due groups round-robin,
    /// and the ladder still comes out of it — a group due twice as often asks
    /// twice as often and is served twice as often.
    ///
    /// # A group that is stale wins nothing over one that is due
    ///
    /// Cadence is the outer gate and staleness the inner one: a group whose
    /// inputs moved but whose turn has not come **holds the texels it has** and
    /// is redrawn on its turn. The alternative — staleness overriding the
    /// cadence — is a schedule that binds only while nothing moves, which is
    /// the case that never needed bounding.
    ///
    /// What that leaves is a map drawn for a region that has since moved out
    /// from under it, which is worse than a lagging shadow — and that is
    /// [`Group::forced`], which bypasses the period and takes the budget first.
    /// A frame whose whole atlas was relaid has nothing to hold at all, and
    /// `crate::forward` passes [`Cadence::EVERY_FRAME`] there rather than
    /// making it a case here.
    #[must_use]
    pub fn schedule(self, frame: u64, wanted: &[Option<Group>; GROUPS]) -> [bool; GROUPS] {
        // Which groups are due, and in what order they are offered the budget.
        // An array rather than a `Vec`: the count is bounded by the atlas's own
        // groups and this runs every frame.
        let mut due = [(0usize, 0usize, 0usize); GROUPS];
        let mut count = 0;
        for (group, want) in wanted.iter().enumerate() {
            let Some(want) = want else {
                continue;
            };
            let period = u64::from(self.period(want.tier));
            if !want.forced && !(frame + group as u64).is_multiple_of(period) {
                continue;
            }
            // Forced first, then round-robin — so a map about somewhere else is
            // redrawn before one that is merely a frame old, and no group holds
            // a fixed place in the order.
            let rotated = (group + (frame % GROUPS as u64) as usize) % GROUPS;
            due[count] = (usize::from(!want.forced), rotated, group);
            count += 1;
        }
        due[..count].sort_unstable();

        let mut redraw = [false; GROUPS];
        let mut spent = 0;
        for &(_, _, group) in &due[..count] {
            let faces = wanted[group].map_or(0, |want| want.faces);
            if spent + faces > self.faces {
                continue;
            }
            spent += faces;
            redraw[group] = true;
        }
        // A budget smaller than every map that is asking would draw none of
        // them, on this frame and on every frame after. The first in the order
        // goes through instead, so a frame always makes progress and the
        // overshoot is one map.
        if spent == 0
            && let Some(&(_, _, group)) = due[..count].first()
        {
            redraw[group] = true;
        }
        redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shadow::POINT_FACES;

    /// `wanted` with every group asking for one tile on `tier`.
    fn all_on(tier: usize) -> [Option<Group>; GROUPS] {
        [Some(Group {
            faces: 1,
            tier,
            forced: false,
        }); GROUPS]
    }

    /// **The shipped defaults redraw everything that asks**, which is what makes
    /// every golden in the tree the frame it was blessed from: the cadence is a
    /// feature a console turns on, not a change to the frame.
    #[test]
    fn the_shipped_limits_redraw_every_group_that_wants_it() {
        // The **declared** defaults rather than the live values: a console
        // variable is process-global, so a test holding one would otherwise
        // decide what this one is about.
        let cadence = Cadence {
            hold: match r_shadow_cadence.default() {
                crcbl_console::Value::Int(hold) => u32::try_from(*hold).expect("a small count"),
                other => panic!("`r_shadow_cadence` is declared as {other:?}"),
            },
            faces: match r_shadow_faces.default() {
                crcbl_console::Value::Int(faces) => usize::try_from(*faces).expect("a small count"),
                other => panic!("`r_shadow_faces` is declared as {other:?}"),
            },
        };
        assert_eq!(
            cadence,
            Cadence::EVERY_FRAME,
            "the console's defaults are the schedule that shipped"
        );
        for frame in 0..8 {
            for tier in 0..TIERS {
                assert_eq!(
                    cadence.schedule(frame, &all_on(tier)),
                    [true; GROUPS],
                    "frame {frame}, tier {tier}"
                );
            }
        }
    }

    /// **The console reaches the schedule**, which is the half
    /// [`the_shipped_limits_redraw_every_group_that_wants_it`] cannot see:
    /// reading the declarations proves what shipped and nothing about the
    /// variables being wired to anything.
    ///
    /// [`crate::forward::ForwardRenderer::set_shadow_cadence`] is what every
    /// test that draws a frame uses instead, and this is why it exists: these
    /// variables are process-global and the shadow pass is in every frame's
    /// pass list, so a test that moved them while drawing would change the frame
    /// every other test in this crate draws. Nothing is drawn here.
    #[test]
    fn the_console_moves_both_limits() {
        let _guard = CONSOLE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        r_shadow_cadence
            .set(&crcbl_console::Value::Int(4))
            .expect("`r_shadow_cadence` is a writable int in range");
        r_shadow_faces
            .set(&crcbl_console::Value::Int(3))
            .expect("`r_shadow_faces` is a writable int in range");
        let moved = Cadence::from_console();
        r_shadow_cadence
            .set(r_shadow_cadence.default())
            .expect("back to what it was declared holding");
        r_shadow_faces
            .set(r_shadow_faces.default())
            .expect("back to what it was declared holding");
        assert_eq!(moved, Cadence { hold: 4, faces: 3 });
        assert_eq!(
            Cadence::from_console(),
            Cadence::EVERY_FRAME,
            "and the restore above put the process back where it found it"
        );
    }

    /// [`the_console_moves_both_limits`]' lock, so the cadence's own tests do
    /// not read a variable another of them is holding.
    static CONSOLE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A group that wants nothing is never redrawn, whatever the frame — the
    /// cadence decides *when* an out-of-date map is redrawn and never whether an
    /// up-to-date one is.
    #[test]
    fn a_group_that_wants_nothing_is_never_redrawn() {
        let cadence = Cadence {
            hold: 8,
            faces: TILES,
        };
        let mut wanted = all_on(0);
        wanted[2] = None;
        for frame in 0..16 {
            assert!(
                !cadence.schedule(frame, &wanted)[2],
                "frame {frame} redrew a group that holds what it should"
            );
        }
    }

    /// **The ladder is a doubling**, and the tier is what walks it: a tier-1
    /// group is redrawn on half the frames a tier-0 one is, a tier-2 group on a
    /// quarter of them.
    #[test]
    fn each_rung_of_the_ladder_halves_how_often_a_group_is_redrawn() {
        let cadence = Cadence {
            hold: 8,
            faces: TILES,
        };
        const FRAMES: u64 = 64;
        let counts: Vec<usize> = (0..TIERS)
            .map(|tier| {
                (0..FRAMES)
                    .filter(|frame| cadence.schedule(*frame, &all_on(tier))[0])
                    .count()
            })
            .collect();
        for (tier, drawn) in counts.iter().enumerate() {
            let expected = FRAMES as usize >> tier;
            assert_eq!(
                *drawn, expected,
                "tier {tier} was redrawn {drawn} times in {FRAMES} frames"
            );
        }
    }

    /// **[`Cadence::hold`] caps the ladder**, so the coarsest tiers share a
    /// period rather than disappearing for eight frames at a time.
    #[test]
    fn the_hold_caps_the_period_the_ladder_asks_for() {
        let cadence = Cadence {
            hold: 2,
            faces: TILES,
        };
        for tier in 1..TIERS {
            assert_eq!(cadence.period(tier), 2, "tier {tier}");
        }
        assert_eq!(cadence.period(0), 1, "the top of the ladder is every frame");
    }

    /// **The budget is spent in tiles**, so a point light's cube costs six of it
    /// and a cascade one.
    #[test]
    fn a_frame_never_redraws_more_tiles_than_the_budget() {
        let cadence = Cadence { hold: 1, faces: 7 };
        let mut wanted = [None; GROUPS];
        wanted[0] = Some(Group {
            faces: 1,
            tier: 0,
            forced: false,
        });
        wanted[1] = Some(Group {
            faces: 1,
            tier: 0,
            forced: false,
        });
        wanted[2] = Some(Group {
            faces: 6,
            tier: 0,
            forced: false,
        });
        wanted[3] = Some(Group {
            faces: 6,
            tier: 0,
            forced: false,
        });
        for frame in 0..8 {
            let redraw = cadence.schedule(frame, &wanted);
            let spent: usize = redraw
                .iter()
                .enumerate()
                .filter(|(_, drawn)| **drawn)
                .filter_map(|(group, _)| wanted[group])
                .map(|want| want.faces)
                .sum();
            assert!(spent <= 7, "frame {frame} redrew {spent} tiles");
        }
    }

    /// **A group the budget refuses is not the end of the walk**: a cascade
    /// behind a cube that will not fit is still redrawn, which is
    /// `super::super::Selection::update`'s rule at this boundary.
    #[test]
    fn a_group_the_budget_refuses_does_not_take_the_ones_behind_it_with_it() {
        let cadence = Cadence { hold: 1, faces: 2 };
        let mut wanted = [None; GROUPS];
        wanted[0] = Some(Group {
            faces: 6,
            tier: 0,
            forced: false,
        });
        wanted[1] = Some(Group {
            faces: 1,
            tier: 0,
            forced: false,
        });
        wanted[2] = Some(Group {
            faces: 1,
            tier: 0,
            forced: false,
        });
        let redraw = cadence.schedule(0, &wanted);
        assert!(!redraw[0], "six tiles do not fit in a budget of two");
        assert!(
            redraw[1] && redraw[2],
            "the two that do fit were refused with it: {redraw:?}"
        );
    }

    /// **A budget too small for any map still draws one.** A frame that admitted
    /// nothing would admit nothing on the next frame too, and the map would be a
    /// tile the atlas never redraws — which is not a budget, it is a light that
    /// is silently unshadowed for ever.
    #[test]
    fn a_budget_smaller_than_every_map_still_draws_one_of_them() {
        let cadence = Cadence { hold: 1, faces: 1 };
        let mut wanted = [None; GROUPS];
        wanted[0] = Some(Group {
            faces: POINT_FACES,
            tier: 0,
            forced: false,
        });
        wanted[3] = Some(Group {
            faces: POINT_FACES,
            tier: 0,
            forced: false,
        });
        for frame in 0..8 {
            let redraw = cadence.schedule(frame, &wanted);
            assert_eq!(
                redraw.iter().filter(|drawn| **drawn).count(),
                1,
                "frame {frame} drew {redraw:?} against a budget no cube fits in"
            );
        }
    }

    /// **Nothing starves while the budget binds.** Every group that keeps asking
    /// is redrawn inside a bounded run of frames, which is what the rotation in
    /// the tie-break buys: without it the same groups would win every frame and
    /// the rest would never be drawn at all.
    #[test]
    fn every_group_that_keeps_asking_is_redrawn_within_a_bounded_run() {
        let cadence = Cadence { hold: 1, faces: 2 };
        let wanted = all_on(0);
        let mut last = [None; GROUPS];
        for frame in 0..64u64 {
            for (group, drawn) in cadence.schedule(frame, &wanted).iter().enumerate() {
                if *drawn {
                    last[group] = Some(frame);
                }
            }
        }
        for (group, seen) in last.iter().enumerate() {
            let seen = seen.unwrap_or_else(|| panic!("group {group} was never redrawn"));
            assert!(
                seen + u64::try_from(GROUPS).expect("a handful of groups") >= 63,
                "group {group} was last redrawn at frame {seen} of 63"
            );
        }
    }

    /// **A forced group bypasses the period**, which is the reset: a map about
    /// somewhere else is redrawn on the frame that notices rather than on the
    /// frame its turn comes round.
    #[test]
    fn a_forced_group_is_due_on_every_frame_however_coarse_its_tier() {
        let cadence = Cadence {
            hold: 8,
            faces: TILES,
        };
        let mut wanted = [None; GROUPS];
        wanted[0] = Some(Group {
            faces: 1,
            tier: TIERS - 1,
            forced: true,
        });
        for frame in 0..16 {
            assert!(
                cadence.schedule(frame, &wanted)[0],
                "frame {frame} held a map that is about somewhere else"
            );
        }
    }

    /// **A forced group is offered the budget first**, so the reset does not
    /// queue behind a map that is merely a frame out of date.
    #[test]
    fn a_forced_group_takes_the_budget_before_one_that_is_only_stale() {
        let cadence = Cadence { hold: 1, faces: 1 };
        let mut wanted = [None; GROUPS];
        wanted[0] = Some(Group {
            faces: 1,
            tier: 0,
            forced: false,
        });
        wanted[5] = Some(Group {
            faces: 1,
            tier: 0,
            forced: true,
        });
        let redraw = cadence.schedule(0, &wanted);
        assert!(
            redraw[5],
            "the forced group was refused the only tile spare"
        );
        assert!(!redraw[0], "the budget of one was spent twice: {redraw:?}");
    }

    /// **The schedule is a function of the frame index and nothing else**, which
    /// is what makes a golden at a stated frame the same golden on every run and
    /// every driver.
    #[test]
    fn one_frame_index_gives_one_answer() {
        let cadence = Cadence { hold: 4, faces: 3 };
        let wanted = all_on(1);
        for frame in 0..32 {
            let first = cadence.schedule(frame, &wanted);
            for _ in 0..4 {
                assert_eq!(cadence.schedule(frame, &wanted), first, "frame {frame}");
            }
        }
    }
}
