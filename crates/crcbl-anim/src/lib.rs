//! Skeletal animation: clip sampling and joint palettes.
//!
//! The client half of the animation evaluation stack recorded in
//! `docs/notes/simulation.md` (_What the deleted 17-animation plan left
//! behind_) — its clip-sampling step, the palette the stack ends at, and the
//! blending above them; plus two-bone IK ([`ik`]), the one post op a caller has
//! asked for, by EW, whose character rig is to place its hands with it; **nothing
//! above that**. There is no GPU skinning here — the skinning dispatch is
//! `crcbl-render`'s (`skinning.rs`), and it takes a [`Palette`] this crate
//! produced. A state machine and root motion are later slices with their own
//! consumers, and building them now against no caller is the failure this
//! project guards against. What is here is what `docs/plan/sample/09-puppet.md`
//! needs through its milestone 2: a character posed from a clip, and a
//! locomotion set mixed by speed.
//!
//! ```text
//! Skeleton   joints in palette order — parent index, inverse bind, rest pose
//! Clip       channels of keyframes over that skeleton
//! Pose       one local Trs per joint, the result of sampling a clip
//! Palette    those composed down the hierarchy and folded against the binds
//!
//! blend_into      two poses mixed by weight, rotations along the shorter arc
//! BlendSpace1d    clips on one axis — idle, walk, run — picked by speed
//!
//! rotate_joint    one joint turned by a rotation spelled in model space
//! solve_two_bone  a limb's end put on a target, bent towards a pole
//! ```
//!
//! # A frame
//!
//! ```
//! use crcbl_anim::{Channel, Clip, Interpolation, Joint, Palette, Pose, Skeleton, Track, Trs};
//! use glam::{Mat4, Quat, Vec3};
//!
//! let skeleton = Skeleton::new(vec![
//!     Joint { parent: None, inverse_bind: Mat4::IDENTITY, rest: Trs::IDENTITY },
//!     Joint {
//!         parent: Some(0),
//!         inverse_bind: Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0)),
//!         rest: Trs { translation: Vec3::new(0.0, 2.0, 0.0), ..Trs::IDENTITY },
//!     },
//! ])?;
//!
//! let spin = Channel::new(
//!     0,
//!     vec![0.0, 1.0],
//!     Interpolation::Linear,
//!     Track::Rotation(vec![Quat::IDENTITY, Quat::from_rotation_z(std::f32::consts::PI)]),
//! )?;
//! let clip = Clip::new(vec![spin]);
//!
//! // Built once, refilled every frame: sampling allocates nothing.
//! let mut pose = Pose::new(&skeleton);
//! let mut palette = Palette::new(&skeleton);
//!
//! clip.sample_into(0.5, &skeleton, &mut pose);
//! palette.compute(&skeleton, &pose);
//! assert_eq!(palette.matrices().len(), skeleton.len());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # This crate does not import glTF
//!
//! It depends on `glam` and nothing else — in particular not on `crcbl-scene`,
//! which is where the parse lives. `crates/crcbl-scene/src/gltf_import.rs`
//! hands out `GltfSkin`, `GltfClip` and `GltfChannel`; turning those into a
//! [`Skeleton`] and a [`Clip`] is index bookkeeping that belongs to whoever
//! holds both, and a *second* consumer needing the same conversion is what
//! would justify extracting it. Meanwhile the arrow that is not here is what
//! keeps a glTF parser out of a browser build that only plays cooked clips.
//!
//! The maths, though, is glTF's, because the source format is: every formula
//! below is checked against the 2.0 specification and quoted where it is
//! implemented. Section numbers in the doc comments are that document's —
//! §3.7.3.2 Transformations, §3.7.4 Skins, and Appendix C on the interpolation
//! modes.
//!
//! # Determinism
//!
//! None is claimed. The animation rules in `docs/notes/simulation.md` put pose
//! evaluation on the client — pose math is client presentation and free to vary
//! — and this crate is `f32` throughout, with a slerp that goes through a
//! transcendental. Nothing here belongs in a tick hash.

pub mod blend;
pub mod clip;
pub mod ik;
pub mod palette;
pub mod sample;
pub mod skeleton;
pub mod trs;

pub use blend::{Blend, BlendSpace1d, BlendSpaceError, blend_into};
pub use clip::{Channel, Clip, ClipError, Interpolation, Track};
pub use ik::{FRAME_TOLERANCE, IkError, IkInput, PARALLEL_TOLERANCE, rotate_joint, solve_two_bone};
pub use palette::Palette;
pub use sample::Pose;
pub use skeleton::{Joint, Skeleton, SkeletonError};
pub use trs::Trs;
