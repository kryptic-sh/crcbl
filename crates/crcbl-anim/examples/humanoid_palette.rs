//! A humanoid's palette and clip work, one kind at a time, for instruction
//! counts.
//!
//! A skeleton the shape of a shooter's character — hips, a spine to the head,
//! two arms with five three-joint fingers each, two legs — posed and then put
//! through one of the things a character rig does many times a tick:
//!
//! ```text
//! full       compute from a pose whose every joint changed since the last
//! repeat     compute again from a pose nothing changed
//! turns      rotate_joints: a lean spread over two spine joints and the neck
//! two-bone   solve_two_bone: a hand put on a target, alternating between two
//! sample     Clip::sample_into: a walk cycle at advancing times
//! blend      blend_into: a walk pose and a run pose mixed, alternating weights
//! ```
//!
//! The walk and the run are shaped like an exported locomotion clip: a
//! rotation channel on every joint, a translation on the hips and a scale on
//! the hands and feet, linearly interpolated, with the fingers keyed on a
//! timeline of their own and everything else on one shared timeline.
//!
//! ```text
//! cargo build --release -p crcbl-anim --example humanoid_palette
//! valgrind --tool=callgrind target/release/examples/humanoid_palette full 0
//! valgrind --tool=callgrind target/release/examples/humanoid_palette full 100000
//! ```
//!
//! The second argument is how many calls to make. The difference between the
//! two runs' totals, divided by the calls, is what one call costs, with the
//! setup cancelled out.

use std::hint::black_box;

use crcbl_anim::{
    Channel, Clip, Interpolation, Joint, Palette, Pose, Skeleton, Track, Trs, blend_into,
    rotate_joints, solve_two_bone,
};
use glam::{Mat4, Quat, Vec3};

/// The joints, as `(parent, rest offset from it)`, in palette order.
struct Builder {
    joints: Vec<(Option<usize>, Vec3)>,
}

impl Builder {
    fn add(&mut self, parent: Option<usize>, offset: Vec3) -> usize {
        self.joints.push((parent, offset));
        self.joints.len() - 1
    }

    fn arm(&mut self, chest: usize, side: f32) -> [usize; 3] {
        let clavicle = self.add(Some(chest), Vec3::new(side * 0.05, 0.15, 0.0));
        let upper = self.add(Some(clavicle), Vec3::new(side * 0.15, 0.0, 0.0));
        let fore = self.add(Some(upper), Vec3::new(side * 0.28, 0.0, 0.0));
        let hand = self.add(Some(fore), Vec3::new(side * 0.25, 0.0, 0.0));
        for finger in 0..5u8 {
            let spread = (f32::from(finger) - 2.0) * 0.02;
            let mut joint = hand;
            for _ in 0..3 {
                joint = self.add(Some(joint), Vec3::new(side * 0.03, 0.0, spread));
            }
        }
        [upper, fore, hand]
    }

    fn leg(&mut self, hips: usize, side: f32) -> usize {
        let thigh = self.add(Some(hips), Vec3::new(side * 0.1, -0.05, 0.0));
        let calf = self.add(Some(thigh), Vec3::new(0.0, -0.45, 0.0));
        let foot = self.add(Some(calf), Vec3::new(0.0, -0.42, 0.0));
        self.add(Some(foot), Vec3::new(0.0, -0.05, 0.12));
        foot
    }
}

/// The humanoid, its left arm's two-bone chain, the three joints a lean turns,
/// and the hands and feet.
fn humanoid() -> (Skeleton, [usize; 3], [usize; 3], [usize; 4]) {
    let mut builder = Builder { joints: Vec::new() };
    let hips = builder.add(None, Vec3::new(0.0, 1.0, 0.0));
    let spine = builder.add(Some(hips), Vec3::new(0.0, 0.1, 0.0));
    let chest = builder.add(Some(spine), Vec3::new(0.0, 0.15, 0.0));
    let upper_chest = builder.add(Some(chest), Vec3::new(0.0, 0.15, 0.0));
    let neck = builder.add(Some(upper_chest), Vec3::new(0.0, 0.15, 0.0));
    let head = builder.add(Some(neck), Vec3::new(0.0, 0.1, 0.0));
    builder.add(Some(head), Vec3::new(0.0, 0.2, 0.0));
    builder.add(Some(head), Vec3::new(0.03, 0.08, 0.09));
    builder.add(Some(head), Vec3::new(-0.03, 0.08, 0.09));
    let left_arm = builder.arm(upper_chest, 1.0);
    let right_arm = builder.arm(upper_chest, -1.0);
    let left_foot = builder.leg(hips, 1.0);
    let right_foot = builder.leg(hips, -1.0);
    let extremities = [left_arm[2], right_arm[2], left_foot, right_foot];

    // The bind pose is the rest pose, so each inverse bind undoes the joint's
    // rest global, composed the way a palette composes it.
    let mut globals: Vec<Mat4> = Vec::with_capacity(builder.joints.len());
    let joints = builder
        .joints
        .iter()
        .map(|&(parent, offset)| {
            let rest = Trs {
                translation: offset,
                ..Trs::IDENTITY
            };
            let global = parent.map_or(Mat4::IDENTITY, |parent| globals[parent]) * rest.to_mat4();
            globals.push(global);
            Joint {
                parent,
                inverse_bind: global.inverse(),
                rest,
            }
        })
        .collect();
    let skeleton = Skeleton::new(joints).expect("every parent is added before its children");
    (skeleton, left_arm, [spine, chest, neck], extremities)
}

/// A pose with every joint turned a little, differently for each `phase`.
fn posed(skeleton: &Skeleton, phase: f32) -> Pose {
    let mut pose = Pose::new(skeleton);
    for (index, local) in pose.locals_mut().iter_mut().enumerate() {
        let angle = 0.05 + 0.01 * (index as f32) + phase;
        local.rotation = Quat::from_euler(glam::EulerRot::YXZ, angle, -0.5 * angle, 0.25 * angle);
    }
    pose
}

/// The keyframe times of a cycle `duration` seconds long at `rate` keys a
/// second, computed afresh for each channel the way an importer hands every
/// channel its own array.
fn timeline(duration: f32, rate: f32) -> Vec<f32> {
    let keys = (duration * rate).round() as usize + 1;
    (0..keys).map(|key| key as f32 / rate).collect()
}

/// A locomotion cycle `duration` seconds long whose swing grows with `stride`.
///
/// The fingers are keyed at a rate of their own, as a separately baked layer
/// would be, and every other channel at the body rate.
fn cycle(skeleton: &Skeleton, extremities: [usize; 4], duration: f32, stride: f32) -> Clip {
    const BODY_RATE: f32 = 30.0;
    const FINGER_RATE: f32 = 24.0;
    // A finger is any joint with a hand among its ancestors.
    let finger = |joint: usize| {
        let mut ancestor = skeleton.joints()[joint].parent;
        while let Some(parent) = ancestor {
            if parent == extremities[0] || parent == extremities[1] {
                return true;
            }
            ancestor = skeleton.joints()[parent].parent;
        }
        false
    };
    let wave = |time: f32, joint: usize| {
        (std::f32::consts::TAU * time / duration + 0.3 * joint as f32).sin() * stride
    };
    let mut channels = Vec::new();
    for joint in 0..skeleton.len() {
        let rate = if finger(joint) {
            FINGER_RATE
        } else {
            BODY_RATE
        };
        let times = timeline(duration, rate);
        let rotations = times
            .iter()
            .map(|&time| {
                let swing = wave(time, joint);
                Quat::from_euler(glam::EulerRot::YXZ, 0.2 * swing, swing, -0.1 * swing)
            })
            .collect();
        channels.push(
            Channel::new(
                joint,
                times,
                Interpolation::Linear,
                Track::Rotation(rotations),
            )
            .expect("well formed"),
        );
    }
    let times = timeline(duration, BODY_RATE);
    let bob = times
        .iter()
        .map(|&time| Vec3::new(0.0, 1.0 + 0.05 * wave(2.0 * time, 0), 0.02 * wave(time, 0)))
        .collect();
    channels.push(
        Channel::new(0, times, Interpolation::Linear, Track::Translation(bob))
            .expect("well formed"),
    );
    for joint in extremities {
        let times = timeline(duration, BODY_RATE);
        let scales = times
            .iter()
            .map(|&time| Vec3::splat(1.0 + 0.01 * wave(time, joint)))
            .collect();
        channels.push(
            Channel::new(joint, times, Interpolation::Linear, Track::Scale(scales))
                .expect("well formed"),
        );
    }
    Clip::new(channels)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().expect("the first argument is the mode");
    let calls: u32 = args
        .next()
        .map_or(Ok(100_000), |count| count.parse())
        .expect("the second argument is a call count");
    let (skeleton, arm, lean, extremities) = humanoid();
    assert_eq!(skeleton.len(), 55, "the humanoid EW measured has 55 joints");
    let poses = [posed(&skeleton, 0.0), posed(&skeleton, 0.1)];
    let mut pose = poses[0].clone();
    let mut palette = Palette::new(&skeleton);
    palette.compute(&skeleton, &pose);
    let model = Mat4::from_translation(Vec3::new(3.0, 0.0, -2.0));
    let turn = Quat::from_rotation_x(0.001);
    let turns = lean.map(|joint| (joint, turn));
    let targets = [Vec3::new(3.4, 1.2, -1.7), Vec3::new(3.3, 1.4, -1.6)];
    let walk = cycle(&skeleton, extremities, 1.5, 0.4);
    let run = cycle(&skeleton, extremities, 1.0, 0.8);
    let (mut walk_pose, mut run_pose) = (Pose::new(&skeleton), Pose::new(&skeleton));
    walk.sample_into(0.4, &skeleton, &mut walk_pose);
    run.sample_into(0.3, &skeleton, &mut run_pose);
    const TICK: f32 = 1.0 / 60.0;
    for n in 0..calls {
        let which = (n % 2) as usize;
        match mode.as_str() {
            "full" => palette.compute(&skeleton, black_box(&poses[which])),
            "repeat" => palette.compute(&skeleton, black_box(&pose)),
            "turns" => rotate_joints(&skeleton, model, &mut pose, &mut palette, &turns)
                .expect("the lean's joints are conformal"),
            "two-bone" => solve_two_bone(
                &skeleton,
                model,
                &mut pose,
                &mut palette,
                arm,
                black_box(targets[which]),
                Vec3::NEG_Z,
            )
            .expect("the arm is a chain"),
            "sample" => walk.sample_into(
                black_box((n as f32 * TICK) % walk.duration()),
                &skeleton,
                &mut pose,
            ),
            "blend" => blend_into(
                &walk_pose,
                &run_pose,
                black_box([0.3, 0.7][which]),
                &mut pose,
            ),
            other => panic!("unknown mode {other}: full, repeat, turns, two-bone, sample or blend"),
        }
    }
    let wrist = palette.globals()[arm[2]].w_axis;
    let elbow = pose.locals()[arm[1]].rotation;
    println!("{mode}: {calls} calls, wrist at {wrist}, elbow turned {elbow}");
}
