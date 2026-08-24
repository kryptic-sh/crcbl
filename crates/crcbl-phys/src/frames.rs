//! A hierarchy of reference frames, and the sphere-of-influence crossings that
//! move a body between them.
//!
//! `docs/plan/05-physics.md`: "bodies parent to dominant gravity source (galaxy
//! → star → planet → moon → vehicle). Simulation runs in the local frame; the
//! frame itself moves on-rails. Frame transitions (sphere-of-influence
//! crossing) are explicit events."
//!
//! # Why a hierarchy at all
//!
//! Gravity is an n-body problem and nothing here pretends otherwise; what a
//! hierarchy buys is that **one** body dominates at any given place, so the
//! motion is a two-body problem plus small perturbations. That is the patched
//! conic approximation, and it is why a spacecraft's trajectory can be
//! propagated analytically for a thousand orbits instead of integrated: inside
//! a sphere of influence there is one attractor, and at its boundary the body
//! is handed to the next frame out or in.
//!
//! # Why the positions are [`WorldPos`]
//!
//! A frame conversion is an addition, and at these distances an addition in
//! plain `f64` is where a seam comes from: at Jupiter's orbit an `f64` metre
//! coordinate has about 128 m between representable values, so a body would
//! visibly snap as it crossed. [`WorldPos`] carries the large part as an
//! integer sector index and the small part as an offset below `2^20` m, where
//! the spacing is a fraction of a nanometre — so the crossing is exact and
//! [`Frames::convert`] round-trips bit for bit. That is what
//! `06-orbit.md`'s "no visible seam or jitter" asks for, and
//! `a_crossing_far_from_the_origin_keeps_millimetres` is what measures it: a
//! millimetre held 10^15 m from the root, which a plain `f64` metre coordinate
//! cannot represent at all.
//!
//! # What drove it
//!
//! `docs/plan/sample/06-orbit.md` milestone 3 — the moon transfer, which is an
//! SOI transition and a sector crossing at once.

use crcbl_core::WorldPos;
use glam::{DVec3, I64Vec3};

/// A frame's handle in a [`Frames`] hierarchy.
///
/// A plain index rather than the generational id [`crate::ColliderId`] is,
/// because frames are not removed: a hierarchy is the star, planets and moons
/// of a scene, built once. With no recycling there is no slot to confuse, and
/// an id from one [`Frames`] used against another is the only misuse left —
/// which panics on the bounds check rather than reading a neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameId(u32);

impl FrameId {
    /// The index this id names, for subscripting a caller's own per-frame
    /// array.
    #[inline]
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A position and velocity, read in some frame.
///
/// Which frame is not carried here: a state is only ever handled together with
/// the [`FrameId`] it belongs to, and pairing them in one struct would invite
/// [`Frames::convert`] to be called with a frame the state is not actually in
/// and quietly agree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct State {
    /// Position relative to the frame's origin.
    pub position: WorldPos,
    /// Velocity relative to the frame, in m/s.
    pub velocity: DVec3,
}

impl State {
    /// A state at a frame's origin, at rest in it.
    pub const AT_ORIGIN: Self = Self {
        position: WorldPos::ORIGIN,
        velocity: DVec3::ZERO,
    };

    /// Builds a state.
    #[inline]
    #[must_use]
    pub fn new(position: WorldPos, velocity: DVec3) -> Self {
        Self { position, velocity }
    }
}

/// One frame of the hierarchy.
#[derive(Debug)]
struct Frame {
    parent: Option<FrameId>,
    children: Vec<FrameId>,
    depth: u32,
    mu: f64,
    soi_radius: f64,
    /// Where this frame's origin sits in its parent's frame.
    origin: WorldPos,
    /// How fast this frame moves through its parent's, in m/s.
    velocity: DVec3,
}

/// A tree of reference frames: a root, and bodies parented to whatever
/// dominates them.
///
/// Build it once with [`new`](Self::new) and [`add`](Self::add), move frames
/// with [`set_state`](Self::set_state) as they orbit, read a body's state in
/// another frame with [`convert`](Self::convert), and ask
/// [`transition`](Self::transition) whether a body has left the frame it was
/// being simulated in.
///
/// Nothing here integrates or propagates anything. A frame's motion is set by
/// its caller — today by whatever is stepping the scene, later by the on-rails
/// Kepler propagator, which changes nothing about the conversions.
#[derive(Debug)]
pub struct Frames {
    frames: Vec<Frame>,
}

impl Frames {
    /// Builds a hierarchy with a single root frame of gravitational parameter
    /// `mu` (`GM`, in m³/s²).
    ///
    /// The root has no parent and an infinite sphere of influence: nothing is
    /// outside it, which is what makes [`transition`](Self::transition)
    /// terminate.
    #[must_use]
    pub fn new(mu: f64) -> Self {
        Self {
            frames: vec![Frame {
                parent: None,
                children: Vec::new(),
                depth: 0,
                mu,
                soi_radius: f64::INFINITY,
                origin: WorldPos::ORIGIN,
                velocity: DVec3::ZERO,
            }],
        }
    }

    /// The root frame's id, which is always the first one added.
    #[inline]
    #[must_use]
    pub fn root(&self) -> FrameId {
        FrameId(0)
    }

    /// How many frames the hierarchy holds, the root included.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Always false — a hierarchy always has its root. Present because clippy
    /// asks for it wherever `len` is, and answering honestly is cheaper than
    /// explaining the exception.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Adds a frame under `parent`, at `origin` in the parent's frame and
    /// moving at `velocity` through it.
    ///
    /// `mu` is the body's gravitational parameter `GM` in m³/s², and
    /// `soi_radius` the radius in metres beyond which the parent dominates —
    /// [`sphere_of_influence`] computes the usual one.
    ///
    /// # Panics
    ///
    /// If `soi_radius` is not positive. A frame nothing can be inside cannot
    /// be transitioned into and would sit in the tree as a body that silently
    /// exerts no influence.
    pub fn add(
        &mut self,
        parent: FrameId,
        mu: f64,
        soi_radius: f64,
        origin: WorldPos,
        velocity: DVec3,
    ) -> FrameId {
        assert!(
            soi_radius > 0.0,
            "a frame's sphere of influence must have a positive radius, got {soi_radius}"
        );
        let id = FrameId(u32::try_from(self.frames.len()).expect("frame count fits in u32"));
        let depth = self.frames[parent.index()].depth + 1;
        self.frames.push(Frame {
            parent: Some(parent),
            children: Vec::new(),
            depth,
            mu,
            soi_radius,
            origin: origin.normalize(),
            velocity,
        });
        self.frames[parent.index()].children.push(id);
        id
    }

    /// This frame's parent, or `None` for the root.
    #[inline]
    #[must_use]
    pub fn parent(&self, id: FrameId) -> Option<FrameId> {
        self.frames[id.index()].parent
    }

    /// The frames parented directly to `id`.
    #[inline]
    #[must_use]
    pub fn children(&self, id: FrameId) -> &[FrameId] {
        &self.frames[id.index()].children
    }

    /// This frame's gravitational parameter `GM`, in m³/s².
    #[inline]
    #[must_use]
    pub fn mu(&self, id: FrameId) -> f64 {
        self.frames[id.index()].mu
    }

    /// The radius of this frame's sphere of influence, in metres. Infinite for
    /// the root.
    #[inline]
    #[must_use]
    pub fn soi_radius(&self, id: FrameId) -> f64 {
        self.frames[id.index()].soi_radius
    }

    /// Where this frame's origin sits in its parent's frame, and how fast it
    /// moves through it. Both are zero for the root.
    #[inline]
    #[must_use]
    pub fn state_in_parent(&self, id: FrameId) -> State {
        let frame = &self.frames[id.index()];
        State::new(frame.origin, frame.velocity)
    }

    /// Moves a frame within its parent.
    ///
    /// This is how a moon orbits: whatever is propagating it — an integrator
    /// now, the Kepler propagator later — writes the new state here each tick,
    /// and every conversion through it follows.
    ///
    /// # Panics
    ///
    /// If `id` is the root, whose origin is the definition of the hierarchy's
    /// coordinates and cannot move without moving everything with it.
    pub fn set_state(&mut self, id: FrameId, state: State) {
        assert!(
            self.frames[id.index()].parent.is_some(),
            "the root frame's origin is what every other frame is measured from and cannot move"
        );
        let frame = &mut self.frames[id.index()];
        frame.origin = state.position.normalize();
        frame.velocity = state.velocity;
    }

    /// The deepest frame that is an ancestor of both `a` and `b`, or one of
    /// them if it is an ancestor of the other.
    ///
    /// Every pair has one, because every frame descends from the root.
    #[must_use]
    pub fn common_ancestor(&self, a: FrameId, b: FrameId) -> FrameId {
        let (mut a, mut b) = (a, b);
        // Lift the deeper one until they are level, then step both up together.
        // The parent of a non-root frame always exists, so neither unwrap can
        // fire before the two meet at the root.
        while self.frames[a.index()].depth > self.frames[b.index()].depth {
            a = self.frames[a.index()]
                .parent
                .expect("a deeper frame has a parent");
        }
        while self.frames[b.index()].depth > self.frames[a.index()].depth {
            b = self.frames[b.index()]
                .parent
                .expect("a deeper frame has a parent");
        }
        while a != b {
            a = self.frames[a.index()]
                .parent
                .expect("frames level and unequal are not the root");
            b = self.frames[b.index()]
                .parent
                .expect("frames level and unequal are not the root");
        }
        a
    }

    /// Reads a state given in frame `from` as it stands in frame `to`.
    ///
    /// Exact in position: every step is a sector-and-offset addition, which
    /// loses nothing, so converting out and back returns the position bit for
    /// bit. Velocity is a plain `f64` sum of the frames' velocities and is as
    /// exact as those are.
    #[must_use]
    pub fn convert(&self, state: State, from: FrameId, to: FrameId) -> State {
        let meeting = self.common_ancestor(from, to);

        let mut state = state;
        let mut up = from;
        while up != meeting {
            let frame = &self.frames[up.index()];
            state.position = compose(frame.origin, state.position);
            state.velocity += frame.velocity;
            up = frame.parent.expect("the walk stops at the common ancestor");
        }

        // The way down is the same steps in reverse, so collect them first.
        let mut down = Vec::new();
        let mut cursor = to;
        while cursor != meeting {
            down.push(cursor);
            cursor = self.frames[cursor.index()]
                .parent
                .expect("the walk stops at the common ancestor");
        }
        for id in down.into_iter().rev() {
            let frame = &self.frames[id.index()];
            state.position = decompose(state.position, frame.origin);
            state.velocity -= frame.velocity;
        }

        state
    }

    /// The frame a body at `state` in `frame` should be simulated in next, or
    /// `None` if it is already in the right one.
    ///
    /// One step: a body that has fallen through two boundaries in a tick — a
    /// tick long enough to cross a whole sphere of influence — needs the answer
    /// applied and the question asked again. The caller does that rather than a
    /// loop here, because a malformed hierarchy is the one thing that could
    /// make such a loop run for ever, and a caller that has counted its steps
    /// notices.
    ///
    /// **Entering a child wins over leaving for the parent.** In a well-formed
    /// hierarchy the two cannot both be true, because a child's sphere of
    /// influence lies inside its parent's; where a hand-built one says
    /// otherwise, the deeper frame is the better approximation.
    #[must_use]
    pub fn transition(&self, state: State, frame: FrameId) -> Option<FrameId> {
        let here = &self.frames[frame.index()];
        for &child in &here.children {
            let inside = &self.frames[child.index()];
            if state.position.distance(inside.origin) < inside.soi_radius {
                return Some(child);
            }
        }
        // `>=` so the boundary itself belongs to the parent, which is the same
        // side the child test above puts it on: exactly one frame claims a body
        // standing on the line.
        if state.position.distance(WorldPos::ORIGIN) >= here.soi_radius {
            return here.parent;
        }
        None
    }
}

/// The radius of a body's sphere of influence, in metres.
///
/// The Laplace sphere of influence of the patched-conic approximation
/// (Bate, Mueller & White, *Fundamentals of Astrodynamics*, §7.4):
///
/// ```text
/// r_soi = a · (m / M)^(2/5)
/// ```
///
/// where `a` is the semi-major axis of the body's orbit about its primary and
/// `m / M` the mass ratio between them. It is given here in gravitational
/// parameters because `G` cancels — `mu = GM`, so `mu / primary_mu` is that
/// same ratio — and because `mu` is what is actually published to eight
/// figures for a body while its mass is not.
///
/// It is not a physical surface. It is where the primary's perturbation of a
/// two-body orbit about the body first exceeds the body's perturbation of a
/// two-body orbit about the primary, which is the point at which it is cheaper
/// to be wrong the other way round.
///
/// # Panics
///
/// If `primary_mu` is not positive, which would make the ratio meaningless, or
/// if `semi_major_axis` or `mu` is negative.
#[must_use]
pub fn sphere_of_influence(semi_major_axis: f64, mu: f64, primary_mu: f64) -> f64 {
    assert!(
        semi_major_axis >= 0.0,
        "a semi-major axis is not negative, got {semi_major_axis}"
    );
    assert!(
        mu >= 0.0,
        "a gravitational parameter is not negative, got {mu}"
    );
    assert!(
        primary_mu > 0.0,
        "the primary must have mass for a sphere of influence to be defined, got {primary_mu}"
    );
    semi_major_axis * (mu / primary_mu).powf(0.4)
}

// ---------------------------------------------------------------------------
// Sector-exact offsets
// ---------------------------------------------------------------------------
//
// A frame's origin in its parent is a *displacement*, and one that can be
// larger than a sector needs the same integer-plus-offset form a position does
// — so it is carried as a `WorldPos` and chained by adding the two parts. The
// alternative, `WorldPos::delta` down to a `DVec3` and back, would round the
// sum to whatever `f64` can hold at that magnitude, which is the seam this
// whole module exists to avoid.

/// `origin` displaced by `offset`: reading a position given in a frame as it
/// stands in that frame's parent.
fn compose(origin: WorldPos, offset: WorldPos) -> WorldPos {
    WorldPos::new(
        saturating_add(origin.sector, offset.sector),
        origin.local + offset.local,
    )
}

/// The inverse of [`compose`]: reading a position given in a parent as it
/// stands in the child whose origin is at `origin`.
fn decompose(position: WorldPos, origin: WorldPos) -> WorldPos {
    WorldPos::new(
        saturating_sub(position.sector, origin.sector),
        position.local - origin.local,
    )
}

/// Sector addition that clamps at the edge of the addressable volume rather
/// than wrapping to the far side of it, matching what
/// [`WorldPos::normalize`] already does with a carry.
fn saturating_add(a: I64Vec3, b: I64Vec3) -> I64Vec3 {
    I64Vec3::new(
        a.x.saturating_add(b.x),
        a.y.saturating_add(b.y),
        a.z.saturating_add(b.z),
    )
}

/// Sector subtraction, clamping for the same reason [`saturating_add`] does.
fn saturating_sub(a: I64Vec3, b: I64Vec3) -> I64Vec3 {
    I64Vec3::new(
        a.x.saturating_sub(b.x),
        a.y.saturating_sub(b.y),
        a.z.saturating_sub(b.z),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Gravitational parameters as published by JPL (DE440, `GM` in m³/s²) —
    /// the values the spheres of influence below are computed from.
    const SUN_MU: f64 = 1.327_124_400_18e20;
    const EARTH_MU: f64 = 3.986_004_418e14;
    const MOON_MU: f64 = 4.902_800_118e12;

    /// Semi-major axes in metres: Earth about the Sun, and the Moon about
    /// Earth.
    const EARTH_ORBIT: f64 = 1.495_978_707e11;
    const MOON_ORBIT: f64 = 3.844_00e8;

    /// A hierarchy of Sun → Earth → Moon, with the Moon out along `+x` and
    /// Earth far enough from the origin that a naive `f64` sum would lose
    /// metres.
    fn solar_system() -> (Frames, FrameId, FrameId, FrameId) {
        let mut frames = Frames::new(SUN_MU);
        let sun = frames.root();
        let earth = frames.add(
            sun,
            EARTH_MU,
            sphere_of_influence(EARTH_ORBIT, EARTH_MU, SUN_MU),
            WorldPos::from_offset(DVec3::new(EARTH_ORBIT, 0.0, 0.0)),
            DVec3::new(0.0, 29_780.0, 0.0),
        );
        let moon = frames.add(
            earth,
            MOON_MU,
            sphere_of_influence(MOON_ORBIT, MOON_MU, EARTH_MU),
            WorldPos::from_offset(DVec3::new(MOON_ORBIT, 0.0, 0.0)),
            DVec3::new(0.0, 1_022.0, 0.0),
        );
        (frames, sun, earth, moon)
    }

    /// **The formula against the published radii**, not against numbers this
    /// file chose: Earth's sphere of influence is about 924 000 km and the
    /// Moon's about 66 100 km. Both are quoted to three figures in the
    /// literature, so 1% is the tolerance the sources support.
    #[test]
    fn the_spheres_of_influence_match_the_published_radii() {
        let earth = sphere_of_influence(EARTH_ORBIT, EARTH_MU, SUN_MU);
        let published = 9.24e8;
        assert!(
            (earth - published).abs() <= published * 0.01,
            "Earth's SOI came out {earth} m, published {published} m"
        );

        let moon = sphere_of_influence(MOON_ORBIT, MOON_MU, EARTH_MU);
        let published = 6.61e7;
        assert!(
            (moon - published).abs() <= published * 0.01,
            "the Moon's SOI came out {moon} m, published {published} m"
        );

        // The exponent is what makes it a Laplace radius rather than any other
        // power law, so pin it: quadrupling the mass ratio must multiply the
        // radius by 4^(2/5), which no neighbouring exponent gives.
        let base = sphere_of_influence(1.0, 1.0, 1.0e6);
        let quadrupled = sphere_of_influence(1.0, 4.0, 1.0e6);
        let ratio = quadrupled / base;
        let expected = 4.0f64.powf(0.4);
        assert!(
            (ratio - expected).abs() < 1e-12,
            "four times the mass must give {expected} times the radius, got {ratio}"
        );
    }

    /// A conversion out and back is the identity, to the bit.
    ///
    /// This is the claim [`Frames::convert`] makes, and it is only true because
    /// the walk adds sector indices as integers. The state used sits a long way
    /// from every origin involved, where an `f64` sum would already have lost
    /// the low bits.
    #[test]
    fn a_conversion_out_and_back_returns_the_state_exactly() {
        let (frames, sun, _earth, moon) = solar_system();
        let start = State::new(
            WorldPos::from_offset(DVec3::new(1.0e7, -2.5e6, 3.25e6)),
            DVec3::new(-900.0, 120.5, 7.25),
        );

        let out = frames.convert(start, moon, sun);
        let back = frames.convert(out, sun, moon);
        assert_eq!(
            back.position, start.position,
            "a round trip through the hierarchy must return the same position"
        );
        assert_eq!(back.velocity, start.velocity);
    }

    /// **The seam test.** A millimetre at the far side of the solar system
    /// survives a frame conversion.
    ///
    /// Earth's frame sits 1.496 × 10¹¹ m from the root here, where an `f64`
    /// metre coordinate has about 30 μm between neighbouring values — a
    /// millimetre still survives that, so the test places the pair a further
    /// 10¹⁵ m out, where the spacing is about a quarter of a metre and a naive
    /// sum would collapse the two positions onto each other or fling them
    /// apart. What is asserted is the *separation*, because that is what a
    /// viewer sees: a seam is two points that were a millimetre apart landing
    /// somewhere else relative to one another.
    #[test]
    fn a_crossing_far_from_the_origin_keeps_millimetres() {
        const FAR: f64 = 1.0e15;
        const MILLIMETRE: f64 = 1.0e-3;

        let mut frames = Frames::new(SUN_MU);
        let sun = frames.root();
        let far = frames.add(
            sun,
            EARTH_MU,
            1.0e9,
            WorldPos::from_offset(DVec3::new(FAR, FAR, FAR)),
            DVec3::ZERO,
        );

        let a = State::new(
            WorldPos::from_offset(DVec3::new(1.0e8, 0.0, 0.0)),
            DVec3::ZERO,
        );
        // Displaced from `a` rather than built from `1.0e8 + 1.0e-3`, which is
        // not a representable `f64` and would start the pair seven nanometres
        // off the millimetre before anything under test had run. Added to a
        // normalized position it lands on the small local offset, where it is
        // exact — which is the whole reason a position is held this way.
        let b = State::new(a.position.translated(DVec3::X * MILLIMETRE), DVec3::ZERO);
        let before = a.position.distance(b.position);
        assert!(
            (before - MILLIMETRE).abs() < MILLIMETRE * 1e-6,
            "the two should start about a millimetre apart, got {before} m"
        );

        let converted_a = frames.convert(a, far, sun);
        let converted_b = frames.convert(b, far, sun);
        let after = converted_a.position.distance(converted_b.position);
        // A conversion is one `f64` addition per axis on a coordinate below
        // `SECTOR_SIZE`, so it can move a separation by at most an ulp there —
        // `2^20` has an ulp of `2^-32` m, under a quarter of a nanometre.
        const SEAM: f64 = 1.0e-9;
        assert!(
            (after - before).abs() < SEAM,
            "a millimetre {FAR} m from the origin came back as {after} m, from {before} m"
        );

        // And that this is worth doing: the same two points held as plain `f64`
        // metres from the root are the same point.
        let naive_a = FAR + 1.0e8;
        let naive_b = FAR + 1.0e8 + MILLIMETRE;
        assert_eq!(
            naive_b - naive_a,
            0.0,
            "an `f64` metre coordinate at {FAR} m cannot hold a millimetre at all, \
             which is what the sector split is for"
        );
    }

    /// Between two frames neither of which contains the other, the walk goes up
    /// to the common ancestor and back down — and lands where the geometry
    /// says.
    #[test]
    fn a_state_converts_between_siblings_through_their_common_ancestor() {
        let (mut frames, sun, earth, moon) = solar_system();
        let mars = frames.add(
            sun,
            4.282_837e13,
            1.0e9,
            WorldPos::from_offset(DVec3::new(0.0, 2.279_39e11, 0.0)),
            DVec3::ZERO,
        );
        assert_eq!(frames.common_ancestor(moon, mars), sun);
        assert_eq!(frames.common_ancestor(moon, earth), earth);
        assert_eq!(frames.common_ancestor(earth, earth), earth);

        // A body sitting exactly on the Moon's origin is, seen from Mars, at
        // whatever vector separates the two origins.
        let seen = frames.convert(State::AT_ORIGIN, moon, mars);
        let expected = DVec3::new(EARTH_ORBIT + MOON_ORBIT, -2.279_39e11, 0.0);
        let error = (seen.position.delta(WorldPos::ORIGIN) - expected).length();
        assert!(
            error < 1.0e-6,
            "the Moon seen from Mars should be at {expected:?}, came out {:?}",
            seen.position.delta(WorldPos::ORIGIN)
        );

        // And its velocity is its own plus Earth's, since Mars is not moving in
        // this hierarchy.
        assert_eq!(seen.velocity, DVec3::new(0.0, 29_780.0 + 1_022.0, 0.0));
    }

    /// Past the boundary the body belongs to the parent; inside a child's, to
    /// the child; and in between, to neither.
    #[test]
    fn a_body_crossing_a_boundary_is_handed_to_the_frame_that_now_dominates() {
        let (frames, sun, earth, moon) = solar_system();
        let moon_soi = frames.soi_radius(moon);

        let inside = State::new(
            WorldPos::from_offset(DVec3::new(moon_soi * 0.5, 0.0, 0.0)),
            DVec3::ZERO,
        );
        assert_eq!(
            frames.transition(inside, moon),
            None,
            "a body well inside the Moon's sphere of influence stays in it"
        );

        let outside = State::new(
            WorldPos::from_offset(DVec3::new(moon_soi * 1.5, 0.0, 0.0)),
            DVec3::ZERO,
        );
        assert_eq!(
            frames.transition(outside, moon),
            Some(earth),
            "past the Moon's sphere of influence, Earth dominates"
        );

        // The same body read in Earth's frame is inside the Moon's SOI again
        // only if it is near the Moon; put one there and Earth must hand it
        // over.
        let near_moon = frames.convert(inside, moon, earth);
        assert_eq!(
            frames.transition(near_moon, earth),
            Some(moon),
            "a body inside the Moon's sphere of influence is handed to the Moon"
        );

        // Halfway out to the Moon it is Earth's and nobody else's.
        let between = State::new(
            WorldPos::from_offset(DVec3::new(MOON_ORBIT * 0.5, 0.0, 0.0)),
            DVec3::ZERO,
        );
        assert_eq!(frames.transition(between, earth), None);

        // Nothing leaves the root: its sphere of influence is everything.
        let anywhere = State::new(WorldPos::from_offset(DVec3::splat(1.0e18)), DVec3::ZERO);
        assert_eq!(frames.transition(anywhere, sun), None);
    }

    /// **A transition changes the description, not the body.**
    ///
    /// The state that comes out the other side of a hand-over, read back in the
    /// root, is the state that went in — so nothing accelerates, teleports or
    /// jitters at the boundary, which is what `06-orbit.md` means by an
    /// invisible crossing.
    #[test]
    fn a_transition_leaves_the_physical_state_untouched() {
        let (frames, sun, earth, moon) = solar_system();
        let moon_soi = frames.soi_radius(moon);

        // Sat exactly on the boundary, moving outward.
        let leaving = State::new(
            WorldPos::from_offset(DVec3::new(moon_soi, 0.0, 0.0)),
            DVec3::new(310.0, -40.0, 5.0),
        );
        assert_eq!(frames.transition(leaving, moon), Some(earth));

        let handed_over = frames.convert(leaving, moon, earth);
        assert_eq!(
            frames.convert(handed_over, earth, sun),
            frames.convert(leaving, moon, sun),
            "the same body seen from the root must not move when it changes frame"
        );

        // And it is still on the boundary: its distance from the Moon's origin,
        // now measured in Earth's frame, is the radius it crossed.
        let moon_origin = frames.state_in_parent(moon).position;
        let still_there = handed_over.position.distance(moon_origin);
        assert!(
            (still_there - moon_soi).abs() < 1.0e-6,
            "the body was {moon_soi} m from the Moon and is now {still_there} m from it"
        );

        // The velocity it is handed over with differs from the one it had by
        // exactly the Moon's own — no more, no less.
        assert_eq!(
            handed_over.velocity - leaving.velocity,
            frames.state_in_parent(moon).velocity
        );
    }

    /// A frame that moves takes everything in it along.
    #[test]
    fn moving_a_frame_moves_what_is_measured_through_it() {
        let (mut frames, sun, _earth, moon) = solar_system();
        let before = frames.convert(State::AT_ORIGIN, moon, sun);

        let shifted = frames.state_in_parent(moon);
        frames.set_state(
            moon,
            State::new(
                shifted.position.translated(DVec3::new(0.0, 1.0e6, 0.0)),
                shifted.velocity,
            ),
        );

        let after = frames.convert(State::AT_ORIGIN, moon, sun);
        let moved = after.position.delta(before.position);
        assert!(
            (moved - DVec3::new(0.0, 1.0e6, 0.0)).length() < 1.0e-6,
            "moving the Moon a megametre should move a body sitting on it the same, got {moved:?}"
        );
    }
}
