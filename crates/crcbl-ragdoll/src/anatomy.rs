//! The humanoid profile of [`RagdollRig::generate`](crate::RagdollRig::generate),
//! documented on [`Humanoid`], which is public where this module is not.

mod ellipse;
mod names;

use core::f64::consts::PI;

use crcbl_core::trig::atan2;
use crcbl_phys::{Spring, rotation_from_scaled_axis};
use glam::{DQuat, DVec3};

use crate::limits::{SoftRange, SoftSwing, SwingTwist};
use crate::rig::{RigJointKind, joint_rotation};
use ellipse::Ellipse;
use names::{Digit, Role, Side, roles};

/// The humanoid profile [`RagdollRig::generate`](crate::RagdollRig::generate)
/// takes from [`GenerateSettings::anatomy`](crate::GenerateSettings::anatomy):
/// joint types, neutrals and limits read off a humanoid's bone names and
/// bind pose (`docs/plan/35-ragdolls.md`, "Anatomical joint limits").
///
/// # Names
///
/// Matched without regard to case, after any namespace (`mixamorig:`):
/// Mixamo's (`LeftUpLeg`, `LeftLeg`, `LeftForeArm`, `LeftHandIndex1`, ...),
/// Unreal's (`thigh_l`, `calf_l`, `upperarm_l`, `lowerarm_l`, `ball_l`,
/// `spine_01`, `neck_01`, `index_01_l`, `index_metacarpal_l`, ...) and EW's
/// (`UpperArm_L`, `indexFinger_01_l`, `finger_01_l`, `toes_l`, ...). The side
/// comes from a `_l`/`_r` suffix or a `Left`/`Right` prefix or suffix; a limb
/// name without one, or a trunk name with one, is not matched. The hierarchy
/// confirms a name: a calf only under its side's thigh, a hand only under its
/// forearm, a digit only on its side's hand or metacarpal, and a digit's
/// segment is counted from the hand, not read from its number. Toes straight
/// off the foot, with no ball bone, are the ball of the foot's joint.
///
/// # Folded bones
///
/// These carry no body and move rigidly with their parent, and so does every
/// bone below one of them: IK helpers (`ik_*`); twist and corrective bones
/// (any name holding `twist` or `corrective`) with no limb below them, which
/// would otherwise make a thigh or a forearm a sphere among its children;
/// eyes, eyebrows and
/// everything else past the head, which is one sphere; everything off the
/// pelvis's own tree — a `root`, Unreal 5's `center_of_mass`; and with
/// [`fold_digits`](Self::fold_digits) the fingers and the toes beyond the
/// ball of the foot. They are not counted among their parent's children
/// either, so a head with eyes is fitted as a head without them.
///
/// A clavicle, a metacarpal, a neck bone below another (Unreal 5's
/// `neck_02`) and a twist bone in a limb's chain (Daz's `lShldrTwist`, with
/// the forearm below it) are spliced out instead: no body, and what hangs from it is
/// fitted and jointed as though it hung from the bone above — an upper arm
/// from the chest, a finger from the hand, the head from the first neck
/// bone. The shoulder's ranges below are the whole shoulder's, clavicle and
/// shoulder blade included, so a joint at the clavicle would count them
/// twice; and a light body held at tight limits between two heavy ones — a
/// clavicle between chest and arm, two short necks between chest and head —
/// threw the solver's energy up by as much as 1.5 kJ in the strike runs of
/// 2026-11-01 and 2026-11-02, where the spliced rigs gained none.
///
/// The head, everything past it folded, is a sphere on the line from its
/// standing ancestor: its radius [`HEAD_REACH`] of its farthest folded bone
/// (a crown end bone makes it the head's height across), or, where that is
/// smaller — EW's eyes and eyebrows, Unreal 5's bare head —
/// [`HEAD_FROM_HEIGHT`] of the head joint's height, de Leva's proportion of
/// head to stature. De Leva's head is 6.68% (women) and 6.94% (men) of the
/// body's mass ("Adjustments to Zatsiorsky-Seluyanov's segment inertia
/// parameters", J. Biomech. 29(9), 1996, table 4); the test humanoids'
/// heads, necks long and short, come to within a sixth of that.
///
/// # Axes and the bind pose
///
/// The body's axes come from the bind pose as placed by the model: **left**
/// from the right thigh to the left (the hip line), or from the right upper
/// arm to the left without thighs; **up** from the pelvis, or the middle of
/// the thighs, to the head, neck or highest spine bone; **forward** is left
/// across up, made square to both. So the bind may face any way and stand
/// at any scale; what is assumed is that a `_l` bone is the character's own
/// left, that the trunk is upright, and that the wrists, ankles, spine and
/// neck stand at their anatomical zero — wrists straight, feet flat. T-pose
/// and A-pose both hold that; the hips and shoulders, which the
/// two differ in, and every hinge are measured, not assumed. A hand's palm
/// faces across its index and other fingers' knuckles, or down with no
/// fingers to say; an elbow bends forwards from its bind, as it does from a
/// T-pose or A-pose whose upper arms have not been turned outwards. A
/// skeleton the axes cannot be read from keeps today's ball at every joint,
/// and its folds.
///
/// # Joints
///
/// Knees, elbows, finger and toe segments are hinges about the bend axis —
/// square to the bone and to the way it bends (a calf's heel backwards, a
/// forearm forwards, a finger to the palm, a toe down) — so their positive
/// bend is the anatomical flexion, and the joint measures the bind's own
/// bend. Hips, shoulders, the spine, neck, head, wrists, ankles and a
/// thumb's base are balls whose frame's x is the flexion axis and y the
/// abduction axis, the twist about the bone.
///
/// A hinge's neutral is NASA's posture where it gives one, and rest where it
/// does not; a bind up to [`BIND_SLACK`] past straight moves the stop on
/// extension to it. **A ball's swing limit is an ellipse centred on its
/// neutral**, and most anatomical ranges are lopsided about NASA's posture —
/// a shoulder flexes 180° and extends 50°, its posture 36° of flexion — so
/// a ball's neutral is the centre of the ellipse that passes through its
/// four stops (pure flexion, extension, abduction and adduction from the
/// anatomical zero): driven purely along each, it stops at the range's
/// value, and between them where the ellipse runs. NASA's posture, with its
/// spread, is kept inside the free ellipse instead. The one ball no ellipse
/// passes through is the shoulder, whose flexion and abduction both end
/// straight overhead; it stops both [`SHOULDER_SHORT_OF_OVERHEAD`] short. A
/// ball none fits — a fit that does not converge, or converges on a root
/// centred outside the stops or not holding the anatomical zero — keeps the
/// default ball. The swing from rest to the centre is measured from the
/// bind, so an A-posed and a T-posed arm get the same limits; and the twist
/// is measured from the anatomical zero as the shortest swing from the bind
/// reaches it, the neutral turning the child's frame back by whatever twist
/// the bind carries there (an arm in a T-pose, 31.2°; in an A-pose 45°
/// down, 16.8°), so the twist limits are the same for both.
///
/// # Ranges: sources
///
/// Hard limits, in degrees from the anatomical zero, from:
///
/// - **38 CFR §4.71, Plates I and II** (US Department of Veterans Affairs
///   rating schedule; the anatomical position is 0°): shoulder flexion 180,
///   abduction 180, internal and external rotation 90; elbow flexion 145;
///   forearm pronation 80, supination 85; wrist extension 70, flexion 80,
///   radial deviation 20, ulnar deviation 45; hip flexion 125, abduction 45;
///   knee flexion 140; ankle dorsiflexion 20, plantar flexion 45.
/// - **38 CFR §4.71a**, the spine's general rating formula, note (2):
///   cervical flexion, extension and lateral flexion 45, rotation 80;
///   thoracolumbar flexion 90, extension, lateral flexion and rotation 30.
///   The hand's digits, note (1): metacarpophalangeal 90, proximal
///   interphalangeal 100, distal interphalangeal "70 or 80" (70 is used).
/// - **Washington State DSHS form 13-585A**, "Range of Joint Motion
///   Evaluation Chart" (rev. 03/2014), for what the CFR does not give: hip
///   extension 30, adduction 20; shoulder extension 50, adduction 30; ankle
///   inversion 30, eversion 20; thumb metacarpophalangeal flexion 60,
///   interphalangeal 80.
///
/// A range the trunk shares is split evenly across the joints that carry it:
/// the thoracolumbar one across the spine's bodied joints, the cervical one
/// across the neck's and the head's. **No source was found** for hip
/// rotation, a thumb's base, an ankle's toe-in and toe-out, or the toes;
/// those are engineering values, each named so below.
///
/// Neutrals, from NASA/SP-2010-3407 Rev 1, figure 4.3-2 (the plan's table):
/// knee 47° (±8), elbow 58° (±24), hip flexion 52° (±7) and abduction 12°
/// (±6), shoulder flexion 36° (±19), the head and neck 24° (±5) forwards
/// split like their range, and the ankle 26° (±6) of plantar flexion: the
/// figure's 21° from the torso line, plus the 5° the shank stands forward of
/// it at the posture's hip and knee (52° − 47°).
///
/// # Soft zones
///
/// A hinge's free zone is NASA's mean ± its spread, or the neutral alone
/// where NASA gives none, and its soft bounds lie [`SOFT_FRACTION`] of the
/// way from the free zone's edges to the hard limits: free near the
/// posture, resisted over the outer part of the range, harder towards the
/// stop. A ball's soft ellipse is its hard one scaled about the same
/// centre: just big enough to hold NASA's posture box (mean ± spread on
/// both axes, or the anatomical zero), then [`SOFT_FRACTION`] of the way out
/// to the hard one. A bind pose is not that posture: a standing thigh starts
/// in its hip's soft zone, 1.12 times the soft ellipse out, and is pushed
/// towards the posture, as the owner's rules ask; a hanging arm, 0.92 of the
/// way out, is free. Every zone is [`ZONE`]'s strength.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Humanoid {
    /// Whether the fingers and the toes beyond the ball of the foot carry no
    /// body and move with their hand or forefoot: fewer, heavier bodies and
    /// no trembling digits, for a ragdoll seen from afar.
    pub fold_digits: bool,
}

impl Humanoid {
    /// Every digit a body of its own.
    pub const DEFAULT: Self = Self { fold_digits: false };
}

impl Default for Humanoid {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How far from the free zone's edge to the hard limit the soft bound lies,
/// as a fraction of that distance: halfway leaves the inner half of the
/// range beyond the posture free and ramps the resistance over the outer
/// half, so a limb meets resistance well before its stop and none near its
/// posture.
pub const SOFT_FRACTION: f64 = 0.5;

/// Every soft zone's strength at its hard limit, critically damped: the
/// frequency `crcbl-phys`'s energy tests measure soft zones at
/// (`crates/crcbl-phys/tests/joints.rs`), well under the rigid rows'
/// [`crcbl_phys::Joint::CONSTRAINT_HERTZ`] so the hard stop still decides
/// where a joint ends. A frequency rather than a stiffness, so a finger's
/// zone is as quick as a thigh's.
pub const ZONE: Spring = Spring {
    hertz: 8.0,
    damping_ratio: 1.0,
};

/// A range on one axis, in degrees from the anatomical zero, positive the
/// way the axis names.
#[derive(Clone, Copy)]
struct Range {
    least: f64,
    most: f64,
}

/// NASA's neutral posture on one axis, in degrees: its mean and spread.
#[derive(Clone, Copy)]
struct Posture {
    mean: f64,
    spread: f64,
}

const fn range(least: f64, most: f64) -> Range {
    Range { least, most }
}

const fn posture(mean: f64, spread: f64) -> Posture {
    Posture { mean, spread }
}

/// No NASA posture: the free zone is the anatomical zero alone.
const AT_ZERO: Posture = posture(0.0, 0.0);

/// How far short of overhead, in degrees, the shoulder's flexion and
/// abduction stop: both sources' 180° is the same swing, straight up, and an
/// ellipse through it twice is not defined. This short keeps the fit well
/// conditioned (it fails at the overhead point itself, converges within it)
/// and the arm nearly overhead either way.
pub const SHOULDER_SHORT_OF_OVERHEAD: f64 = 10.0;

/// A head sphere's radius as a fraction of the farthest folded bone past
/// the head joint: half, so a crown end bone (Mixamo's `HeadTop_End`) is
/// the sphere's diameter. See [`Humanoid`]'s folded bones.
pub const HEAD_REACH: f64 = 0.5;

/// The least a head sphere's radius may be, as a fraction of the head
/// joint's height over the body's lowest joint: half de Leva's head length
/// (vertex to mid-gonion, 203.3 mm) over the stature below it (1741 mm less
/// that) for men, table 4 of "Adjustments to Zatsiorsky-Seluyanov's segment
/// inertia parameters", J. Biomech. 29(9), 1996. The head joint is taken to
/// stand at the jaw's angle, as the atlas nearly does. The radius every head
/// gets whose folded bones say less — EW's eyes and eyebrows, a bare head —
/// and close to what a crown end bone gives.
pub const HEAD_FROM_HEIGHT: f64 = 0.5 * 203.3 / (1741.0 - 203.3);

/// A direction in the body, resolved for one joint.
#[derive(Clone, Copy)]
enum Towards {
    Forward,
    Backward,
    Down,
    /// Away from the body's midline on the joint's side.
    Lateral,
    /// The body's left, whatever the side: a symmetric range's axis.
    Left,
    /// Into the joint's hand's palm.
    Palm,
    /// Towards its hand's thumb side.
    Radial,
    /// Across its hand's palm towards the little finger: a thumb's bend.
    ThumbBend,
}

/// Where a ball's anatomical zero points.
#[derive(Clone, Copy)]
enum Zero {
    /// Straight down: a limb hanging at the body's side.
    Down,
    /// Along the bone in the bind pose.
    Rest,
}

/// A joint the profile shapes.
enum Spec {
    Hinge {
        bends: Towards,
        flexion: Range,
        posture: Posture,
    },
    Ball {
        zero: Zero,
        flexes: Towards,
        abducts: Towards,
        flexion: Range,
        abduction: Range,
        /// The twist about the bone, positive turning right-handed about its
        /// far end, as `[least, most]` on the left side; the right's is its
        /// mirror.
        left_twist: Range,
        flexion_posture: Posture,
        abduction_posture: Posture,
    },
}

/// The body's axes in the world, square to each other.
#[derive(Clone, Copy)]
struct Axes {
    forward: DVec3,
    up: DVec3,
    left: DVec3,
}

/// A hand's palm in the world: the way it faces, and its thumb side.
#[derive(Clone, Copy)]
struct Palm {
    facing: DVec3,
    radial: DVec3,
}

/// The profile fitted to one skeleton.
pub(crate) struct Profile {
    roles: Vec<Option<Role>>,
    parents: Vec<Option<usize>>,
    folded: Vec<bool>,
    /// Whether each joint is the pelvis's or below it.
    in_body: Vec<bool>,
    /// Each joint's nearest ancestor left standing.
    kept_parents: Vec<Option<usize>>,
    axes: Option<Axes>,
    palms: Vec<Option<Palm>>,
    spine_bodies: usize,
    cervical_bodies: usize,
}

impl Profile {
    /// The profile for the skeleton whose joints are `names`, with
    /// `parents`, standing at `at` in the bind pose.
    pub(crate) fn new(
        humanoid: &Humanoid,
        names: &[&str],
        parents: &[Option<usize>],
        at: &dyn Fn(usize) -> DVec3,
    ) -> Self {
        let roles = roles(names, parents);
        let pelvis = roles.iter().position(|role| *role == Some(Role::Pelvis));
        // A helper, a face bone, a folded digit, and anything off the
        // pelvis's own tree (a root, its IK targets, a centre of mass) takes
        // everything below it along; a clavicle, a metacarpal or a second
        // neck bone is spliced out and leaves what hangs from it to the bone
        // above.
        // Whether a bone the profile gives a body hangs below each joint: a
        // twist bone with one is in the limb's chain, and is spliced out; one
        // without, whatever unnamed bones hang from it, is folded whole.
        let mut limb_below = vec![false; roles.len()];
        for joint in (0..roles.len()).rev() {
            let bodied = !matches!(
                roles[joint],
                None | Some(Role::Helper | Role::Face | Role::Twist)
            );
            if let Some(parent) = parents[joint] {
                limb_below[parent] |= limb_below[joint] || bodied;
            }
        }
        let twist = |joint: usize| roles[joint] == Some(Role::Twist);
        let mut in_body = vec![pelvis.is_none(); roles.len()];
        let mut whole = vec![false; roles.len()];
        for joint in 0..roles.len() {
            let parent = parents[joint];
            in_body[joint] |= Some(joint) == pelvis || parent.is_some_and(|p| in_body[p]);
            let own = match roles[joint] {
                Some(Role::Helper | Role::Face) => true,
                Some(Role::Twist) => !limb_below[joint],
                Some(Role::Finger(..) | Role::Toe(_)) => humanoid.fold_digits,
                _ => false,
            };
            // Everything past the head — face bones, a jaw, an end bone — is
            // the head's: it is one sphere.
            whole[joint] = own || parent.is_some_and(|p| whole[p] || roles[p] == Some(Role::Head));
        }
        // A neck bone below another: Unreal 5's `neck_02`.
        let upper_neck = |joint: usize| {
            roles[joint] == Some(Role::Neck)
                && parents[joint].is_some_and(|p| roles[p] == Some(Role::Neck))
        };
        let folded: Vec<bool> = (0..roles.len())
            .map(|joint| {
                whole[joint]
                    || !in_body[joint]
                    || spliced(roles[joint])
                    || upper_neck(joint)
                    || (twist(joint) && limb_below[joint])
            })
            .collect();
        let kept_parents: Vec<Option<usize>> = (0..roles.len())
            .map(|joint| {
                let mut parent = parents[joint];
                while let Some(above) = parent.filter(|&above| folded[above]) {
                    parent = parents[above];
                }
                parent
            })
            .collect();
        let axes = axes(&roles, pelvis, at);
        let palms = (0..roles.len())
            .map(|joint| {
                let Some(Role::Hand(side)) = roles[joint] else {
                    return None;
                };
                palm(&roles, &kept_parents, joint, side, axes?, at)
            })
            .collect();
        Self {
            roles,
            parents: parents.to_vec(),
            folded,
            in_body,
            kept_parents,
            axes,
            palms,
            spine_bodies: 0,
            cervical_bodies: 0,
        }
    }

    /// Whether `joint` carries no body and follows its parent.
    pub(crate) fn is_folded(&self, joint: usize) -> bool {
        self.folded[joint]
    }

    /// The nearest ancestor of `joint` left standing, which its body is
    /// fitted against.
    pub(crate) fn kept_parent(&self, joint: usize) -> Option<usize> {
        self.kept_parents[joint]
    }

    /// The sphere, centre then radius, for a head at `joint`, everything
    /// past which is folded: on the line from its nearest standing ancestor,
    /// as big as [`HEAD_REACH`] of its farthest folded bone or
    /// [`HEAD_FROM_HEIGHT`] of the head joint's height over the sole,
    /// whichever is the greater. `None` for any other joint, which keeps the generator's own
    /// fit.
    pub(crate) fn head_sphere(
        &self,
        joint: usize,
        at: &dyn Fn(usize) -> DVec3,
    ) -> Option<(DVec3, f64)> {
        if self.roles[joint] != Some(Role::Head) {
            return None;
        }
        let up = (at(joint) - at(self.kept_parents[joint]?)).try_normalize()?;
        // The head joint's height over the body's lowest joint, the sole.
        let vertical = self.axes?.up;
        let sole = (0..self.roles.len())
            .filter(|&other| self.in_body[other])
            .map(|other| at(other).dot(vertical))
            .fold(f64::INFINITY, f64::min);
        let height = at(joint).dot(vertical) - sole;
        let below = |mut other: usize| loop {
            match self.parents[other] {
                Some(parent) if parent == joint => break true,
                Some(parent) => other = parent,
                None => break false,
            }
        };
        let reach = (0..self.roles.len())
            .filter(|&other| self.folded[other] && below(other))
            .map(|other| at(other).distance(at(joint)))
            .fold(0.0, f64::max);
        let radius = (HEAD_REACH * reach).max(HEAD_FROM_HEIGHT * height);
        Some((at(joint) + up * radius, radius))
    }

    /// Counts the bodied joints the trunk's ranges are split across.
    pub(crate) fn count_bodies(&mut self, bodied: impl Iterator<Item = usize>) {
        for joint in bodied {
            match self.roles[joint] {
                Some(Role::Spine) => self.spine_bodies += 1,
                Some(Role::Neck | Role::Head) => self.cervical_bodies += 1,
                _ => {}
            }
        }
    }

    /// The joint at `joint`, whose body's rest axis is `axis`: its frame's
    /// world rotation at rest and its kind, or `None` to keep the default
    /// ball — a bone the profile does not name, or whose axes it cannot
    /// read.
    pub(crate) fn joint(
        &self,
        joint: usize,
        axis: DVec3,
        at: &dyn Fn(usize) -> DVec3,
    ) -> Option<(DQuat, RigJointKind)> {
        let role = self.roles[joint]?;
        let axes = self.axes?;
        let spec = self.spec(role)?;
        let side = side_of(role);
        let palm = self.palm_of(joint);
        let towards = |towards: Towards| resolve(towards, axes, side, palm);
        match spec {
            Spec::Hinge {
                bends,
                flexion,
                posture,
            } => {
                let parent = at(joint) - at(self.parents[joint]?);
                hinge(parent, axis, towards(bends)?, flexion, posture)
            }
            Spec::Ball {
                zero,
                flexes,
                abducts,
                flexion,
                abduction,
                left_twist,
                flexion_posture,
                abduction_posture,
            } => {
                let zero = match zero {
                    Zero::Down => -axes.up,
                    Zero::Rest => axis,
                };
                let twist = match side {
                    Some(Side::Right) => range(-left_twist.most, -left_twist.least),
                    _ => left_twist,
                };
                ball(
                    axis,
                    zero,
                    [towards(flexes)?, towards(abducts)?],
                    [flexion, abduction],
                    [flexion_posture, abduction_posture],
                    twist,
                )
            }
        }
    }

    /// The palm of the hand `joint` is, or is on.
    fn palm_of(&self, joint: usize) -> Option<Palm> {
        let mut at = Some(joint);
        while let Some(joint) = at {
            if let Some(Role::Hand(_)) = self.roles[joint] {
                return self.palms[joint];
            }
            at = self.parents[joint];
        }
        None
    }

    /// The ranges for `role`: see [`Humanoid`] for each value's source.
    fn spec(&self, role: Role) -> Option<Spec> {
        let spine = self.spine_bodies.max(1) as f64;
        let cervical = self.cervical_bodies.max(1) as f64;
        let ball =
            |zero, flexes, abducts, flexion, abduction, left_twist, postures: [Posture; 2]| {
                Spec::Ball {
                    zero,
                    flexes,
                    abducts,
                    flexion,
                    abduction,
                    left_twist,
                    flexion_posture: postures[0],
                    abduction_posture: postures[1],
                }
            };
        let hinge = |bends, flexion, posture| Spec::Hinge {
            bends,
            flexion,
            posture,
        };
        Some(match role {
            // The pelvis has no joint above it; the rest are folded.
            Role::Pelvis
            | Role::Helper
            | Role::Twist
            | Role::Face
            | Role::Clavicle(_)
            | Role::Metacarpal(_) => {
                return None;
            }
            // Thoracolumbar (38 CFR §4.71a): flexion 90, extension 30,
            // lateral flexion and rotation 30 each way.
            Role::Spine => ball(
                Zero::Rest,
                Towards::Forward,
                Towards::Left,
                range(-30.0 / spine, 90.0 / spine),
                range(-30.0 / spine, 30.0 / spine),
                range(-30.0 / spine, 30.0 / spine),
                [AT_ZERO; 2],
            ),
            // Cervical (38 CFR §4.71a): 45 each way, rotation 80; NASA's
            // 24° forward head and neck.
            Role::Neck | Role::Head => ball(
                Zero::Rest,
                Towards::Forward,
                Towards::Left,
                range(-45.0 / cervical, 45.0 / cervical),
                range(-45.0 / cervical, 45.0 / cervical),
                range(-80.0 / cervical, 80.0 / cervical),
                [posture(24.0 / cervical, 5.0 / cervical), AT_ZERO],
            ),
            // Flexion 180 and abduction 180 (CFR), extension 50 and
            // adduction 30 (DSHS), rotation 90 each way (CFR); NASA's 36°
            // of flexion. Flexion and abduction both end overhead, one swing,
            // and no ellipse passes through two stops at one swing: the
            // profile stops both `SHOULDER_SHORT_OF_OVERHEAD` short of it.
            Role::UpperArm(_) => ball(
                Zero::Down,
                Towards::Forward,
                Towards::Lateral,
                range(-50.0, 180.0 - SHOULDER_SHORT_OF_OVERHEAD),
                range(-30.0, 180.0 - SHOULDER_SHORT_OF_OVERHEAD),
                range(-90.0, 90.0),
                [posture(36.0, 19.0), AT_ZERO],
            ),
            // Flexion 145 (CFR), none past straight; NASA's 58°.
            Role::LowerArm(_) => hinge(Towards::Forward, range(0.0, 145.0), posture(58.0, 24.0)),
            // Flexion 80, extension 70, radial 20, ulnar 45 (CFR). The
            // forearm is one hinged body, so the wrist's twist is its
            // pronation, 80, and supination, 85 (CFR): pronation turns a
            // left forearm right-handed about its far end.
            Role::Hand(_) => ball(
                Zero::Rest,
                Towards::Palm,
                Towards::Radial,
                range(-70.0, 80.0),
                range(-45.0, 20.0),
                range(-85.0, 80.0),
                [AT_ZERO; 2],
            ),
            // No source for a thumb's base: an engineering 20° each way and
            // 15° of turn.
            Role::Finger(_, Digit::Thumb, 0) => ball(
                Zero::Rest,
                Towards::Palm,
                Towards::Radial,
                range(-20.0, 20.0),
                range(-20.0, 20.0),
                range(-15.0, 15.0),
                [AT_ZERO; 2],
            ),
            // Thumb metacarpophalangeal 60, interphalangeal 80 (DSHS).
            Role::Finger(_, Digit::Thumb, 1) => {
                hinge(Towards::ThumbBend, range(0.0, 60.0), AT_ZERO)
            }
            Role::Finger(_, Digit::Thumb, _) => {
                hinge(Towards::ThumbBend, range(0.0, 80.0), AT_ZERO)
            }
            // Metacarpophalangeal 90, proximal 100, distal 70 (38 CFR
            // §4.71a).
            Role::Finger(_, _, 0) => hinge(Towards::Palm, range(0.0, 90.0), AT_ZERO),
            Role::Finger(_, _, 1) => hinge(Towards::Palm, range(0.0, 100.0), AT_ZERO),
            Role::Finger(..) => hinge(Towards::Palm, range(0.0, 70.0), AT_ZERO),
            // Flexion 125 and abduction 45 (CFR), extension 30 and
            // adduction 20 (DSHS); no source for its rotation, an
            // engineering 30° each way; NASA's 52° of flexion and 12° of
            // abduction.
            Role::Thigh(_) => ball(
                Zero::Down,
                Towards::Forward,
                Towards::Lateral,
                range(-30.0, 125.0),
                range(-20.0, 45.0),
                range(-30.0, 30.0),
                [posture(52.0, 7.0), posture(12.0, 6.0)],
            ),
            // Flexion 140 (CFR), none past straight; NASA's 47°.
            Role::Calf(_) => hinge(Towards::Backward, range(0.0, 140.0), posture(47.0, 8.0)),
            // Plantar flexion 45 and dorsiflexion 20 (CFR); inversion 30 and
            // eversion 20 (DSHS) as the twist about the foot, eversion
            // turning a left foot right-handed about its toes; no source for
            // toeing in and out, an engineering 10° each way. NASA's ankle,
            // converted: 26° of plantar flexion.
            Role::Foot(_) => ball(
                Zero::Rest,
                Towards::Down,
                Towards::Lateral,
                range(-20.0, 45.0),
                range(-10.0, 10.0),
                range(-30.0, 20.0),
                [posture(26.0, 6.0), AT_ZERO],
            ),
            // No source for the toes: an engineering 40° up and 30° down at
            // the ball, 50° down beyond it.
            Role::Ball(_) => hinge(Towards::Down, range(-40.0, 30.0), AT_ZERO),
            Role::Toe(_) => hinge(Towards::Down, range(0.0, 50.0), AT_ZERO),
        })
    }
}

/// Whether a bone of `role` is spliced out — no body, what hangs from it
/// fitted to the bone above — rather than folded with all below it: a
/// clavicle, whose motion the shoulder's ranges already hold, and a
/// metacarpal, which is the hand's.
const fn spliced(role: Option<Role>) -> bool {
    matches!(role, Some(Role::Clavicle(_) | Role::Metacarpal(_)))
}

/// The side a role names, if any.
const fn side_of(role: Role) -> Option<Side> {
    match role {
        Role::Clavicle(side)
        | Role::Metacarpal(side)
        | Role::UpperArm(side)
        | Role::LowerArm(side)
        | Role::Hand(side)
        | Role::Finger(side, ..)
        | Role::Thigh(side)
        | Role::Calf(side)
        | Role::Foot(side)
        | Role::Ball(side)
        | Role::Toe(side) => Some(side),
        _ => None,
    }
}

/// `towards` in the world for a joint on `side`, of a hand with `palm`.
fn resolve(towards: Towards, axes: Axes, side: Option<Side>, palm: Option<Palm>) -> Option<DVec3> {
    Some(match towards {
        Towards::Forward => axes.forward,
        Towards::Backward => -axes.forward,
        Towards::Down => -axes.up,
        Towards::Lateral => axes.left * side?.sign(),
        Towards::Left => axes.left,
        Towards::Palm => palm?.facing,
        Towards::Radial => palm?.radial,
        Towards::ThumbBend => {
            let palm = palm?;
            (palm.facing - palm.radial).normalize()
        }
    })
}

/// The body's axes, from the hip line (or the shoulders) and the trunk.
fn axes(
    roles: &[Option<Role>],
    pelvis: Option<usize>,
    at: &dyn Fn(usize) -> DVec3,
) -> Option<Axes> {
    let find = |role: Role| roles.iter().position(|r| *r == Some(role));
    let pair = |left: Role, right: Role| Some((at(find(left)?), at(find(right)?)));
    let (left_at, right_at) = pair(Role::Thigh(Side::Left), Role::Thigh(Side::Right))
        .or_else(|| pair(Role::UpperArm(Side::Left), Role::UpperArm(Side::Right)))?;
    let base = pelvis.map_or_else(
        || {
            let thighs = pair(Role::Thigh(Side::Left), Role::Thigh(Side::Right))?;
            Some(0.5 * (thighs.0 + thighs.1))
        },
        |pelvis| Some(at(pelvis)),
    )?;
    let top = find(Role::Head)
        .or_else(|| find(Role::Neck))
        .or_else(|| roles.iter().rposition(|r| *r == Some(Role::Spine)))?;
    let left = left_at - right_at;
    let up = at(top) - base;
    let forward = left.cross(up).try_normalize()?;
    let up = forward.cross(left).try_normalize()?;
    Some(Axes {
        forward,
        up,
        left: up.cross(forward),
    })
}

/// The palm of the hand at `hand` on `side`: facing across its index and
/// farthest other finger's knuckles — found through any metacarpal, as the
/// fingers' nearest ancestors left standing are `kept_parents` — or down,
/// square to the hand, without.
fn palm(
    roles: &[Option<Role>],
    parents: &[Option<usize>],
    hand: usize,
    side: Side,
    axes: Axes,
    at: &dyn Fn(usize) -> DVec3,
) -> Option<Palm> {
    let knuckle = |digit: Digit| {
        (0..roles.len()).filter(move |&joint| {
            parents[joint] == Some(hand) && roles[joint] == Some(Role::Finger(side, digit, 0))
        })
    };
    let wrist = at(hand);
    let sign = side.sign();
    if let Some(index) = knuckle(Digit::Index).next() {
        let other = knuckle(Digit::Other).max_by(|&a, &b| {
            at(a)
                .distance_squared(at(index))
                .total_cmp(&at(b).distance_squared(at(index)))
        });
        if let Some(other) = other {
            let along = (at(index) - wrist).try_normalize()?;
            let across = at(index) - at(other);
            let radial = (across - along * along.dot(across)).try_normalize()?;
            let facing = (along.cross(radial) * sign).try_normalize()?;
            return Some(Palm { facing, radial });
        }
    }
    let along = (wrist - at(parents[hand]?)).try_normalize()?;
    let down = -axes.up;
    let facing = (down - along * along.dot(down)).try_normalize()?;
    let radial = (facing.cross(along) * sign).try_normalize()?;
    Some(Palm { facing, radial })
}

/// How far, as a sine, a bone must stand from the way it bends for that way
/// to name an axis: about six degrees.
const SQUARE_ENOUGH: f64 = 0.1;

/// How far, in degrees, a bind may stand past a hinge's stop on extension
/// and have the stop moved to it rather than be snapped inside: a model's
/// elbow or finger bound a degree or two past straight is its own rest, as
/// is the rounding of an `f32` skeleton's straight knee. A bind farther past
/// is bent wrong, and stays outside for the editor to warn of.
pub const BIND_SLACK: f64 = 5.0;

/// A hinge at a bone along `axis`, whose parent bone runs `parent`, bending
/// towards `bends`: its frame's rotation, z the bend axis, and its kind.
fn hinge(
    parent: DVec3,
    axis: DVec3,
    bends: DVec3,
    flexion: Range,
    posture: Posture,
) -> Option<(DQuat, RigJointKind)> {
    let bend_axis = axis.cross(bends);
    if bend_axis.length() < SQUARE_ENOUGH {
        return None;
    }
    let bend_axis = bend_axis.normalize();
    let parent = parent.try_normalize()?;
    // The bind's own bend, positive the way the joint flexes.
    let rest = atan2(parent.cross(axis).dot(bend_axis), parent.dot(axis));
    let [least, most, mean, spread] =
        [flexion.least, flexion.most, posture.mean, posture.spread].map(f64::to_radians);
    let least = if rest < least && least - rest <= BIND_SLACK.to_radians() {
        rest
    } else {
        least
    };
    let neutral = mean - rest;
    let (lower, upper) = (least - mean, most - mean);
    let (free_lower, free_upper) = (lower.max(-spread), upper.min(spread));
    let soft = SoftRange {
        lower: free_lower + SOFT_FRACTION * (lower - free_lower),
        upper: free_upper + SOFT_FRACTION * (upper - free_upper),
        hertz: ZONE.hertz,
        damping_ratio: ZONE.damping_ratio,
    };
    if !(-PI..=PI).contains(&neutral) {
        return None;
    }
    let rotation = joint_rotation(bend_axis, Some(axis))?;
    Some((
        rotation,
        RigJointKind::Hinge {
            lower,
            upper,
            neutral,
            soft: Some(soft),
        },
    ))
}

/// The swing ellipse through a ball's `stops`, abduction turning about the
/// frame's y with `sign`.
fn swing_ellipse(stops: [Range; 2], sign: f64) -> Option<Ellipse> {
    let radians = f64::to_radians;
    Ellipse::through([
        [radians(stops[0].most), 0.0],
        [radians(stops[0].least), 0.0],
        [0.0, sign * radians(stops[1].most)],
        [0.0, sign * radians(stops[1].least)],
    ])
}

/// The soft ellipse's size as a fraction of `ellipse`'s: just holding the
/// free zone — NASA's posture box, mean ± spread on both axes, or the
/// anatomical zero — then [`SOFT_FRACTION`] of the way out to the hard one.
fn soft_scale(ellipse: &Ellipse, postures: [Posture; 2], sign: f64) -> f64 {
    let radians = f64::to_radians;
    let [flexed, abducted] = postures;
    let free = [-1.0, 1.0]
        .into_iter()
        .flat_map(|a| [-1.0, 1.0].map(|b| (a, b)))
        .map(|(a, b)| {
            ellipse.radius([
                radians(flexed.mean + a * flexed.spread),
                sign * radians(abducted.mean + b * abducted.spread),
            ])
        })
        .fold(0.0, f64::max);
    if free >= 1.0 {
        1.0
    } else {
        free + SOFT_FRACTION * (1.0 - free)
    }
}

/// The shortest swing taking the direction `from` onto `to`, as a rotation
/// vector, or `None` for opposite directions, which have no shortest one.
fn shortest_swing(from: DVec3, to: DVec3) -> Option<DVec3> {
    let across = from.cross(to);
    let sine = across.length();
    if sine > 1e-12 {
        Some(across * (atan2(sine, from.dot(to)) / sine))
    } else if from.dot(to) > 0.0 {
        Some(DVec3::ZERO)
    } else {
        None
    }
}

/// A ball at a bone along `axis`, whose anatomical zero points `zero`,
/// flexing and abducting towards `towards`: its frame's rotation at rest —
/// z along the bone, x rolled so that, tipped by the neutral swing, it is
/// the flexion axis carried to the ellipse's centre — and its kind. The
/// swing ellipse passes through the four stops of `stops` (see
/// [`Ellipse`]); `None`, keeping the default ball, where none does.
fn ball(
    axis: DVec3,
    zero: DVec3,
    towards: [DVec3; 2],
    stops: [Range; 2],
    postures: [Posture; 2],
    twist: Range,
) -> Option<(DQuat, RigJointKind)> {
    let flexion_axis = zero.cross(towards[0]).try_normalize()?;
    let abduction_axis = zero.cross(flexion_axis);
    // Whether abducting turns about the frame's y or against it.
    let sign = if zero.cross(towards[1]).dot(abduction_axis) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let ellipse = swing_ellipse(stops, sign)?;
    let soft = ellipse.scaled_half(soft_scale(&ellipse, postures, sign));
    let to_middle = rotation_from_scaled_axis(
        flexion_axis * ellipse.centre[0] + abduction_axis * ellipse.centre[1],
    );
    let centre = to_middle * zero;
    let centre_x = to_middle * flexion_axis;
    let swing = shortest_swing(axis, centre)?;
    let x_at_rest = rotation_from_scaled_axis(-swing) * centre_x;
    let rotation = joint_rotation(axis, Some(x_at_rest))?;
    let local = rotation.inverse() * swing;
    // The frame's turn about the bone once the bone is swung the shortest
    // way from rest to its anatomical zero, against the anatomical frame
    // there. Shortest swings do not compose — rest to centre is not rest to
    // zero then zero to centre — so a bind that is not the anatomical zero
    // (an arm in a T-pose or an A-pose) carries a twist there that depends
    // on the bind; the neutral twists the child's frame back by it, so the
    // twist limits are measured from the anatomical zero whatever the bind.
    let at_zero = joint_rotation(zero, Some(flexion_axis))?.inverse()
        * rotation_from_scaled_axis(shortest_swing(axis, zero)?)
        * rotation;
    let at_zero = if at_zero.w < 0.0 { -at_zero } else { at_zero };
    let twist_at_zero = 2.0 * atan2(at_zero.z, at_zero.w);
    let [least, most] = [twist.least, twist.most].map(f64::to_radians);
    Some((
        rotation,
        RigJointKind::Ball {
            swing_x: ellipse.half[0],
            swing_y: ellipse.half[1],
            twist_lower: least,
            twist_upper: most,
            neutral: SwingTwist {
                swing_x: local.x,
                swing_y: local.y,
                twist: twist_at_zero,
            },
            soft_swing: Some(SoftSwing {
                x: soft[0],
                y: soft[1],
                hertz: ZONE.hertz,
                damping_ratio: ZONE.damping_ratio,
            }),
            soft_twist: Some(SoftRange {
                lower: SOFT_FRACTION * least,
                upper: SOFT_FRACTION * most,
                hertz: ZONE.hertz,
                damping_ratio: ZONE.damping_ratio,
            }),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A standing thigh starts in its soft zone and a hanging arm in its
    /// free one**, as the docs say: the anatomical zero is outside the hip's
    /// soft ellipse, 1.12 times out, and inside the shoulder's, 0.92 of the
    /// way, on either side.
    #[test]
    fn a_standing_hip_is_resisted_and_a_hanging_arm_is_free() {
        let hip = [range(-30.0, 125.0), range(-20.0, 45.0)];
        let hip_posture = [posture(52.0, 7.0), posture(12.0, 6.0)];
        let shoulder = [
            range(-50.0, 180.0 - SHOULDER_SHORT_OF_OVERHEAD),
            range(-30.0, 180.0 - SHOULDER_SHORT_OF_OVERHEAD),
        ];
        let shoulder_posture = [posture(36.0, 19.0), AT_ZERO];
        for (stops, postures, out) in [(hip, hip_posture, 1.12), (shoulder, shoulder_posture, 0.92)]
        {
            for sign in [1.0, -1.0] {
                let ellipse = swing_ellipse(stops, sign).expect("a fit");
                let zero = ellipse.radius([0.0, 0.0]) / soft_scale(&ellipse, postures, sign);
                assert!(
                    (zero - out).abs() < 0.005,
                    "the zero is {zero} of the soft ellipse, not {out}"
                );
            }
        }
    }
}
