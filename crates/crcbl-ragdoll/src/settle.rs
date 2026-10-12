//! Coming to rest: the settled pose a server sends and a client takes, and
//! the budget that settles ragdolls a game cannot afford to keep simulating.
//!
//! `docs/plan/35-ragdolls.md` has the server settle a corpse and broadcast
//! its final pose, so every client shows the same body in the same place.
//! [`SettledPose`] is that pose on the wire — each body's transform in the
//! physics crate's quantized replication form — and [`Ragdoll::settle_to`]
//! puts a ragdoll into it, asleep. How the pose travels, and any blend a
//! client draws on the way, are the game's.
//!
//! [`SettleBudget`] is the plan's two guards: a ragdoll still awake after a
//! timeout is settled where it stands, and when more are awake than the cap
//! the longest-awake are. The plan's "excess deaths settle instantly to a
//! canned pose" is not followed to the letter: a newly dead body settled at
//! once would hang in the air, and there is no canned pose to swap in, so
//! the cap settles the ragdolls that have had longest to fall instead.

use crcbl_phys::{PhysicsSystem, Transform};

use crate::Ragdoll;

/// Each body's transform, in rig order, as a server sends a settled corpse.
#[derive(Clone, Debug, PartialEq)]
pub struct SettledPose {
    bodies: Vec<Transform>,
}

/// The first byte of an encoded [`SettledPose`]: bumped if the layout
/// changes, so an old payload is refused rather than misread.
const FORMAT: u8 = 1;

impl SettledPose {
    /// A pose of these body transforms, in rig order.
    #[must_use]
    pub const fn new(bodies: Vec<Transform>) -> Self {
        Self { bodies }
    }

    /// The body transforms, in rig order.
    #[must_use]
    pub fn bodies(&self) -> &[Transform] {
        &self.bodies
    }

    /// Appends the pose's wire form to `out`: a format byte, the body count
    /// as two little-endian bytes, then per body a length byte and
    /// [`Transform::encode_wire`]'s bytes, which quantize a position inside
    /// [`Transform::WIRE_POSITION_EXTENT`] and carry one outside it exactly.
    ///
    /// # Panics
    ///
    /// Panics for more than `u16::MAX` bodies, which no rig has.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let count = u16::try_from(self.bodies.len()).expect("a rig has fewer than 65536 bodies");
        out.push(FORMAT);
        out.extend_from_slice(&count.to_le_bytes());
        let mut body = Vec::with_capacity(Transform::ENCODED_LEN);
        for transform in &self.bodies {
            body.clear();
            transform.encode_wire(&mut body);
            let length = u8::try_from(body.len()).expect("a transform encodes in under 256 bytes");
            out.push(length);
            out.extend_from_slice(&body);
        }
    }

    /// Reads what [`encode`](Self::encode) wrote. `None` for another format,
    /// a short or overlong payload, or a body [`Transform::decode_wire`]
    /// refuses — these are bytes a peer sent.
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        let (&format, rest) = data.split_first()?;
        if format != FORMAT {
            return None;
        }
        let (count, mut rest) = rest.split_first_chunk::<2>()?;
        let count = usize::from(u16::from_le_bytes(*count));
        let mut bodies = Vec::with_capacity(count.min(rest.len()));
        for _ in 0..count {
            let (&length, after) = rest.split_first()?;
            let (body, after) = after.split_at_checked(usize::from(length))?;
            bodies.push(Transform::decode_wire(body)?);
            rest = after;
        }
        rest.is_empty().then_some(Self { bodies })
    }
}

/// Why [`Ragdoll::settle_to`] did not apply a pose. Nothing moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettleError {
    /// The pose has another number of bodies than the ragdoll.
    #[error("the pose has {pose} bodies and the ragdoll {ragdoll}")]
    BodyCount {
        /// The pose's.
        pose: usize,
        /// The ragdoll's.
        ragdoll: usize,
    },
    /// A body of the ragdoll is no longer in the system.
    #[error("a ragdoll body is no longer in the physics system")]
    BodyMissing,
    /// A transform in the pose is not finite with a unit rotation.
    #[error("a settled pose transform is not a rigid transform")]
    InvalidTransform,
}

impl Ragdoll {
    /// The pose the bodies stand in now, to send once the ragdoll has
    /// settled. `None` if a body is gone.
    #[must_use]
    pub fn settled_pose(&self, physics: &PhysicsSystem) -> Option<SettledPose> {
        self.body_transforms(physics).map(SettledPose::new)
    }

    /// Puts every body at `pose`, still, and asleep: a client taking the
    /// server's settled corpse, or a game restoring one. Sleep needs a system
    /// with it on, as [`Ragdoll::force_settle`] does.
    ///
    /// # Errors
    ///
    /// [`SettleError`] for a pose of another size, a missing body or a
    /// transform that is not rigid, checked before anything moves.
    pub fn settle_to(
        &mut self,
        physics: &mut PhysicsSystem,
        pose: &SettledPose,
    ) -> Result<(), SettleError> {
        if pose.bodies.len() != self.entities.len() {
            return Err(SettleError::BodyCount {
                pose: pose.bodies.len(),
                ragdoll: self.entities.len(),
            });
        }
        if self.entities.iter().any(|&e| physics.body(e).is_none()) {
            return Err(SettleError::BodyMissing);
        }
        if !pose.bodies.iter().all(crate::rig::is_rigid) {
            return Err(SettleError::InvalidTransform);
        }
        for (&entity, transform) in self.entities.iter().zip(&pose.bodies) {
            physics.set_transform(entity, *transform);
            let body = physics.body_mut(entity).expect("checked above");
            body.velocity = glam::DVec3::ZERO;
            body.angular_velocity = glam::DVec3::ZERO;
        }
        self.force_settle(physics);
        self.awake_for = 0.0;
        Ok(())
    }

    /// How long the ragdoll has been awake without a break, in seconds, as
    /// [`SettleBudget::enforce`] counts it.
    #[must_use]
    pub const fn awake_for(&self) -> f64 {
        self.awake_for
    }
}

/// The plan's guards on ragdolls a game keeps simulating: see the module
/// docs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettleBudget {
    /// The most ragdolls awake at once.
    pub max_awake: usize,
    /// The longest a ragdoll may stay awake, in seconds.
    pub timeout: f64,
}

impl SettleBudget {
    /// Counts `dt` seconds onto every awake ragdoll's [`Ragdoll::awake_for`]
    /// and back to zero for each asleep, settles each one past the timeout,
    /// then settles the longest-awake while more than `max_awake` remain.
    /// Returns the indices settled, in the order they were.
    ///
    /// Call it once a step, after the physics step, over every ragdoll the
    /// game keeps. A ragdoll settles where it stands, its velocities gone; a
    /// system with sleep off cannot settle anything and settles nothing.
    pub fn enforce(
        &self,
        physics: &mut PhysicsSystem,
        ragdolls: &mut [Ragdoll],
        dt: f64,
    ) -> Vec<usize> {
        let mut settled = Vec::new();
        for (index, ragdoll) in ragdolls.iter_mut().enumerate() {
            if ragdoll.is_settled(physics) {
                ragdoll.awake_for = 0.0;
                continue;
            }
            ragdoll.awake_for += dt;
            if ragdoll.awake_for > self.timeout && ragdoll.force_settle(physics) {
                ragdoll.awake_for = 0.0;
                settled.push(index);
            }
        }
        let mut awake: Vec<usize> = (0..ragdolls.len())
            .filter(|&index| !ragdolls[index].is_settled(physics))
            .collect();
        if awake.len() > self.max_awake {
            // Longest awake first; equal times keep the caller's order.
            awake.sort_by(|&a, &b| ragdolls[b].awake_for.total_cmp(&ragdolls[a].awake_for));
            for &index in &awake[..awake.len() - self.max_awake] {
                if ragdolls[index].force_settle(physics) {
                    ragdolls[index].awake_for = 0.0;
                    settled.push(index);
                }
            }
        }
        settled
    }
}
