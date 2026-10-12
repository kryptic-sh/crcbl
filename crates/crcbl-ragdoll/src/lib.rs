//! Ragdolls: a skeleton's bones as jointed rigid bodies, thrown by the
//! killing blow and read back into the pose that skins the mesh
//! (`docs/plan/35-ragdolls.md`).
//!
//! - A [`RagdollRig`] says which skeleton joints carry bodies, their shapes
//!   and masses, the joints between them and the pairs kept from colliding.
//!   It comes from an authored [`RagdollDesc`] file, resolved against the
//!   skeleton's bone names, or from [`RagdollRig::generate`], fitted to the
//!   bones when nobody has authored one.
//! - A [`Ragdoll`] is a rig spawned into a
//!   [`PhysicsSystem`](crcbl_phys::PhysicsSystem) at the last animated pose,
//!   moving as that pose was. [`Ragdoll::strike_with`] carries a recorded
//!   contact's impulse over to the limb it landed on, and
//!   [`Ragdoll::pose_into`] writes the pose the bodies stand in, through
//!   [`crcbl_anim::ragdoll::RagdollBinding`].
//!
//! Not here yet: the server's simplified ragdoll and the settled-pose
//! snapshot sent to clients, the soft pull between the two, editor preview,
//! budgets and distance LOD. Deciding the death, choosing the killing
//! contact and revival stay with the game.

mod desc;
mod frames;
mod generate;
mod instance;
mod rig;

pub use desc::{BodyDesc, DescError, JointDesc, JointKindDesc, RagdollDesc, ShapeDesc};
pub use frames::REST_CLEARANCE;
pub use generate::{GenerateError, GenerateSettings};
pub use instance::{Handoff, PreviousPose, Ragdoll, SpawnError, StrikeError};
pub use rig::{BodyShape, RagdollRig, RigBody, RigError, RigJoint, RigJointKind};
