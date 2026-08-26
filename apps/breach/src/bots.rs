//! The practice map's bots: a capsule on an authored patrol, a ray to say
//! whether it can see the player, and a trigger on a timer.
//!
//! ```text
//!   Route waypoints ──▶ walk_toward ──▶ CharacterController::move_and_slide
//!                                                    │
//!   player eye ──▶ Ray ──▶ PhysicsWorld::cast_ray ──▶ has_line_of_sight
//!                                                    │
//!                              alerted_until ────────┴──▶ fire, on a cadence
//! ```
//!
//! # Deliberately dumb, and deliberately not a navmesh
//!
//! A bot here does four things: walk its list of waypoints, notice the player,
//! shoot, and lose interest. It does not choose where to go, take cover, flank,
//! or know that another bot exists. `docs/plan/24-navigation.md` is a post-MVP
//! subsystem and its own text names `arena`'s bots as its forcing function —
//! **not breach's** — so there is no path query, no poly mesh and no steering
//! here at all. What there is instead is [`crate::map::practice::ROUTES`], which
//! a level author wrote down.
//!
//! # Everything a bot moves through is the player's own controller
//!
//! [`Bot::patrol`] asks [`CharacterController::move_and_slide`] for a
//! displacement, exactly as [`crate::game`] does for the player, against the
//! same [`PhysicsWorld`]. A bot walks into a crate and slides along it for the
//! same reason the player does, and `crcbl-phys` gained nothing for either.
//!
//! # The sighting ray is cast from the player's end, and that is not arbitrary
//!
//! Occlusion along a segment is symmetric, and one of the two ends is a much
//! better place to start from: **the player carries no collider of their own**
//! (their pistol casts from inside where their body would be, so giving them one
//! would make every shot hit themselves), while a bot's body is a capsule in the
//! world that its own eye sits inside. A ray leaving a bot's eye would strike
//! the inside of that capsule 30 cm later and report the bot as blind.
//!
//! So [`has_line_of_sight`] casts **player eye → bot eye** and asks whether the
//! first thing the ray meets is that bot. Cover answers no; another bot standing
//! in the way answers no; nothing in the way answers yes.
//! `a_bot_behind_the_pillar_cannot_see_the_spawn` is what holds that to the map.
//!
//! # A shot is resolved along the same segment as the sighting
//!
//! [`Bot::wants_to_shoot`] fires while the bot is still *interested* —
//! [`INTEREST_S`] after it last saw anything — and [`crate::game`] resolves the
//! shot with the same line of sight. A bot that has lost the player keeps
//! shooting at where they were, and those rounds go into whatever is now in the
//! way. That is the whole of the difference between the two counters on the
//! `[HUD]` line: `fired` is trigger pulls and `taken` is the ones that arrived,
//! and cover is the only thing that separates them.

use crcbl::math::DVec3;
use crcbl::phys::{
    Capsule, CharacterConfig, CharacterController, ColliderId, MoveOutcome, PhysicsWorld, Ray,
};

use crate::camera::EYE_HEIGHT;
use crate::map::practice::{BOTS, BotView, ROUTES, Route};

/// How fast a bot walks, in metres a second.
///
/// Slower than the player's [`crate::game::WALK_SPEED`], so a practice bot is
/// something a shooter can lead rather than chase.
pub const BOT_SPEED: f64 = 2.2;

/// How near a waypoint counts as having reached it, in metres.
///
/// Comfortably over one tick's travel at [`BOT_SPEED`], so a bot cannot step
/// across the acceptance radius in a single move and orbit the point for ever.
pub const WAYPOINT_M: f64 = 0.35;

/// How far a bot can notice the player, in metres.
///
/// Shorter than the arena's diagonal, so it is a real limit rather than
/// decoration, and longer than the distance from the spawn to any waypoint, so
/// what decides a sighting on this map is **cover** and not range.
/// `the_notice_range_is_a_limit_but_not_the_one_the_map_turns_on` asserts both.
pub const NOTICE_M: f64 = 22.0;

/// How long a bot goes on shooting at a player it can no longer see, in seconds.
pub const INTEREST_S: f64 = 2.5;

/// How long between a bot's shots, in seconds.
///
/// Slow, and slower than it needs to be to demonstrate anything: three bots on
/// a one-second cadence take a standing player from full health to nothing in
/// six seconds, which is a practice map nobody gets to practise on.
pub const SHOT_PERIOD_S: f64 = 1.5;

/// How much of the player's health one bot's round takes.
///
/// [`HEALTH_MAX`] divided by this is how many rounds a player can stand in the
/// open for — see [`SHOT_PERIOD_S`] for why that number is not two.
pub const BOT_DAMAGE: u32 = 7;

/// What the player starts with, and what a respawn gives back.
pub const HEALTH_MAX: u32 = 100;

/// How long a bot the player has shot stays down, in seconds.
///
/// Long enough to read as a kill from across the room, short enough that a
/// player working the map finds the patrol back on its feet rather than an empty
/// arena.
pub const BOT_RESPAWN_S: f64 = 6.0;

/// Gravity, as [`crate::game::GRAVITY`] means it — the same integrator the
/// player falls under, because a bot standing on the floor is a bot the ground
/// probe found.
const GRAVITY: f64 = crate::game::GRAVITY;

/// One practice bot.
#[derive(Debug)]
pub struct Bot {
    /// The patrol it was authored with.
    route: &'static Route,
    /// The capsule it walks as, which is the player's controller with the
    /// player's own [`CharacterConfig`].
    controller: CharacterController,
    /// Its body in the world — what the player's pistol hits and what stops
    /// another bot's line of sight.
    body: ColliderId,
    /// Which waypoint it is walking towards.
    leg: usize,
    /// How fast it is falling, in metres a second, negative downward.
    fall_speed: f64,
    /// Which way it is walking, in [`crate::camera::forward`]'s measure — kept
    /// so a bot that has arrived at a waypoint still faces somewhere.
    facing: f64,
    /// Whether it is on its feet.
    alive: bool,
    /// When it comes back, in [`crate::game`]'s elapsed seconds. Only read while
    /// it is down.
    revive_at: f64,
    /// How long it goes on treating the player as a target, in the same seconds.
    /// Zero for a bot that has never seen them.
    alerted_until: f64,
    /// When its next round is due, in the same seconds.
    next_shot_at: f64,
}

/// Where a bot's **eye** is, given where its feet are.
///
/// [`EYE_HEIGHT`], because a bot is the same body the player is: the sighting
/// segment runs eye to eye, and a bot that saw from its navel would be seen over
/// cover it was hiding behind.
#[must_use]
pub fn eye_of(feet: DVec3) -> DVec3 {
    DVec3::new(feet.x, feet.y + EYE_HEIGHT, feet.z)
}

/// Whether `bot` is close enough to the player's `eye` to notice them at all.
///
/// Separate from [`has_line_of_sight`] rather than folded into it, because the
/// two answers mean different things on the readout: a bot that is in range and
/// cannot see the player is the **control** for one that can, and a bot that is
/// simply too far away is neither.
#[must_use]
pub fn is_within_notice(eye: DVec3, bot: &Bot) -> bool {
    bot.alive && (eye_of(bot.feet()) - eye).length_squared() <= NOTICE_M * NOTICE_M
}

/// Whether `eye` and `bot` can see each other.
///
/// Cast from `eye` — the **player's** — for the reason the module docs give: the
/// player carries no collider and a bot's eye sits inside its own. The answer is
/// "the first thing the segment meets is this bot", so a wall, a crate or
/// another bot in the way is a no.
///
/// The ray's direction is the segment itself and its bounds are `0..=1`, so `t`
/// is measured in whole segments and nothing beyond the bot can answer.
#[must_use]
pub fn has_line_of_sight(world: &mut PhysicsWorld, eye: DVec3, bot: &Bot) -> bool {
    if !is_within_notice(eye, bot) {
        return false;
    }
    let ray = Ray::new(eye, eye_of(bot.feet()) - eye).with_bounds(0.0, 1.0);
    world.cast_ray(&ray).is_some_and(|(hit, _)| hit == bot.body)
}

impl Bot {
    /// Puts a bot on the first waypoint of `route`, with its body in `world`.
    ///
    /// The body is registered as the controller's own
    /// [`self_collider`](CharacterController::with_self_collider), so every move
    /// writes the capsule back into the world and no sweep of its own finds it.
    /// Without that a bot would collide with the copy of itself it left behind.
    #[must_use]
    pub fn spawn(world: &mut PhysicsWorld, route: &'static Route) -> Self {
        let config = CharacterConfig::default();
        let feet = route.waypoints[0];
        let centre = feet + DVec3::Y * (config.radius + config.half_height);
        let body = world.add_capsule(Capsule::new(centre, config.radius, config.half_height));
        Self {
            route,
            controller: CharacterController::new(config, centre).with_self_collider(body),
            body,
            leg: 1 % route.waypoints.len(),
            fall_speed: 0.0,
            facing: 0.0,
            alive: true,
            revive_at: 0.0,
            alerted_until: 0.0,
            next_shot_at: SHOT_PERIOD_S,
        }
    }

    /// What this bot is called.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        self.route.label
    }

    /// Its body, for [`crate::game`] to match a ray's answer against.
    #[must_use]
    pub const fn body(&self) -> ColliderId {
        self.body
    }

    /// Whether it is on its feet.
    #[must_use]
    pub const fn is_alive(&self) -> bool {
        self.alive
    }

    /// Where its feet are, in metres.
    #[must_use]
    pub fn feet(&self) -> DVec3 {
        let config = self.controller.config();
        let centre = self.controller.position();
        DVec3::new(
            centre.x,
            centre.y - (config.radius + config.half_height),
            centre.z,
        )
    }

    /// Whether it is still treating the player as a target at `now`.
    #[must_use]
    pub fn is_alerted(&self, now: f64) -> bool {
        self.alive && now < self.alerted_until
    }

    /// What the frame draws of it.
    #[must_use]
    pub fn view(&self, now: f64) -> BotView {
        BotView {
            feet: self.feet(),
            facing: self.facing as f32,
            alive: self.alive,
            alerted: self.is_alerted(now),
        }
    }

    /// Takes it off its feet until `now + BOT_RESPAWN_S`.
    ///
    /// The body becomes a **trigger** rather than being removed: a trigger is
    /// non-solid, so `cast_ray` and every sweep skip it — which is what makes a
    /// downed bot something a shot goes through and a living one something it
    /// stops at — and the collider keeps its id, so nothing else here has to
    /// learn a new one when it gets up.
    pub fn down(&mut self, world: &mut PhysicsWorld, now: f64) {
        self.alive = false;
        self.revive_at = now + BOT_RESPAWN_S;
        self.alerted_until = 0.0;
        // The return says only whether the id resolved, and it is this bot's
        // own: a `false` here would be a collider removed behind its back,
        // which nothing in this sample does.
        world.set_trigger(self.body, true);
    }

    /// Puts it back on the first waypoint of its route, solid again.
    fn revive(&mut self, world: &mut PhysicsWorld, now: f64) {
        let config = *self.controller.config();
        let centre = self.route.waypoints[0] + DVec3::Y * (config.radius + config.half_height);
        self.controller.set_position(centre);
        world.set_capsule(self.body, self.controller.capsule());
        world.set_trigger(self.body, false);
        self.alive = true;
        self.fall_speed = 0.0;
        self.leg = 1 % self.route.waypoints.len();
        self.next_shot_at = now + SHOT_PERIOD_S;
    }

    /// Walks one tick of the patrol, and comes back if it is time to.
    ///
    /// Every metre goes through [`CharacterController::move_and_slide`] against
    /// the world the player sweeps — see the module docs.
    pub fn patrol(&mut self, world: &mut PhysicsWorld, now: f64, dt: f64) -> MoveOutcome {
        if !self.alive {
            if now >= self.revive_at {
                self.revive(world, now);
            }
            return MoveOutcome::default();
        }

        let feet = self.feet();
        let flat_to = |target: DVec3| DVec3::new(target.x - feet.x, 0.0, target.z - feet.z);
        // Arrived is decided *before* the step, and the next leg is what this
        // tick walks: turning a tick late leaves the bot overshooting every
        // corner by one move, which reads as a patrol that wanders.
        if flat_to(self.route.waypoints[self.leg]).length() <= WAYPOINT_M {
            self.leg = (self.leg + 1) % self.route.waypoints.len();
        }
        let direction = flat_to(self.route.waypoints[self.leg]).normalize_or_zero();
        if direction != DVec3::ZERO {
            // The bearing a `forward(yaw, 0)` would produce, so the capsule is
            // drawn facing the way it is walking in the same measure the camera
            // and the pistol use.
            self.facing = direction.x.atan2(-direction.z);
        }

        self.fall_speed += GRAVITY * dt;
        let motion = direction * BOT_SPEED * dt + DVec3::Y * self.fall_speed * dt;
        let outcome = self.controller.move_and_slide(world, motion);
        if outcome.grounded {
            self.fall_speed = 0.0;
        } else if outcome.hit_ceiling {
            self.fall_speed = self.fall_speed.min(0.0);
        }
        outcome
    }

    /// Records that it can see the player at `now`.
    pub fn notice(&mut self, now: f64) {
        self.alerted_until = now + INTEREST_S;
    }

    /// Whether its trigger comes down on the tick that ends at `now`, and arms
    /// the next one if it does.
    ///
    /// One round per [`SHOT_PERIOD_S`] while it is interested, and **not** a
    /// question about whether it can see anything: a bot that has just lost the
    /// player keeps shooting at where they were. What becomes of the round is
    /// [`crate::game`]'s to resolve, along the same segment as the sighting.
    pub fn wants_to_shoot(&mut self, now: f64) -> bool {
        if !self.is_alerted(now) || now < self.next_shot_at {
            return false;
        }
        self.next_shot_at = now + SHOT_PERIOD_S;
        true
    }
}

/// Every bot on the practice map, spawned onto its own route.
#[must_use]
pub fn spawn_all(world: &mut PhysicsWorld) -> Vec<Bot> {
    ROUTES
        .iter()
        .map(|route| Bot::spawn(world, route))
        .collect()
}

/// The views a frame draws, one per route.
///
/// A fixed-size array rather than the `Vec` the stage holds, because
/// [`crate::game::RenderState`] is a `Copy` snapshot taken under the tick's lock
/// and a heap allocation there would be one per draw.
///
/// # Panics
///
/// If `bots` is shorter than [`BOTS`]. Only ever called with what [`spawn_all`]
/// produced, which is one bot per route.
#[must_use]
pub fn views(bots: &[Bot], now: f64) -> [BotView; BOTS] {
    core::array::from_fn(|index| bots[index].view(now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::practice::{self, COVER, PILLAR};

    /// The spawn's eye, which every sighting on this map is cast from.
    fn spawn_eye() -> DVec3 {
        eye_of(practice::SPAWN)
    }

    /// **A bot standing behind the pillar cannot see the spawn, and the same bot
    /// a few metres to one side can.** The positive and its control, made
    /// against the map's own geometry: a build that noticed unconditionally
    /// passes the first half and fails the second, and one that never noticed
    /// fails the first.
    ///
    /// Driven by moving the bot's *body* rather than by walking it, so what is
    /// being asked about is the ray and not the patrol.
    #[test]
    fn a_bot_behind_the_pillar_cannot_see_the_spawn() {
        let mut world = practice::world();
        let mut bots = spawn_all(&mut world);
        let bot = &mut bots[0];
        let config = *bot.controller.config();
        let lift = DVec3::Y * (config.radius + config.half_height);

        let pillar = COVER[PILLAR];
        let hidden = DVec3::new(pillar.x, 0.0, pillar.z - 4.0);
        bot.controller.set_position(hidden + lift);
        world.set_capsule(bot.body, bot.controller.capsule());
        assert!(
            !has_line_of_sight(&mut world, spawn_eye(), bot),
            "the pillar did not stop the sighting",
        );

        // Off to the left rather than the right: the far crate is on the right
        // at this depth, and a control that stood the bot inside a second block
        // of cover would fail for the wrong reason.
        let shown = DVec3::new(pillar.x - 6.0, 0.0, pillar.z - 4.0);
        bot.controller.set_position(shown + lift);
        world.set_capsule(bot.body, bot.controller.capsule());
        assert!(
            has_line_of_sight(&mut world, spawn_eye(), bot),
            "a bot in the open was not seen either, so the check above proves nothing",
        );
    }

    /// **A bot the player has shot is neither seen nor solid**, which is what
    /// makes a kill a kill rather than a colour change: the body stops stopping
    /// rays, so a bot behind it becomes visible.
    #[test]
    fn a_downed_bot_stops_being_something_a_ray_can_find() {
        let mut world = practice::world();
        let mut bots = spawn_all(&mut world);
        let config = *bots[0].controller.config();
        let lift = DVec3::Y * (config.radius + config.half_height);

        // Two bots on the same bearing from the spawn, the near one in front.
        let near = DVec3::new(0.0, 0.0, 2.0);
        let far = DVec3::new(0.0, 0.0, 0.5);
        for (bot, feet) in bots.iter_mut().take(2).zip([near, far]) {
            bot.controller.set_position(feet + lift);
            world.set_capsule(bot.body, bot.controller.capsule());
        }
        assert!(has_line_of_sight(&mut world, spawn_eye(), &bots[0]));
        assert!(
            !has_line_of_sight(&mut world, spawn_eye(), &bots[1]),
            "the near bot is not standing in front of the far one, so this proves nothing",
        );

        bots[0].down(&mut world, 0.0);
        assert!(
            !has_line_of_sight(&mut world, spawn_eye(), &bots[0]),
            "a downed bot is still being sighted",
        );
        assert!(
            has_line_of_sight(&mut world, spawn_eye(), &bots[1]),
            "a downed bot is still stopping the ray behind it",
        );
    }

    /// **A bot walks its own route and comes back round to where it started.**
    /// The patrol is a cycle, so a bot left alone for long enough returns —
    /// which is what says the waypoint list is being advanced rather than a
    /// bot walking into the first corner and stopping.
    #[test]
    fn a_bot_walks_its_route_and_comes_back_round() {
        let mut world = practice::world();
        let mut bots = spawn_all(&mut world);
        let bot = &mut bots[0];
        let dt = 1.0 / 60.0;
        let start = bot.feet();

        let mut furthest = 0.0f64;
        let mut returned = false;
        let mut visited = vec![bot.leg];
        // Long enough for the whole circuit at `BOT_SPEED` with room to spare.
        for tick in 0..(60 * 40) {
            bot.patrol(&mut world, f64::from(tick) * dt, dt);
            furthest = furthest.max((bot.feet() - start).length());
            if !visited.contains(&bot.leg) {
                visited.push(bot.leg);
            }
            if furthest > 4.0 && (bot.feet() - start).length() < WAYPOINT_M {
                returned = true;
                break;
            }
        }
        assert!(
            furthest > 4.0,
            "it only got {furthest:.2} m from where it started",
        );
        assert_eq!(
            visited.len(),
            ROUTES[0].waypoints.len(),
            "it walked {} of {} legs",
            visited.len(),
            ROUTES[0].waypoints.len(),
        );
        assert!(returned, "it never came back round to its first waypoint");
        assert!(
            bot.controller.is_grounded(),
            "it left the floor on the way round",
        );
    }

    /// **The trigger is a cadence, not a tick rate**, and it only comes down
    /// while the bot is interested.
    #[test]
    fn a_bot_fires_on_its_cadence_and_only_while_it_is_interested() {
        let mut world = practice::world();
        let mut bots = spawn_all(&mut world);
        let bot = &mut bots[0];
        let dt = 1.0 / 60.0;

        let mut fired = 0;
        for tick in 0..(60 * 10) {
            let now = f64::from(tick) * dt;
            // Noticed for the first two seconds and never again.
            if now < 2.0 {
                bot.notice(now);
            }
            if bot.wants_to_shoot(now) {
                fired += 1;
            }
        }
        // Interested until 2 s + `INTEREST_S`, at one round per period.
        let window = 2.0 + INTEREST_S;
        let expected = (window / SHOT_PERIOD_S).floor() as usize;
        assert!(
            (fired as i64 - expected as i64).abs() <= 1,
            "it fired {fired} rounds over {window:.1} s at one per {SHOT_PERIOD_S} s",
        );
        assert!(
            !bot.is_alerted(9.0),
            "it was still interested seven seconds after the last sighting",
        );
    }

    /// **The notice range is a real limit and not the thing this map turns on.**
    /// Shorter than the room's diagonal, so a bot in the far corner genuinely
    /// cannot see a player in the near one; longer than the distance from the
    /// spawn to every waypoint, so what decides a sighting on the patrol is
    /// cover.
    #[test]
    fn the_notice_range_is_a_limit_but_not_the_one_the_map_turns_on() {
        let diagonal = (practice::DEPTH.powi(2) + (2.0 * practice::HALF_WIDTH).powi(2)).sqrt();
        assert!(
            NOTICE_M < diagonal,
            "a notice range of {NOTICE_M} m covers the whole {diagonal:.1} m room",
        );
        for route in ROUTES {
            for point in route.waypoints {
                let away = (*point - practice::SPAWN).length();
                assert!(
                    away < NOTICE_M,
                    "{} stands {away:.1} m from the spawn, past the {NOTICE_M} m it can be \
                     noticed at — so a sighting there would fail on range rather than on cover",
                    route.label,
                );
            }
        }
    }
}
