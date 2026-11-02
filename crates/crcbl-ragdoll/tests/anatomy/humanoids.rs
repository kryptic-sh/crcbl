//! The humanoids the anatomy tests fit rigs to, in centimetres, T-posed
//! facing `+z` with their left at `+x`, and the helpers that build and read
//! their rigs. One shape under three namings: EW's skeleton of 2026-11-01
//! (with an Unreal `root` above it, which EW's list does not name), Mixamo's,
//! and Unreal 5's mannequin with its helpers. Any can be A-posed, its arms
//! dropped from level.

use crcbl_anim::{Joint, Skeleton, Trs};
use crcbl_ragdoll::{GenerateSettings, Humanoid, RagdollRig, RigJoint, RigJointKind};
use glam::{Mat4, Quat, Vec3};

/// One bone: its name, parent and offset from its parent at rest.
pub type Bone = (String, Option<usize>, [f32; 3]);

/// Appends a bone, returning its index.
pub fn add(
    bones: &mut Vec<Bone>,
    name: impl Into<String>,
    parent: Option<usize>,
    at: [f32; 3],
) -> usize {
    bones.push((name.into(), parent, at));
    bones.len() - 1
}

/// `offset` on a left arm (`x` of `1`) or right (`-1`) dropped `drop`
/// degrees from level towards the body's side, about `z`.
pub fn arm(offset: [f32; 3], x: f32, drop: f32) -> [f32; 3] {
    let turned = Quat::from_rotation_z(-x * drop.to_radians()) * Vec3::from(offset);
    turned.into()
}

/// The humanoid named as EW's skeleton, its arms dropped `drop` degrees.
pub fn ew(drop: f32) -> Vec<Bone> {
    let mut bones = Vec::new();
    let b = &mut bones;
    let root = add(b, "root", None, [0.0; 3]);
    let ik_foot = add(b, "ik_foot_root", Some(root), [0.0; 3]);
    add(b, "ik_foot_l", Some(ik_foot), [9.0, 8.0, 0.0]);
    add(b, "ik_foot_r", Some(ik_foot), [-9.0, 8.0, 0.0]);
    let ik_hand = add(b, "ik_hand_root", Some(root), [0.0; 3]);
    let gun = add(b, "ik_hand_gun", Some(ik_hand), [-70.0, 145.0, 0.0]);
    add(b, "ik_hand_l", Some(gun), [140.0, 0.0, 0.0]);
    add(b, "ik_hand_r", Some(gun), [0.0; 3]);
    let pelvis = add(b, "Pelvis", Some(root), [0.0, 95.0, 0.0]);
    let mut spine = pelvis;
    for (k, rise) in [10.0, 12.0, 14.0].into_iter().enumerate() {
        spine = add(
            b,
            format!("spine_0{}", k + 1),
            Some(spine),
            [0.0, rise, 0.0],
        );
    }
    let neck = add(b, "neck_01", Some(spine), [0.0, 14.0, 0.0]);
    let head = add(b, "head", Some(neck), [0.0, 10.0, 0.0]);
    add(b, "eyebrows", Some(head), [0.0, 10.0, 9.0]);
    add(b, "eyes", Some(head), [0.0, 7.0, 8.0]);
    for (side, upper_side, x) in [("l", "L", 1.0_f32), ("r", "R", -1.0)] {
        let clavicle = add(
            b,
            format!("clavicle_{side}"),
            Some(spine),
            [3.0 * x, 10.0, 0.0],
        );
        let upper = add(
            b,
            format!("UpperArm_{upper_side}"),
            Some(clavicle),
            [15.0 * x, 0.0, 0.0],
        );
        let lower = add(
            b,
            format!("lowerarm_{side}"),
            Some(upper),
            arm([28.0 * x, 0.0, 0.0], x, drop),
        );
        let hand = add(
            b,
            format!("Hand_{upper_side}"),
            Some(lower),
            arm([25.0 * x, 0.0, 0.0], x, drop),
        );
        let digits: [(&str, usize, [f32; 3], [f32; 3]); 3] = [
            ("finger", 4, [9.0, 0.0, -1.5], [4.6, 4.3, 2.0]),
            ("indexFinger", 4, [9.0, 0.0, 2.5], [4.6, 4.3, 2.0]),
            ("thumb", 3, [3.0, -1.5, 3.0], [4.4, 4.2, 0.0]),
        ];
        for (digit, count, base, lengths) in digits {
            let mut joint = add(
                b,
                format!("{digit}_01_{side}"),
                Some(hand),
                arm([base[0] * x, base[1], base[2]], x, drop),
            );
            for k in 1..count {
                joint = add(
                    b,
                    format!("{digit}_0{}_{side}", k + 1),
                    Some(joint),
                    arm([lengths[k - 1] * x, 0.0, 0.0], x, drop),
                );
            }
        }
    }
    for (side, upper_side, x) in [("l", "L", 1.0_f32), ("r", "R", -1.0)] {
        let thigh = add(
            b,
            format!("Thigh_{upper_side}"),
            Some(pelvis),
            [9.0 * x, -5.0, 0.0],
        );
        let calf = add(b, format!("calf_{side}"), Some(thigh), [0.0, -42.0, 0.0]);
        let foot = add(
            b,
            format!("Foot_{upper_side}"),
            Some(calf),
            [0.0, -40.0, 0.0],
        );
        let ball = add(b, format!("ball_{side}"), Some(foot), [0.0, -7.0, 12.0]);
        add(b, format!("toes_{side}"), Some(ball), [0.0, 0.0, 5.0]);
    }
    bones
}

/// The same humanoid named as Mixamo's, with five fingers, a head end and
/// eyes, and no helpers.
pub fn mixamo(drop: f32) -> Vec<Bone> {
    let mut bones = Vec::new();
    let b = &mut bones;
    let name = |bone: &str| format!("mixamorig:{bone}");
    let hips = add(b, name("Hips"), None, [0.0, 95.0, 0.0]);
    let mut spine = hips;
    for (bone, rise) in [("Spine", 10.0), ("Spine1", 12.0), ("Spine2", 14.0)] {
        spine = add(b, name(bone), Some(spine), [0.0, rise, 0.0]);
    }
    let neck = add(b, name("Neck"), Some(spine), [0.0, 14.0, 0.0]);
    let head = add(b, name("Head"), Some(neck), [0.0, 10.0, 0.0]);
    add(b, name("HeadTop_End"), Some(head), [0.0, 20.0, 0.0]);
    add(b, name("LeftEye"), Some(head), [3.0, 7.0, 8.0]);
    add(b, name("RightEye"), Some(head), [-3.0, 7.0, 8.0]);
    for (side, x) in [("Left", 1.0_f32), ("Right", -1.0)] {
        let shoulder = add(
            b,
            name(&format!("{side}Shoulder")),
            Some(spine),
            [3.0 * x, 10.0, 0.0],
        );
        let upper = add(
            b,
            name(&format!("{side}Arm")),
            Some(shoulder),
            [15.0 * x, 0.0, 0.0],
        );
        let lower = add(
            b,
            name(&format!("{side}ForeArm")),
            Some(upper),
            arm([28.0 * x, 0.0, 0.0], x, drop),
        );
        let hand = add(
            b,
            name(&format!("{side}Hand")),
            Some(lower),
            arm([25.0 * x, 0.0, 0.0], x, drop),
        );
        let fingers: [(&str, [f32; 3], [f32; 3]); 5] = [
            ("Thumb", [3.0, -1.5, 3.0], [4.4, 4.2, 2.0]),
            ("Index", [9.0, 0.0, 2.5], [4.6, 4.3, 2.0]),
            ("Middle", [9.5, 0.0, 0.5], [4.8, 4.4, 2.0]),
            ("Ring", [9.0, 0.0, -1.5], [4.6, 4.3, 2.0]),
            ("Pinky", [8.0, 0.0, -3.2], [4.3, 4.1, 2.0]),
        ];
        for (finger, base, lengths) in fingers {
            let mut joint = add(
                b,
                name(&format!("{side}Hand{finger}1")),
                Some(hand),
                arm([base[0] * x, base[1], base[2]], x, drop),
            );
            for (k, length) in lengths.into_iter().enumerate() {
                joint = add(
                    b,
                    name(&format!("{side}Hand{finger}{}", k + 2)),
                    Some(joint),
                    arm([length * x, 0.0, 0.0], x, drop),
                );
            }
        }
    }
    for (side, x) in [("Left", 1.0_f32), ("Right", -1.0)] {
        let thigh = add(
            b,
            name(&format!("{side}UpLeg")),
            Some(hips),
            [9.0 * x, -5.0, 0.0],
        );
        let calf = add(
            b,
            name(&format!("{side}Leg")),
            Some(thigh),
            [0.0, -42.0, 0.0],
        );
        let foot = add(
            b,
            name(&format!("{side}Foot")),
            Some(calf),
            [0.0, -40.0, 0.0],
        );
        let ball = add(
            b,
            name(&format!("{side}ToeBase")),
            Some(foot),
            [0.0, -7.0, 12.0],
        );
        add(
            b,
            name(&format!("{side}Toe_End")),
            Some(ball),
            [0.0, 0.0, 5.0],
        );
    }
    bones
}

/// The same humanoid named as Unreal 5's mannequin: a five-bone spine, two
/// neck bones, a head with no bone beyond it, twist and corrective bones on
/// every long limb bone, metacarpals, a `center_of_mass` beside the pelvis,
/// and the IK helpers under the root.
pub fn unreal(drop: f32) -> Vec<Bone> {
    let mut bones = Vec::new();
    let b = &mut bones;
    let root = add(b, "root", None, [0.0; 3]);
    add(b, "center_of_mass", Some(root), [0.0, 98.0, 2.0]);
    let ik_foot = add(b, "ik_foot_root", Some(root), [0.0; 3]);
    add(b, "ik_foot_l", Some(ik_foot), [9.0, 8.0, 0.0]);
    add(b, "ik_foot_r", Some(ik_foot), [-9.0, 8.0, 0.0]);
    let pelvis = add(b, "pelvis", Some(root), [0.0, 95.0, 0.0]);
    let mut spine = pelvis;
    for (k, rise) in [7.0, 7.0, 7.0, 7.0, 8.0].into_iter().enumerate() {
        spine = add(
            b,
            format!("spine_0{}", k + 1),
            Some(spine),
            [0.0, rise, 0.0],
        );
    }
    let neck = add(b, "neck_01", Some(spine), [0.0, 7.0, 0.0]);
    let neck = add(b, "neck_02", Some(neck), [0.0, 7.0, 0.0]);
    add(b, "head", Some(neck), [0.0, 10.0, 0.0]);
    for (side, x) in [("l", 1.0_f32), ("r", -1.0)] {
        let clavicle = add(
            b,
            format!("clavicle_{side}"),
            Some(spine),
            [3.0 * x, 10.0, 0.0],
        );
        let upper = add(
            b,
            format!("upperarm_{side}"),
            Some(clavicle),
            [15.0 * x, 0.0, 0.0],
        );
        add(
            b,
            format!("upperarm_twist_01_{side}"),
            Some(upper),
            arm([14.0 * x, 0.0, 0.0], x, drop),
        );
        add(
            b,
            format!("upperarm_correctiveRoot_{side}"),
            Some(upper),
            [0.0; 3],
        );
        let lower = add(
            b,
            format!("lowerarm_{side}"),
            Some(upper),
            arm([28.0 * x, 0.0, 0.0], x, drop),
        );
        add(
            b,
            format!("lowerarm_twist_01_{side}"),
            Some(lower),
            arm([12.0 * x, 0.0, 0.0], x, drop),
        );
        let hand = add(
            b,
            format!("hand_{side}"),
            Some(lower),
            arm([25.0 * x, 0.0, 0.0], x, drop),
        );
        let thumb = add(
            b,
            format!("thumb_01_{side}"),
            Some(hand),
            arm([3.0 * x, -1.5, 3.0], x, drop),
        );
        let thumb = add(
            b,
            format!("thumb_02_{side}"),
            Some(thumb),
            arm([4.4 * x, 0.0, 0.0], x, drop),
        );
        let thumb = add(
            b,
            format!("thumb_03_{side}"),
            Some(thumb),
            arm([4.2 * x, 0.0, 0.0], x, drop),
        );
        add(
            b,
            format!("thumb_04_leaf_{side}"),
            Some(thumb),
            arm([2.0 * x, 0.0, 0.0], x, drop),
        );
        let fingers: [(&str, [f32; 3], [f32; 3]); 4] = [
            ("index", [9.0, 0.0, 2.5], [4.6, 4.3, 2.0]),
            ("middle", [9.5, 0.0, 0.5], [4.8, 4.4, 2.0]),
            ("ring", [9.0, 0.0, -1.5], [4.6, 4.3, 2.0]),
            ("pinky", [8.0, 0.0, -3.2], [4.3, 4.1, 2.0]),
        ];
        for (finger, base, lengths) in fingers {
            // The metacarpal from the wrist two thirds of the way out.
            let metacarpal = add(
                b,
                format!("{finger}_metacarpal_{side}"),
                Some(hand),
                arm([base[0] / 3.0 * x, base[1] / 3.0, base[2] / 3.0], x, drop),
            );
            let mut joint = add(
                b,
                format!("{finger}_01_{side}"),
                Some(metacarpal),
                arm(
                    [
                        base[0] * 2.0 / 3.0 * x,
                        base[1] * 2.0 / 3.0,
                        base[2] * 2.0 / 3.0,
                    ],
                    x,
                    drop,
                ),
            );
            for (k, length) in lengths.into_iter().enumerate() {
                joint = add(
                    b,
                    format!("{finger}_0{}_{side}", k + 2),
                    Some(joint),
                    arm([length * x, 0.0, 0.0], x, drop),
                );
            }
        }
    }
    for (side, x) in [("l", 1.0_f32), ("r", -1.0)] {
        let thigh = add(
            b,
            format!("thigh_{side}"),
            Some(pelvis),
            [9.0 * x, -5.0, 0.0],
        );
        add(
            b,
            format!("thigh_twist_01_{side}"),
            Some(thigh),
            [0.0, -20.0, 0.0],
        );
        add(
            b,
            format!("thigh_correctiveRoot_{side}"),
            Some(thigh),
            [0.0, -2.0, 1.0],
        );
        let calf = add(b, format!("calf_{side}"), Some(thigh), [0.0, -42.0, 0.0]);
        add(
            b,
            format!("calf_twist_01_{side}"),
            Some(calf),
            [0.0, -20.0, 0.0],
        );
        let foot = add(b, format!("foot_{side}"), Some(calf), [0.0, -40.0, 0.0]);
        let ball = add(b, format!("ball_{side}"), Some(foot), [0.0, -7.0, 12.0]);
        add(b, format!("ball_leaf_{side}"), Some(ball), [0.0, 0.0, 5.0]);
    }
    bones
}

/// Unreal 5's humanoid with other proportions: a longer neck in a long
/// `neck_01` and a short `neck_02`, and a head with only a facial root just
/// above it — the case a head sized from its neck bone or its face gets
/// wrong.
pub fn long_necked() -> Vec<Bone> {
    let mut bones = unreal(0.0);
    let at = |bones: &[Bone], name: &str| bones.iter().position(|b| b.0 == name).expect(name);
    let first = at(&bones, "neck_01");
    let second = at(&bones, "neck_02");
    let head = at(&bones, "head");
    bones[first].2 = [0.0, 10.0, 0.0];
    bones[second].2 = [0.0, 3.0, 0.0];
    bones[head].2 = [0.0, 6.0, 0.0];
    add(
        &mut bones,
        "FACIAL_C_FacialRoot",
        Some(head),
        [0.0, 3.0, 4.0],
    );
    bones
}

/// The skeleton of `bones`, its inverse bind matrices from the rest pose.
pub fn skeleton(bones: &[Bone]) -> Skeleton {
    let mut globals: Vec<Mat4> = Vec::with_capacity(bones.len());
    let joints = bones
        .iter()
        .map(|&(_, parent, offset)| {
            let rest = Trs {
                translation: Vec3::from(offset),
                ..Trs::IDENTITY
            };
            let global = parent.map_or(Mat4::IDENTITY, |p| globals[p]) * rest.to_mat4();
            globals.push(global);
            Joint {
                parent,
                inverse_bind: global.inverse(),
                rest,
            }
        })
        .collect();
    Skeleton::new(joints).expect("parents come first")
}

pub fn names(bones: &[Bone]) -> Vec<&str> {
    bones.iter().map(|(name, _, _)| name.as_str()).collect()
}

/// Centimetres to metres, standing `lift` metres up, turned `turn` radians
/// about `y`: the character faces `+z` turned by it.
pub fn model(lift: f32, turn: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(0.0, lift, 0.0))
        * Mat4::from_rotation_y(turn)
        * Mat4::from_scale(Vec3::splat(0.01))
}

/// The settings with the profile on.
pub fn profiled(fold_digits: bool) -> GenerateSettings {
    GenerateSettings {
        anatomy: Some(Humanoid { fold_digits }),
        ..GenerateSettings::DEFAULT
    }
}

pub fn generate(bones: &[Bone], model: Mat4, settings: &GenerateSettings) -> RagdollRig {
    RagdollRig::generate(&skeleton(bones), &names(bones), model, settings).expect("a rig")
}

/// The index of the body on the bone named `name`, if it has one.
pub fn body(rig: &RagdollRig, bones: &[Bone], name: &str) -> Option<usize> {
    rig.bodies()
        .iter()
        .position(|body| bones[body.joint].0 == name)
}

/// The rig joint whose child is the body on `name`.
pub fn joint<'a>(rig: &'a RagdollRig, bones: &[Bone], name: &str) -> &'a RigJoint {
    let child = body(rig, bones, name).unwrap_or_else(|| panic!("a body on {name}"));
    rig.joints()
        .iter()
        .find(|joint| joint.child == child)
        .unwrap_or_else(|| panic!("a joint at {name}"))
}

/// A humanoid's naming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Naming {
    Ew,
    Mixamo,
    Unreal,
}

impl Naming {
    pub const ALL: [Self; 3] = [Self::Ew, Self::Mixamo, Self::Unreal];

    /// The humanoid, its arms dropped `drop` degrees.
    pub fn bones(self, drop: f32) -> Vec<Bone> {
        match self {
            Self::Ew => ew(drop),
            Self::Mixamo => mixamo(drop),
            Self::Unreal => unreal(drop),
        }
    }

    /// Its knees and elbows, by name: thigh, calf, foot, then upper arm,
    /// forearm, hand, left then right.
    pub fn limbs(self) -> [[&'static str; 3]; 4] {
        match self {
            Self::Ew => [
                ["Thigh_L", "calf_l", "Foot_L"],
                ["Thigh_R", "calf_r", "Foot_R"],
                ["UpperArm_L", "lowerarm_l", "Hand_L"],
                ["UpperArm_R", "lowerarm_r", "Hand_R"],
            ],
            Self::Mixamo => [
                [
                    "mixamorig:LeftUpLeg",
                    "mixamorig:LeftLeg",
                    "mixamorig:LeftFoot",
                ],
                [
                    "mixamorig:RightUpLeg",
                    "mixamorig:RightLeg",
                    "mixamorig:RightFoot",
                ],
                [
                    "mixamorig:LeftArm",
                    "mixamorig:LeftForeArm",
                    "mixamorig:LeftHand",
                ],
                [
                    "mixamorig:RightArm",
                    "mixamorig:RightForeArm",
                    "mixamorig:RightHand",
                ],
            ],
            Self::Unreal => [
                ["thigh_l", "calf_l", "foot_l"],
                ["thigh_r", "calf_r", "foot_r"],
                ["upperarm_l", "lowerarm_l", "hand_l"],
                ["upperarm_r", "lowerarm_r", "hand_r"],
            ],
        }
    }

    /// Its head bone's name.
    pub const fn head(self) -> &'static str {
        match self {
            Self::Ew | Self::Unreal => "head",
            Self::Mixamo => "mixamorig:Head",
        }
    }
}

/// `bones` without those whose names `gone` accepts, each kept bone's
/// parent the nearest kept ancestor; a removed bone's offset is added to
/// its children's.
pub fn without(bones: &[Bone], gone: impl Fn(&str) -> bool) -> Vec<Bone> {
    let mut kept = Vec::new();
    let mut new_index: Vec<Option<usize>> = Vec::new();
    let mut offset_to: Vec<[f32; 3]> = Vec::new();
    for (name, parent, at) in bones {
        let carried = parent.map_or([0.0; 3], |p| offset_to[p]);
        let at = [at[0] + carried[0], at[1] + carried[1], at[2] + carried[2]];
        let parent_index = parent.and_then(|p| new_index[p]);
        if gone(name) {
            new_index.push(parent_index);
            offset_to.push(at);
        } else {
            new_index.push(Some(kept.len()));
            offset_to.push([0.0; 3]);
            kept.push((name.clone(), parent_index, at));
        }
    }
    kept
}

/// `rig` with every soft zone taken out: its hard limits alone.
pub fn without_zones(rig: &RagdollRig) -> RagdollRig {
    let joints = rig
        .joints()
        .iter()
        .map(|joint| {
            let kind = match joint.kind {
                RigJointKind::Ball {
                    swing_x,
                    swing_y,
                    twist_lower,
                    twist_upper,
                    neutral,
                    ..
                } => RigJointKind::Ball {
                    swing_x,
                    swing_y,
                    twist_lower,
                    twist_upper,
                    neutral,
                    soft_swing: None,
                    soft_twist: None,
                },
                RigJointKind::Hinge {
                    lower,
                    upper,
                    neutral,
                    ..
                } => RigJointKind::Hinge {
                    lower,
                    upper,
                    neutral,
                    soft: None,
                },
                RigJointKind::Fixed => RigJointKind::Fixed,
            };
            RigJoint { kind, ..*joint }
        })
        .collect();
    RagdollRig::new(
        rig.joint_count(),
        rig.bodies().to_vec(),
        joints,
        rig.ignored().to_vec(),
        rig.substeps(),
    )
    .expect("the same rig, softer")
}
