//! [`Clip::sample_into`] searches each group of channels that share keyframe
//! times once, instead of once per channel; these hold it to the per-channel
//! sampler, bit for bit.
//!
//! The reference is the sampler as it was before channels were grouped, kept
//! here verbatim: one binary search per channel, in channel order. Every
//! comparison is of bit patterns, not of values, so a `-0.0` against a `0.0`
//! is a difference and a `NaN` against a `NaN` is not.

use crcbl_anim::{Channel, Clip, Interpolation, Joint, Pose, Skeleton, Track, Trs, blend_into};
use crcbl_rand::Rng;
use glam::{Mat4, Quat, Vec3};

/// The reference: the sampler before it grouped channels, verbatim.
mod reference {
    use crcbl_anim::{Channel, Clip, Interpolation, Pose, Skeleton, Track, Trs};
    use glam::{Quat, Vec3};

    const CUBIC_SPLINE_VALUES_PER_KEYFRAME: usize = 3;

    #[derive(Clone, Copy)]
    enum Segment {
        Exact(usize),
        Between { k: usize, t: f32, t_d: f32 },
    }

    fn locate(times: &[f32], time: f32) -> Segment {
        let last = times.len() - 1;
        if time.is_nan() || time <= times[0] {
            return Segment::Exact(0);
        }
        if time >= times[last] {
            return Segment::Exact(last);
        }
        let k = times.partition_point(|&keyframe| keyframe <= time) - 1;
        if times[k] == time {
            return Segment::Exact(k);
        }
        let t_d = times[k + 1] - times[k];
        Segment::Between {
            k,
            t: (time - times[k]) / t_d,
            t_d,
        }
    }

    const fn cubic_triple(k: usize) -> (usize, usize, usize) {
        let base = k * CUBIC_SPLINE_VALUES_PER_KEYFRAME;
        (base, base + 1, base + 2)
    }

    fn hermite(t: f32, t_d: f32) -> [f32; 4] {
        let t2 = t * t;
        let t3 = t2 * t;
        [
            2.0 * t3 - 3.0 * t2 + 1.0,
            t_d * (t3 - 2.0 * t2 + t),
            -2.0 * t3 + 3.0 * t2,
            t_d * (t3 - t2),
        ]
    }

    fn sample_vec3(values: &[Vec3], interpolation: Interpolation, segment: Segment) -> Vec3 {
        match segment {
            Segment::Exact(k) => match interpolation {
                Interpolation::Linear | Interpolation::Step => values[k],
                Interpolation::CubicSpline => values[cubic_triple(k).1],
            },
            Segment::Between { k, t, t_d } => match interpolation {
                Interpolation::Step => values[k],
                Interpolation::Linear => values[k] * (1.0 - t) + values[k + 1] * t,
                Interpolation::CubicSpline => {
                    let (_, v_k, b_k) = cubic_triple(k);
                    let (a_next, v_next, _) = cubic_triple(k + 1);
                    let [h00, h10, h01, h11] = hermite(t, t_d);
                    values[v_k] * h00
                        + values[b_k] * h10
                        + values[v_next] * h01
                        + values[a_next] * h11
                }
            },
        }
    }

    fn sample_quat(values: &[Quat], interpolation: Interpolation, segment: Segment) -> Quat {
        match segment {
            Segment::Exact(k) => match interpolation {
                Interpolation::Linear | Interpolation::Step => values[k],
                Interpolation::CubicSpline => values[cubic_triple(k).1],
            },
            Segment::Between { k, t, t_d } => match interpolation {
                Interpolation::Step => values[k],
                Interpolation::Linear => values[k].slerp(values[k + 1], t),
                Interpolation::CubicSpline => {
                    let (_, v_k, b_k) = cubic_triple(k);
                    let (a_next, v_next, _) = cubic_triple(k + 1);
                    let [h00, h10, h01, h11] = hermite(t, t_d);
                    let spline = values[v_k] * h00
                        + values[b_k] * h10
                        + values[v_next] * h01
                        + values[a_next] * h11;
                    spline.normalize()
                }
            },
        }
    }

    fn sample_channel(channel: &Channel, time: f32, local: &mut Trs) {
        let interpolation = channel.interpolation();
        let segment = locate(channel.times(), time);
        match channel.track() {
            Track::Translation(values) => {
                local.translation = sample_vec3(values, interpolation, segment);
            }
            Track::Rotation(values) => {
                local.rotation = sample_quat(values, interpolation, segment);
            }
            Track::Scale(values) => {
                local.scale = sample_vec3(values, interpolation, segment);
            }
        }
    }

    pub fn sample_into(clip: &Clip, time: f32, skeleton: &Skeleton, pose: &mut Pose) {
        for (local, joint) in pose.locals_mut().iter_mut().zip(skeleton.joints()) {
            *local = joint.rest;
        }
        for channel in clip.channels() {
            let Some(local) = pose.locals_mut().get_mut(channel.joint()) else {
                continue;
            };
            sample_channel(channel, time, local);
        }
    }

    pub fn blend_into(a: &Pose, b: &Pose, weight: f32, out: &mut Pose) {
        if weight.is_nan() || weight <= 0.0 {
            out.locals_mut().copy_from_slice(a.locals());
            return;
        }
        if weight >= 1.0 {
            out.locals_mut().copy_from_slice(b.locals());
            return;
        }
        for ((local, &from), &to) in out.locals_mut().iter_mut().zip(a.locals()).zip(b.locals()) {
            *local = Trs {
                translation: from.translation.lerp(to.translation, weight),
                rotation: from.rotation.slerp(to.rotation, weight),
                scale: from.scale.lerp(to.scale, weight),
            };
        }
    }
}

fn bits(pose: &Pose) -> Vec<[u32; 10]> {
    pose.locals()
        .iter()
        .map(|trs| {
            let [tx, ty, tz] = trs.translation.to_array().map(f32::to_bits);
            let [rx, ry, rz, rw] = trs.rotation.to_array().map(f32::to_bits);
            let [sx, sy, sz] = trs.scale.to_array().map(f32::to_bits);
            [tx, ty, tz, rx, ry, rz, rw, sx, sy, sz]
        })
        .collect()
}

/// Panics naming the first joint whose bits differ.
fn assert_same(got: &Pose, want: &Pose, what: &dyn Fn() -> String) {
    let (got_bits, want_bits) = (bits(got), bits(want));
    if let Some(joint) = (0..got_bits.len()).find(|&joint| got_bits[joint] != want_bits[joint]) {
        panic!(
            "{}: joint {joint} differs from the per-channel sampler\n  got  {:?}\n  want {:?}",
            what(),
            got.locals()[joint],
            want.locals()[joint]
        );
    }
}

/// A float in `[-range, range)`.
fn uniform(rng: &mut Rng, range: f32) -> f32 {
    let unit = (rng.next_u32() >> 8) as f32 / (1 << 24) as f32;
    (unit * 2.0 - 1.0) * range
}

fn below(rng: &mut Rng, bound: usize) -> usize {
    rng.next_u32() as usize % bound
}

fn vector(rng: &mut Rng, range: f32) -> Vec3 {
    Vec3::new(
        uniform(rng, range),
        uniform(rng, range),
        uniform(rng, range),
    )
}

fn skeleton(rng: &mut Rng, joints: usize) -> Skeleton {
    let joints = (0..joints)
        .map(|index| Joint {
            parent: (index > 0).then(|| below(rng, index)),
            inverse_bind: Mat4::IDENTITY,
            rest: Trs {
                translation: vector(rng, 1.0),
                rotation: Quat::from_scaled_axis(vector(rng, 3.0)),
                scale: Vec3::ONE + vector(rng, 0.25),
            },
        })
        .collect();
    Skeleton::new(joints).expect("every parent precedes its child")
}

/// Strictly ascending finite times, `1..=60` of them, from a start that is now
/// and then a signed zero — so two arrays can be equal as numbers and differ
/// in bits.
fn timeline(rng: &mut Rng) -> Vec<f32> {
    let keys = 1 + below(rng, 60);
    let mut time = match below(rng, 4) {
        0 => 0.0,
        1 => -0.0,
        _ => uniform(rng, 1.0),
    };
    (0..keys)
        .map(|_| {
            let key = time;
            time += 0.01 + uniform(rng, 0.1).abs();
            key
        })
        .collect()
}

/// A channel on `times` driving `joint`, of a random component and
/// interpolation.
fn channel(rng: &mut Rng, joint: usize, times: Vec<f32>) -> Channel {
    let interpolation = match below(rng, 3) {
        0 => Interpolation::Step,
        1 if times.len() >= 2 => Interpolation::CubicSpline,
        _ => Interpolation::Linear,
    };
    let values = times.len()
        * match interpolation {
            Interpolation::CubicSpline => 3,
            _ => 1,
        };
    let track = match below(rng, 3) {
        0 => Track::Translation((0..values).map(|_| vector(rng, 2.0)).collect()),
        1 => Track::Rotation(
            (0..values)
                .map(|_| Quat::from_scaled_axis(vector(rng, 3.0)))
                .collect(),
        ),
        _ => Track::Scale((0..values).map(|_| vector(rng, 2.0)).collect()),
    };
    Channel::new(joint, times, interpolation, track).expect("well formed")
}

/// A clip over a skeleton of `joints` joints: a few timelines, each channel
/// on one of them (cloned, so equal in bits but not in address) or on one of
/// its own, now and then naming a joint past the skeleton, and now and then
/// driving a component an earlier channel drove too.
///
/// Returns how many channels share a timeline with an earlier channel, so the
/// test can show the grouping it exists for was exercised.
fn clip(rng: &mut Rng, joints: usize) -> (Clip, usize) {
    let shared: Vec<Vec<f32>> = (0..1 + below(rng, 3)).map(|_| timeline(rng)).collect();
    let mut sharing = 0;
    let channels = (0..below(rng, 80))
        .map(|_| {
            let joint = if below(rng, 10) == 0 {
                joints + below(rng, 3)
            } else {
                below(rng, joints)
            };
            let times = if below(rng, 4) == 0 {
                timeline(rng)
            } else {
                sharing += 1;
                shared[below(rng, shared.len())].clone()
            };
            channel(rng, joint, times)
        })
        .collect();
    (Clip::new(channels), sharing)
}

/// The times worth asking about: every keyframe time of every channel, the
/// points between and around them, the ends and past them, and the
/// non-finite ones.
fn times(rng: &mut Rng, clip: &Clip) -> Vec<f32> {
    let mut times = vec![
        f32::NAN,
        -f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        0.0,
        -0.0,
        -5.0,
        clip.duration(),
        clip.duration() + 5.0,
    ];
    for channel in clip.channels().iter().take(6) {
        for pair in channel.times().windows(2) {
            times.push(pair[0]);
            times.push(0.5 * (pair[0] + pair[1]));
            times.push(pair[1].next_down());
        }
        times.push(channel.end_time());
    }
    for _ in 0..32 {
        times.push(uniform(rng, clip.duration().abs() + 1.0));
    }
    times
}

/// **The property.** Across seeded clips, every sample of the grouped sampler
/// is the per-channel sampler's, in every bit — and so is every blend of two
/// of them.
#[test]
fn a_grouped_sample_is_the_per_channel_sample() {
    let (mut sharing, mut samples) = (0, 0);
    for seed in 0..96 {
        let mut rng = Rng::from_u64(seed);
        let joints = 1 + below(&mut rng, 40);
        let skeleton = skeleton(&mut rng, joints);
        let (first, shared) = clip(&mut rng, joints);
        let (second, _) = clip(&mut rng, joints);
        sharing += shared;
        let (mut got, mut want) = (Pose::new(&skeleton), Pose::new(&skeleton));
        let (mut got_b, mut want_b) = (Pose::new(&skeleton), Pose::new(&skeleton));
        let (mut got_mix, mut want_mix) = (Pose::new(&skeleton), Pose::new(&skeleton));
        let weights = [f32::NAN, -1.0, 0.0, 0.25, 0.5, 0.999, 1.0, 2.0];
        for time in times(&mut rng, &first) {
            first.sample_into(time, &skeleton, &mut got);
            reference::sample_into(&first, time, &skeleton, &mut want);
            assert_same(&got, &want, &|| format!("seed {seed}, time {time:?}"));
            second.sample_into(time, &skeleton, &mut got_b);
            reference::sample_into(&second, time, &skeleton, &mut want_b);
            assert_same(&got_b, &want_b, &|| {
                format!("seed {seed}, second clip, time {time:?}")
            });
            let weight = weights[samples % weights.len()];
            blend_into(&got, &got_b, weight, &mut got_mix);
            reference::blend_into(&want, &want_b, weight, &mut want_mix);
            assert_same(&got_mix, &want_mix, &|| {
                format!("seed {seed}, time {time:?}, blend weight {weight:?}")
            });
            samples += 1;
        }
    }
    // The checks above would pass over clips that never shared a timeline.
    assert!(sharing > 1000, "only {sharing} channels shared a timeline");
    assert!(samples > 10_000, "only {samples} samples were compared");
}
