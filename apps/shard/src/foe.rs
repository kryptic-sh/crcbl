//! The zone's foes: three archetypes that hold a post until they notice the
//! character, and one ability each.
//!
//! ```text
//!   character centre ──▶ Ray ──▶ PhysicsWorld::cast_ray ──▶ can_see
//!                                                    │
//!                                    alerted_until ──┤
//!                                                    ▼
//!                    Kind::approach(gap) ──▶ CharacterController::move_and_slide
//!                                                    │
//!                                Kind::period ───────┴──▶ strikes ──▶ damage
//! ```
//!
//! # Sentries, not patrols, and the difference is the point
//!
//! `apps/breach/src/bots.rs` is the neighbouring problem and most of it is
//! reused here — a capsule on the player's own controller, a ray for the
//! sighting, a cooldown for the trigger. What is deliberately **not** reused is
//! the patrol: a breach bot walks an authored route whatever the player does,
//! and a foe here stands on its post until it notices the character. That is
//! the archetype an interior zone wants, and it is also what makes "it reacted"
//! something a check can tell from "it was always like that" — a foe that was
//! already walking about has no such moment.
//!
//! `docs/plan/24-navigation.md` is still a post-MVP subsystem whose forcing
//! function is `arena`, so there is no path query and no steering here either.
//! An engaged foe walks **straight at** what it is engaged with and slides
//! along whatever it meets, which is
//! [`CharacterController::move_and_slide`] doing it rather than the foe.
//!
//! # Three archetypes, and each reads differently from across the room
//!
//! | | [`Kind::Husk`] | [`Kind::Adept`] | [`Kind::Warden`] |
//! | --- | --- | --- | --- |
//! | walks at | [`HUSK_SPEED`] | [`ADEPT_SPEED`] | [`WARDEN_SPEED`] |
//! | wants to be | in your face | [`ADEPT_NEAR`]–[`ADEPT_FAR`] m out | in your face |
//! | ability | a fast jab | a bolt down the sighting line | a slam with a wind-up |
//! | takes | one blow | two | five |
//!
//! **The adept is the one that gives ground.** [`Kind::approach`] is the whole
//! of the difference in movement: a husk and a warden close and stop, and an
//! adept walks *backwards* when the character gets inside its band. **The
//! warden is the one that telegraphs.** It stands still for
//! [`WARDEN_WINDUP_S`] before its slam lands, so a player who moves out of
//! [`Kind::reach`] in that window is missed — and [`FoeView::winding`] is what
//! puts the wind-up in the picture rather than only in the numbers.
//!
//! # The sighting ray is cast from the character's end, and that is not arbitrary
//!
//! The same argument `apps/breach/src/bots.rs` makes, and it holds here for the
//! same reason: **the character carries no collider of their own**, while a
//! foe's body is a capsule in the world that its own centre sits inside. A ray
//! leaving a foe's centre would strike the inside of that capsule and report the
//! foe as blind.
//!
//! So [`can_see`] casts **character centre → foe centre** and asks whether the
//! first thing the ray meets is that foe. Stone answers no; another foe standing
//! in the way answers no; nothing in the way answers yes.
//! `a_foe_behind_the_doorway_cannot_see_the_spawn` holds that to the zone's own
//! geometry.
//!
//! Centre to centre rather than eye to eye, because both ends are capsules and a
//! capsule's centre is the point furthest from its own surface — there is no
//! head on either body to sight from.
//!
//! # The character's own blow is the same query at a shorter range
//!
//! [`can_see`] takes its range from the caller, so [`crate::game`]'s cleave asks
//! it the same question over [`STRIKE_REACH_M`] that a foe asks over
//! [`NOTICE_M`]: *is there a clear line between these two bodies*. One function,
//! because it is one fact — a pillar between the two is what stops a sighting
//! and a swing alike.
//!
//! # A dead foe stays dead
//!
//! There is no respawn timer, unlike breach's practice bots: this is a zone
//! being cleared rather than a range being worked, and a cleared zone that
//! quietly refills is a zone a player cannot finish. [`Foe::fell`] turns the
//! body into a **trigger** — non-solid, so `cast_ray` and every sweep pass
//! through it — which is what makes a corpse something a later swing goes
//! through and a standing foe something it stops at.

use crcbl::math::DVec3;
use crcbl::phys::{Capsule, CharacterConfig, CharacterController, ColliderId, PhysicsWorld, Ray};

use crate::game::GRAVITY;
use crate::zone;

// ---------------------------------------------------------------------------
// The numbers
// ---------------------------------------------------------------------------

/// How many foes the zone holds.
///
/// Three, and a fixed number rather than a flag: `docs/plan/sample/15-shard.md`
/// caps milestone 1 at "a handful of enemy archetypes and abilities", and one
/// of each archetype is the smallest cast that shows all three behaviours.
pub const FOES: usize = 3;

/// How far a foe can notice the character, in metres.
///
/// **Shorter than the corridor is long, deliberately.** The zone's corridor runs
/// some fifteen metres from the spawn to the shrine doorway, so a foe posted at
/// the far end of it is out of range from the spawn and comes into range when
/// the character walks at it — which is what makes an engagement something that
/// *happens* rather than the state the zone opens in.
/// `no_foe_can_reach_the_character_where_the_zone_opens` holds that to the
/// layout rather than to this sentence.
pub const NOTICE_M: f64 = 9.0;

/// How long a foe goes on treating the character as a target after it last saw
/// them, in seconds.
///
/// The same interest window `apps/breach/src/bots.rs` gives its bots, and for
/// the same reason: a foe that forgot the moment a pillar crossed the line
/// would be a foe you could shake by stepping sideways.
pub const INTEREST_S: f64 = 2.5;

/// How fast a [`Kind::Husk`] walks, in metres a second.
///
/// Under [`crate::game::WALK_SPEED`], so a husk is something the character can
/// walk away from — and far enough under it that walking away is a decision
/// rather than a sprint.
pub const HUSK_SPEED: f64 = 3.2;
/// How fast a [`Kind::Adept`] walks, in metres a second. Slow, because it does
/// not need to arrive anywhere.
pub const ADEPT_SPEED: f64 = 2.0;
/// How fast a [`Kind::Warden`] walks, in metres a second. The slowest thing in
/// the zone, which is the other half of what makes it survivable.
pub const WARDEN_SPEED: f64 = 1.4;

/// The nearest an engaged [`Kind::Adept`] will let the character get before it
/// walks backwards, in metres.
pub const ADEPT_NEAR: f64 = 5.0;
/// …and the furthest it will let them get before it closes again.
///
/// Inside [`NOTICE_M`], so an adept holding its band is an adept that can still
/// see what it is shooting at.
pub const ADEPT_FAR: f64 = 8.0;

/// How long a [`Kind::Warden`] stands still before its slam lands, in seconds.
///
/// The telegraph: it is what a player reads to step out of
/// [`Kind::reach`] in time, and [`FoeView::winding`] is what puts it in the
/// picture.
pub const WARDEN_WINDUP_S: f64 = 0.9;

/// What the character starts with, and what a return to the spawn gives back.
pub const HEALTH_MAX: u32 = 100;

/// How far the character's cleave reaches, in metres.
///
/// Past a husk's and a warden's own stand-off, so both can be answered — and
/// nowhere near [`ADEPT_FAR`], which is what makes an adept something you have
/// to corner rather than something you trade with.
pub const STRIKE_REACH_M: f64 = 2.2;

/// How much of a foe's health one blow takes.
///
/// [`Kind::health`] divided by this is how many blows each archetype is worth,
/// and the table in this module's docs is the reading.
pub const STRIKE_DAMAGE: u32 = 20;

/// How long between two blows, in seconds.
pub const STRIKE_PERIOD_S: f64 = 0.45;

/// How far inside its own [`Kind::reach`] a closing foe stops, in metres.
///
/// A margin rather than nothing, so a foe that arrives is a foe that can strike
/// on the next tick instead of one straddling the boundary and drifting in and
/// out of it. It is also what puts a warden inside the character's own
/// [`STRIKE_REACH_M`] despite out-reaching them — see
/// `every_foe_that_can_reach_the_character_can_be_reached_back`.
const STAND_OFF_MARGIN_M: f64 = 0.6;

// ---------------------------------------------------------------------------
// The archetypes
// ---------------------------------------------------------------------------

/// Which archetype a foe is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Closes to arm's length and jabs.
    Husk,
    /// Keeps its distance and throws a bolt down the sighting line.
    Adept,
    /// Slow, heavy, and telegraphs a slam.
    Warden,
}

impl Kind {
    /// What the panel, the `[HUD]` line and the debug readout call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Husk => "husk",
            Self::Adept => "adept",
            Self::Warden => "warden",
        }
    }

    /// The body it walks as.
    ///
    /// A [`CharacterConfig`] per archetype rather than the character's for all
    /// three, because the capsule is also the thing that is **drawn** — see
    /// [`crate::zone`] — so a warden that looked twice a husk's size while
    /// sweeping the same shape would be a picture that lies about the physics.
    /// Everything except the size is the character's own default: the same
    /// slope limit, the same step, the same skin.
    #[must_use]
    pub fn config(self) -> CharacterConfig {
        let (radius, half_height) = match self {
            Self::Husk => (0.28, 0.55),
            Self::Adept => (0.26, 0.60),
            Self::Warden => (0.45, 0.75),
        };
        CharacterConfig {
            radius,
            half_height,
            ..CharacterConfig::default()
        }
    }

    /// How fast it walks, in metres a second.
    #[must_use]
    pub const fn speed(self) -> f64 {
        match self {
            Self::Husk => HUSK_SPEED,
            Self::Adept => ADEPT_SPEED,
            Self::Warden => WARDEN_SPEED,
        }
    }

    /// What it starts with.
    #[must_use]
    pub const fn health(self) -> u32 {
        match self {
            Self::Husk => 20,
            Self::Adept => 40,
            Self::Warden => 100,
        }
    }

    /// How far its ability reaches, in metres.
    ///
    /// An adept's is [`NOTICE_M`] — it throws as far as it can see, which is
    /// the whole of what makes it the archetype you cannot ignore from across
    /// the room. A warden out-reaches a husk, and both are inside
    /// [`STRIKE_REACH_M`], so anything that can hit the character can be hit
    /// back.
    #[must_use]
    pub const fn reach(self) -> f64 {
        match self {
            Self::Husk => 1.8,
            Self::Adept => NOTICE_M,
            Self::Warden => 2.6,
        }
    }

    /// How much of [`HEALTH_MAX`] one of its abilities takes.
    #[must_use]
    pub const fn damage(self) -> u32 {
        match self {
            Self::Husk => 6,
            Self::Adept => 9,
            Self::Warden => 22,
        }
    }

    /// How long between two of them, in seconds.
    #[must_use]
    pub const fn period(self) -> f64 {
        match self {
            Self::Husk => 0.9,
            Self::Adept => 2.0,
            Self::Warden => 3.0,
        }
    }

    /// How long it stands still before the ability lands, in seconds.
    ///
    /// Zero for everything but a [`Kind::Warden`], whose whole readable
    /// difference is that you can see the slam coming.
    #[must_use]
    pub const fn windup(self) -> f64 {
        match self {
            Self::Husk | Self::Adept => 0.0,
            Self::Warden => WARDEN_WINDUP_S,
        }
    }

    /// How close an engaged foe wants to get, in metres.
    ///
    /// Just inside its own [`Kind::reach`], so it stops in striking distance
    /// rather than oscillating across the boundary. `None` for a
    /// [`Kind::Adept`], which holds a *band* rather than a distance —
    /// [`ADEPT_NEAR`] to [`ADEPT_FAR`].
    #[must_use]
    pub const fn stand_off(self) -> Option<f64> {
        match self {
            Self::Husk | Self::Warden => Some(self.reach() - STAND_OFF_MARGIN_M),
            Self::Adept => None,
        }
    }

    /// Which way an engaged foe wants to move, given how far away the character
    /// is: `1` closer, `-1` further, `0` hold.
    ///
    /// **The one function the three archetypes genuinely differ in**, and the
    /// difference a player reads without looking at a number: a husk and a
    /// warden come at you and stop, and an adept walks backwards when you get
    /// inside its band.
    #[must_use]
    pub fn approach(self, gap: f64) -> f64 {
        match self.stand_off() {
            Some(want) => f64::from(u8::from(gap > want)),
            None => {
                if gap > ADEPT_FAR {
                    1.0
                } else if gap < ADEPT_NEAR {
                    -1.0
                } else {
                    0.0
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Where they stand
// ---------------------------------------------------------------------------

/// One authored post: an archetype and the tile it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Post {
    /// Which archetype stands here.
    pub kind: Kind,
    /// Its column on [`zone::LAYOUT`].
    pub col: usize,
    /// Its row on the same.
    pub row: usize,
}

/// The three posts, in the order the frame draws them.
///
/// **Every one of them is out of the frame the zone opens on, and out of
/// [`NOTICE_M`] of the spawn.** That is not decoration: it is what makes the
/// browser gate's "a foe engaged when the character came at it" a claim about
/// the sighting rather than about the state the page loads in, and it is what
/// keeps the still-frame control in that gate's lighting block looking at a
/// zone where nothing is moving.
/// `no_foe_can_reach_the_character_where_the_zone_opens` and
/// `no_foe_is_in_the_frame_the_zone_opens_on` assert both against the layout and
/// the camera, so a post moved without moving them is a failing test rather than
/// a flaky gate.
///
/// The husk holds the shrine doorway, which is the one tile every route into the
/// far hall passes through; the adept and the warden stand behind it in the
/// hall, so clearing the doorway is what puts the character in front of them.
pub const POSTS: [Post; FOES] = [
    Post {
        kind: Kind::Husk,
        col: 6,
        row: 4,
    },
    Post {
        kind: Kind::Adept,
        col: 5,
        row: 2,
    },
    Post {
        kind: Kind::Warden,
        col: 7,
        row: 2,
    },
];

// ---------------------------------------------------------------------------
// A foe
// ---------------------------------------------------------------------------

/// One foe on its post.
#[derive(Debug)]
pub struct Foe {
    kind: Kind,
    /// Where it stands while it has noticed nothing.
    post: DVec3,
    /// The capsule it walks as, which is the character's controller with this
    /// archetype's own [`CharacterConfig`].
    controller: CharacterController,
    /// Its body in the world — what a blow lands on and what stops another
    /// foe's line of sight.
    body: ColliderId,
    /// What it has left. Zero is not a state it is ever in: the blow that would
    /// take it there puts it down instead.
    health: u32,
    /// Whether it is on its feet.
    alive: bool,
    /// How long it goes on treating the character as a target, in
    /// [`crate::game`]'s elapsed seconds. Zero for a foe that has noticed
    /// nothing.
    alerted_until: f64,
    /// When its next ability may begin, in the same seconds.
    next_ability_at: f64,
    /// When a [`Kind::Warden`]'s slam lands, in the same seconds. Only read
    /// while [`Foe::winding`] is set.
    winding_until: f64,
    /// Whether it is standing still with a slam half-thrown.
    winding: bool,
    /// How fast it is falling, in metres a second, negative downward.
    fall_speed: f64,
}

/// Where one foe is drawn, and how.
///
/// The frame's copy of what a [`Foe`] is, snapshotted with the rest of
/// [`crate::game::RenderState`] so a draw never reads through the tick's lock.
///
/// There is no facing on it, and that is the same admission
/// [`crate::zone::Figure`] makes about the character: the body is a capsule, and
/// a capsule has no front to turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoeView {
    /// Which archetype it is — what decides the mesh and the resting colour.
    pub kind: Kind,
    /// Where its **feet** are, in metres.
    pub feet: DVec3,
    /// Whether it is on its feet at all.
    pub alive: bool,
    /// Whether it has noticed the character.
    pub engaged: bool,
    /// Whether it is standing still with a slam half-thrown. Always false for
    /// anything but a [`Kind::Warden`].
    pub winding: bool,
}

impl Default for FoeView {
    /// A fallen husk at the origin.
    ///
    /// What [`crate::game::RenderState::default`] carries, which is the state of
    /// a frame drawn before the first tick has run. `alive: false` rather than
    /// `true` on purpose: a default that stood a foe up would put a body in the
    /// picture that no tick had placed.
    fn default() -> Self {
        Self {
            kind: Kind::Husk,
            feet: DVec3::ZERO,
            alive: false,
            engaged: false,
            winding: false,
        }
    }
}

/// Whether `from` and `foe` have a clear line between them, no further apart
/// than `range`.
///
/// Cast from `from` — the **character's** centre — for the reason the module
/// docs give: they carry no collider and a foe's own centre sits inside its
/// capsule. The answer is "the first thing the segment meets is this foe", so
/// stone, a doorpost or another foe in the way is a no.
///
/// The ray's direction is the segment itself and its bounds are `0..=1`, so `t`
/// is measured in whole segments and nothing beyond the foe can answer.
#[must_use]
pub fn can_see(world: &mut PhysicsWorld, from: DVec3, foe: &Foe, range: f64) -> bool {
    if !foe.alive {
        return false;
    }
    let gap = foe.controller.position() - from;
    if gap.length_squared() > range * range {
        return false;
    }
    let ray = Ray::new(from, gap).with_bounds(0.0, 1.0);
    world.cast_ray(&ray).is_some_and(|(hit, _)| hit == foe.body)
}

impl Foe {
    /// Puts a foe on `post`, with its body in `world`.
    ///
    /// The body is registered as the controller's own
    /// [`self_collider`](CharacterController::with_self_collider), so every move
    /// writes the capsule back into the world and no sweep of its own finds it.
    /// Without that a foe would collide with the copy of itself it left behind.
    #[must_use]
    pub fn stand(world: &mut PhysicsWorld, post: Post) -> Self {
        let config = post.kind.config();
        let feet = zone::tile_centre(post.col, post.row);
        let centre = feet + DVec3::Y * (config.radius + config.half_height);
        let body = world.add_capsule(Capsule::new(centre, config.radius, config.half_height));
        Self {
            kind: post.kind,
            post: feet,
            controller: CharacterController::new(config, centre).with_self_collider(body),
            body,
            health: post.kind.health(),
            alive: true,
            alerted_until: 0.0,
            next_ability_at: 0.0,
            winding_until: 0.0,
            winding: false,
            fall_speed: 0.0,
        }
    }

    /// Which archetype it is.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// What it is called.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        self.kind.label()
    }

    /// Its body, for a ray's answer to be matched against.
    #[must_use]
    pub const fn body(&self) -> ColliderId {
        self.body
    }

    /// Whether it is on its feet.
    #[must_use]
    pub const fn is_alive(&self) -> bool {
        self.alive
    }

    /// What it has left.
    #[must_use]
    pub const fn health(&self) -> u32 {
        self.health
    }

    /// Where it stands while it has noticed nothing, in metres.
    #[must_use]
    pub const fn post(&self) -> DVec3 {
        self.post
    }

    /// Where its capsule's centre is — the end of every segment cast at it.
    #[must_use]
    pub fn centre(&self) -> DVec3 {
        self.controller.position()
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

    /// Whether it is still treating the character as a target at `now`.
    #[must_use]
    pub fn is_engaged(&self, now: f64) -> bool {
        self.alive && now < self.alerted_until
    }

    /// What the frame draws of it.
    #[must_use]
    pub fn view(&self, now: f64) -> FoeView {
        FoeView {
            kind: self.kind,
            feet: self.feet(),
            alive: self.alive,
            engaged: self.is_engaged(now),
            winding: self.winding,
        }
    }

    /// Takes `damage` off it, and reports whether that was the blow that
    /// finished it.
    ///
    /// The body becomes a **trigger** on the way down: a trigger is non-solid,
    /// so [`PhysicsWorld::cast_ray`] and every sweep skip it — which is what
    /// makes a fallen foe something a later swing goes through and a standing
    /// one something it stops at — and the collider keeps its id, so nothing
    /// else here has to learn a new one.
    pub fn wounded(&mut self, world: &mut PhysicsWorld, damage: u32) -> bool {
        if !self.alive {
            return false;
        }
        self.health = self.health.saturating_sub(damage);
        if self.health > 0 {
            return false;
        }
        self.fell(world);
        true
    }

    /// Takes it off its feet for good.
    ///
    /// There is no revive: see the module docs for why a cleared zone does not
    /// refill.
    pub fn fell(&mut self, world: &mut PhysicsWorld) {
        self.alive = false;
        self.alerted_until = 0.0;
        self.winding = false;
        // The return says only whether the id resolved, and it is this foe's
        // own: a `false` here would be a collider removed behind its back,
        // which nothing in this sample does.
        world.set_trigger(self.body, true);
    }

    /// Puts it back into the state a save recorded: `health` left, or down.
    ///
    /// **`health` is a value [`crate::save`] has already validated** against
    /// this archetype's own [`Kind::health`], so nothing is clamped here — a
    /// payload claiming a husk has a warden's health reads as no save at all
    /// rather than as a husk this function quietly corrected.
    ///
    /// Zero is felled, and it goes down through [`Foe::fell`] rather than by
    /// setting the flag: the body has to become a trigger, or a restored zone
    /// would have a corpse still stopping rays and sweeps.
    pub fn restore(&mut self, world: &mut PhysicsWorld, health: u32) {
        debug_assert!(
            health <= self.kind.health(),
            "a {} cannot hold {health} health",
            self.kind.label(),
        );
        self.health = health;
        if health == 0 {
            if self.alive {
                self.fell(world);
            }
            return;
        }
        self.alive = true;
    }

    /// One tick of noticing and moving.
    ///
    /// `target` is the character's capsule centre. Every metre goes through
    /// [`CharacterController::move_and_slide`] against the world the character
    /// sweeps — see the module docs.
    pub fn advance(&mut self, world: &mut PhysicsWorld, target: DVec3, now: f64, dt: f64) {
        if !self.alive {
            return;
        }
        if can_see(world, target, self, NOTICE_M) {
            self.alerted_until = now + INTEREST_S;
        }

        // A foe that has noticed nothing holds its post, and a warden with a
        // slam half-thrown holds still — that stillness *is* the telegraph.
        // Both still fall, because a foe standing on the floor is a foe the
        // ground probe found.
        let engaged = self.is_engaged(now);
        let held = !engaged || self.winding;
        let horizontal = if held {
            DVec3::ZERO
        } else {
            let flat = DVec3::new(target.x - self.centre().x, 0.0, target.z - self.centre().z);
            let gap = flat.length();
            flat.normalize_or_zero() * self.kind.approach(gap) * self.kind.speed() * dt
        };

        self.fall_speed += GRAVITY * dt;
        let motion = horizontal + DVec3::Y * self.fall_speed * dt;
        let outcome = self.controller.move_and_slide(world, motion);
        if outcome.grounded {
            self.fall_speed = 0.0;
        } else if outcome.hit_ceiling {
            self.fall_speed = self.fall_speed.min(0.0);
        }
    }

    /// Whether its ability lands on the tick that ends at `now`, and for how
    /// much.
    ///
    /// Called after [`Foe::advance`], so a foe that stepped into reach this tick
    /// strikes on it rather than on the next one.
    ///
    /// A [`Kind::Warden`] answers `None` on the tick it *begins* its slam and
    /// again on every tick of the wind-up; what it answers on the tick the slam
    /// lands is whether the character is still inside [`Kind::reach`] with a
    /// clear line — so stepping out of the way during the wind-up is a slam that
    /// missed, and the cooldown is spent either way.
    pub fn strikes(&mut self, world: &mut PhysicsWorld, target: DVec3, now: f64) -> Option<u32> {
        if !self.is_engaged(now) {
            return None;
        }
        let in_reach = can_see(world, target, self, self.kind.reach());
        if self.winding {
            if now < self.winding_until {
                return None;
            }
            self.winding = false;
            self.next_ability_at = now + self.kind.period();
            return in_reach.then(|| self.kind.damage());
        }
        if now < self.next_ability_at || !in_reach {
            return None;
        }
        if self.kind.windup() > 0.0 {
            self.winding = true;
            self.winding_until = now + self.kind.windup();
            return None;
        }
        self.next_ability_at = now + self.kind.period();
        Some(self.kind.damage())
    }
}

/// Every foe the zone holds, on its own post.
#[must_use]
pub fn stand_all(world: &mut PhysicsWorld) -> Vec<Foe> {
    POSTS.iter().map(|post| Foe::stand(world, *post)).collect()
}

/// The views a frame draws, one per post.
///
/// A fixed-size array rather than the `Vec` the stage holds, because
/// [`crate::game::RenderState`] is a `Copy` snapshot taken under the tick's lock
/// and a heap allocation there would be one per draw.
///
/// # Panics
///
/// If `foes` is shorter than [`FOES`]. Only ever called with what [`stand_all`]
/// produced, which is one foe per post.
#[must_use]
pub fn views(foes: &[Foe], now: f64) -> [FoeView; FOES] {
    core::array::from_fn(|index| foes[index].view(now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::math::Vec3;

    /// The character's capsule centre where the zone opens.
    fn spawn_centre() -> DVec3 {
        let config = CharacterConfig::default();
        zone::spawn() + DVec3::Y * (config.radius + config.half_height)
    }

    /// How far down `−Z` the browser gate's walk check can leave the character,
    /// in metres.
    ///
    /// That check holds the walk key until one heartbeat reports a metre of
    /// progress and then lets go, so the overshoot is bounded by the beats that
    /// can arrive before the release is seen. Three of them is well past what
    /// has ever been observed, and the two tests below hold the posts out of
    /// range and out of frame for the whole of it.
    #[allow(clippy::cast_precision_loss)]
    fn gate_walk_m() -> f64 {
        let beat = crate::game::HEARTBEAT_TICKS as f64 / f64::from(crate::game::DEFAULT_TICK_HZ);
        3.0 * crate::game::WALK_SPEED * beat
    }

    /// **No foe is inside [`NOTICE_M`] of where the character starts.**
    ///
    /// The property the whole engagement claim rests on: a foe that could
    /// already see the character on the first tick would make "it noticed them
    /// when they came at it" a check with nothing to say. Asserted against the
    /// layout, so moving a post without moving it out of range fails here rather
    /// than in a browser.
    ///
    /// The margin is the second half: the browser gate walks the character a
    /// couple of metres before it puts the torches out and asks for a **still**
    /// frame, so the posts have to be out of range from there too.
    #[test]
    fn no_foe_can_reach_the_character_where_the_zone_opens() {
        let walked = gate_walk_m();
        for post in POSTS {
            let feet = zone::tile_centre(post.col, post.row);
            assert!(
                zone::cell(post.col, post.row).is_open(),
                "{} stands in stone at {},{}",
                post.kind.label(),
                post.col,
                post.row,
            );
            for (label, from) in [
                ("the spawn", zone::spawn()),
                ("the gate's walk", zone::spawn() - DVec3::Z * walked),
            ] {
                let away = (feet - from).length();
                assert!(
                    away > NOTICE_M,
                    "the {} stands {away:.1} m from {label}, inside the {NOTICE_M} m it \
                     notices at",
                    post.kind.label(),
                );
            }
        }
    }

    /// **No foe is in the frame the zone opens on**, which is the other half of
    /// the same requirement: the browser gate's lighting block asks for a canvas
    /// that does not change at all while the torches are out, and a foe standing
    /// in shot would be a body to redraw.
    ///
    /// Asserted through the camera the frame is actually drawn from — the clip
    /// coordinate is out of the frustum **vertically**, which is the one bound
    /// that does not move with the canvas's aspect ratio.
    #[test]
    fn no_foe_is_in_the_frame_the_zone_opens_on() {
        let walked = gate_walk_m();
        for standing in [zone::spawn(), zone::spawn() - DVec3::Z * walked] {
            #[allow(clippy::cast_possible_truncation)]
            let feet = Vec3::new(standing.x as f32, standing.y as f32, standing.z as f32);
            let camera = crate::camera::Iso::default().camera(feet);
            // Any aspect at all: the claim below is about `y`, and a
            // perspective projection's vertical bound is `fov_y` whatever the
            // viewport is.
            let clip = camera.view_projection(16.0 / 9.0);
            for post in POSTS {
                let at = zone::tile_centre(post.col, post.row);
                let config = post.kind.config();
                #[allow(clippy::cast_possible_truncation)]
                let centre = Vec3::new(
                    at.x as f32,
                    (config.radius + config.half_height) as f32,
                    at.z as f32,
                );
                let projected = clip * centre.extend(1.0);
                assert!(
                    projected.y.abs() > projected.w.abs(),
                    "the {} projects to y {:.3} of w {:.3} from {standing:?}, which is inside \
                     the frame",
                    post.kind.label(),
                    projected.y,
                    projected.w,
                );
            }
        }
    }

    /// **A foe on the far side of the shrine doorway cannot see the spawn, and
    /// the same foe in the corridor can.** The positive and its control, made
    /// against the zone's own stonework: a build that noticed unconditionally
    /// passes the first half and fails the second, and one that never noticed
    /// fails the first.
    ///
    /// Driven by moving the foe's *body* rather than by walking it, so what is
    /// being asked about is the ray and not the approach.
    #[test]
    fn a_foe_behind_the_doorway_cannot_see_the_spawn() {
        let mut world = zone::world();
        let mut foes = stand_all(&mut world);
        let foe = &mut foes[0];
        let config = foe.kind.config();
        let lift = DVec3::Y * (config.radius + config.half_height);
        // Far enough that only the stonework can be the answer.
        let range = 100.0;

        // Behind the doorway's own post, one tile off the corridor's centre.
        let hidden = zone::tile_centre(4, 3);
        foe.controller.set_position(hidden + lift);
        world.set_capsule(foe.body, foe.controller.capsule());
        assert!(
            !can_see(&mut world, spawn_centre(), foe, range),
            "the shrine wall did not stop the sighting",
        );

        // …and in the corridor, which is open all the way to the spawn.
        let shown = zone::tile_centre(6, 6);
        foe.controller.set_position(shown + lift);
        world.set_capsule(foe.body, foe.controller.capsule());
        assert!(
            can_see(&mut world, spawn_centre(), foe, range),
            "a foe in the open corridor was not seen either, so the check above proves nothing",
        );
    }

    /// **A foe the character has felled is neither seen nor solid**, which is
    /// what makes a kill a kill rather than a colour change: the body stops
    /// stopping rays, so a foe behind it becomes visible.
    #[test]
    fn a_fallen_foe_stops_being_something_a_ray_can_find() {
        let mut world = zone::world();
        let mut foes = stand_all(&mut world);
        let range = 100.0;

        // Two foes on the same bearing from the spawn, down the corridor, the
        // near one in front.
        for (index, row) in [(0usize, 7usize), (1, 5)] {
            let config = foes[index].kind.config();
            let lift = DVec3::Y * (config.radius + config.half_height);
            foes[index]
                .controller
                .set_position(zone::tile_centre(6, row) + lift);
            let capsule = foes[index].controller.capsule();
            world.set_capsule(foes[index].body, capsule);
        }
        assert!(can_see(&mut world, spawn_centre(), &foes[0], range));
        assert!(
            !can_see(&mut world, spawn_centre(), &foes[1], range),
            "the near foe is not standing in front of the far one, so this proves nothing",
        );

        foes[0].fell(&mut world);
        assert!(
            !can_see(&mut world, spawn_centre(), &foes[0], range),
            "a fallen foe is still being sighted",
        );
        assert!(
            can_see(&mut world, spawn_centre(), &foes[1], range),
            "a fallen foe is still stopping the ray behind it",
        );
    }

    /// **The three archetypes want three different distances**, which is the
    /// difference a player reads without a readout: a husk and a warden close,
    /// and an adept gives ground.
    ///
    /// The adept at close quarters is the control: an `approach` that returned
    /// the same sign for everything would pass the two closers and fail this.
    #[test]
    fn only_the_adept_walks_backwards() {
        for kind in [Kind::Husk, Kind::Warden] {
            assert!(kind.approach(6.0) > 0.0, "{} would not close", kind.label());
            assert_eq!(
                kind.approach(0.5),
                0.0,
                "{} kept walking into the character",
                kind.label(),
            );
        }
        assert!(Kind::Adept.approach(ADEPT_FAR + 1.0) > 0.0);
        assert_eq!(Kind::Adept.approach(0.5 * (ADEPT_NEAR + ADEPT_FAR)), 0.0);
        assert!(
            Kind::Adept.approach(ADEPT_NEAR - 1.0) < 0.0,
            "the adept stood its ground at arm's length",
        );
        // …and its band is one it can see across, or it would back out of its
        // own sightline.
        const { assert!(ADEPT_FAR < NOTICE_M) };
        const { assert!(ADEPT_NEAR < ADEPT_FAR) };
    }

    /// **Every archetype can be answered.** A foe that out-reached the
    /// character's cleave and closed to exactly its own reach would be one you
    /// could never touch, which is a demo with a losing condition and no winning
    /// one.
    #[test]
    fn every_foe_that_can_reach_the_character_can_be_reached_back() {
        for kind in [Kind::Husk, Kind::Warden] {
            let want = kind
                .stand_off()
                .expect("a closer holds a distance rather than a band");
            assert!(
                want < STRIKE_REACH_M,
                "a {} closes to {want} m and strikes from {} m, both outside the \
                 {STRIKE_REACH_M} m the character swings",
                kind.label(),
                kind.reach(),
            );
        }
        // A warden out-reaches the character and still stops inside their swing,
        // which is the pair that makes it dangerous rather than unanswerable.
        assert!(Kind::Warden.reach() > STRIKE_REACH_M);
        // The adept is the exception, and deliberately: it is the archetype you
        // have to corner. Asserted so the exception is a decision rather than an
        // oversight.
        assert_eq!(Kind::Adept.stand_off(), None);
        const { assert!(ADEPT_NEAR > STRIKE_REACH_M) };
    }

    /// How far above a foe's capsule centre the restore check casts from, in
    /// metres. Well clear of the tallest archetype's own capsule.
    const RAY_FROM_ABOVE_M: f64 = 2.0;

    /// **A foe restored as felled is a body a ray goes through**, and one
    /// restored with health left is still a body a ray stops at.
    ///
    /// The pair is the whole of why [`Foe::restore`] goes through [`Foe::fell`]
    /// rather than setting the flag: `alive` alone would make a resumed zone
    /// look right and behave wrong — the character's cleave and every other
    /// foe's sighting line would still be stopped by a corpse. The standing foe
    /// is the control, so "the ray missed" cannot pass for a ray that was
    /// pointed at nothing.
    #[test]
    fn a_foe_restored_as_felled_stops_no_ray_and_a_wounded_one_still_does() {
        let mut world = zone::world();
        let mut foes = stand_all(&mut world);

        for (index, health) in [(0usize, 0u32), (1, 10)] {
            let target = foes[index].centre();
            foes[index].restore(&mut world, health);
            // **Straight down onto the body**, from above the walls and ending
            // at the capsule's own centre. The zone's stonework is what makes
            // a horizontal ray a test of the layout rather than of the body —
            // the adept's post has a doorpost between it and the spawn, which
            // `a_foe_behind_the_doorway_cannot_see_the_spawn` is about — and
            // this sample has no roof, so nothing but the foe is on this line.
            let from = target + DVec3::Y * RAY_FROM_ABOVE_M;
            let ray = Ray::new(from, target - from).with_bounds(0.0, 1.0);
            let hit = world.cast_ray(&ray).map(|(collider, _)| collider);
            let body = foes[index].body();
            if health == 0 {
                assert_ne!(
                    hit,
                    Some(body),
                    "a felled {} still stopped a ray",
                    foes[index].label(),
                );
                assert!(!foes[index].is_alive());
            } else {
                assert_eq!(
                    hit,
                    Some(body),
                    "a wounded {} stopped nothing, so the check above is about \
                     an empty line rather than about a corpse",
                    foes[index].label(),
                );
                assert!(foes[index].is_alive());
                assert_eq!(foes[index].health(), health);
            }
        }
    }

    /// **A foe holds its post until it notices, and then it comes.** The first
    /// half is the control for the second: a foe that walked from the first tick
    /// would pass "it moved" without ever having seen anything.
    #[test]
    fn a_foe_holds_its_post_until_it_sees_the_character() {
        let mut world = zone::world();
        let mut foes = stand_all(&mut world);
        let foe = &mut foes[0];
        let dt = 1.0 / f64::from(crate::game::DEFAULT_TICK_HZ);
        let post = foe.feet();

        // Two seconds with the character standing on the spawn, which is out of
        // range of every post.
        let far = spawn_centre();
        for tick in 0..120 {
            foe.advance(&mut world, far, f64::from(tick) * dt, dt);
        }
        assert!(
            !foe.is_engaged(2.0),
            "it noticed a character it cannot see from {} m away",
            (far - foe.centre()).length(),
        );
        assert!(
            (foe.feet() - post).length() < 0.05,
            "it left its post at {post:?} for {:?} with nothing to chase",
            foe.feet(),
        );

        // …and then the character walks up the corridor to a tile it can see.
        let config = CharacterConfig::default();
        let near = zone::tile_centre(6, 5) + DVec3::Y * (config.radius + config.half_height);
        for tick in 120..(120 + 180) {
            foe.advance(&mut world, near, f64::from(tick) * dt, dt);
        }
        assert!(
            foe.is_engaged(5.0),
            "it never noticed a character in the open"
        );
        let closed = (foe.centre() - near).length();
        assert!(
            closed < 2.0,
            "it noticed the character and stopped {closed:.2} m away",
        );
        assert!(
            foe.controller.is_grounded(),
            "it left the floor on the way over",
        );
    }

    /// **A warden's slam is spent on the wind-up whether or not it lands**,
    /// which is what makes stepping out of the way worth doing.
    ///
    /// The character standing still is the control: the same warden, the same
    /// cadence, and a blow that arrives.
    #[test]
    fn a_warden_that_is_stepped_away_from_slams_nothing() {
        let dt = 1.0 / f64::from(crate::game::DEFAULT_TICK_HZ);
        let landed = |dodge: bool| {
            let mut world = zone::world();
            let mut foes = stand_all(&mut world);
            let warden = foes
                .iter_mut()
                .find(|foe| foe.kind == Kind::Warden)
                .expect("the zone posts a warden");
            let config = CharacterConfig::default();
            let lift = DVec3::Y * (config.radius + config.half_height);
            let close = warden.feet() + DVec3::Z * 1.5 + lift;
            let away = warden.feet() + DVec3::Z * (Kind::Warden.reach() + 3.0) + lift;

            let mut hits = 0;
            let mut winding_seen = false;
            for tick in 0..300 {
                let now = f64::from(tick) * dt;
                // Stand in reach until the wind-up has begun, then leave — or
                // stay, which is the control.
                let target = if dodge && winding_seen { away } else { close };
                warden.advance(&mut world, target, now, dt);
                if warden.winding {
                    winding_seen = true;
                }
                if warden.strikes(&mut world, target, now).is_some() {
                    hits += 1;
                }
            }
            (hits, winding_seen)
        };

        let (stood, telegraphed) = landed(false);
        assert!(telegraphed, "the warden never wound up at all");
        assert!(stood > 0, "a character standing in reach was never slammed");
        let (dodged, _) = landed(true);
        assert_eq!(
            dodged, 0,
            "stepping out of reach during the wind-up was slammed anyway",
        );
    }

    /// **A foe takes blows until it does not**, and the blow that finishes it is
    /// the one that says so.
    #[test]
    fn a_foe_falls_on_the_blow_that_empties_it() {
        let mut world = zone::world();
        let mut foes = stand_all(&mut world);
        let foe = &mut foes[2];
        assert_eq!(foe.kind, Kind::Warden);
        let blows = foe.health().div_ceil(STRIKE_DAMAGE);
        assert!(blows > 1, "a warden that falls to one blow is not a warden");

        for blow in 1..blows {
            assert!(
                !foe.wounded(&mut world, STRIKE_DAMAGE),
                "blow {blow} of {blows} finished it",
            );
            assert!(foe.is_alive());
        }
        assert!(foe.wounded(&mut world, STRIKE_DAMAGE), "it would not fall");
        assert!(!foe.is_alive());
        // …and a blow on a body is not a second kill.
        assert!(!foe.wounded(&mut world, STRIKE_DAMAGE));
    }
}
