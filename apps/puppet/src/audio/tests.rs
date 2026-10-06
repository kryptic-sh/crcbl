use super::*;
use crcbl::audio::AudioSource;
use crcbl::audio::occlusion::Occlusion;
use crcbl::math::Vec3;
use crcbl::phys::PhysicsWorld;

use crate::map::{Map, SPAWN};

/// How high a standing eye is, in metres: the camera at the character's head
/// with the boom pulled all the way in, which is the lowest the ear goes and
/// so the view most mounds stand in front of.
const EYE_HEIGHT: f32 = 1.6;

/// The committed blockout's query world and its acoustic table.
fn the_map() -> (ClientQueryWorld, AcousticMaterials) {
    let map = Map::built_in();
    let (statics, colliders) = map.world_with_ids();
    (
        ClientQueryWorld::new(statics),
        materials(map.surfaces(), &colliders),
    )
}

/// A camera at `eye`, looking along `-Z` as the follow camera does at the
/// spawn.
fn camera_at(eye: Vec3) -> Camera {
    Camera {
        eye,
        target: eye - Vec3::Z,
        ..Camera::default()
    }
}

/// The eye standing on the spawn.
fn spawn_eye() -> Vec3 {
    SPAWN.as_vec3() + Vec3::Y * EYE_HEIGHT
}

/// The left channel's root mean square over a quarter second of `audio`'s mix.
fn level(audio: &Audio) -> f32 {
    let frames = INTERNAL_SAMPLE_RATE as usize / 4;
    let mut buf = vec![0.0; frames * crcbl::audio::CHANNELS];
    audio.mixer().fill(&mut buf, INTERNAL_SAMPLE_RATE);
    let left: Vec<f32> = buf
        .iter()
        .step_by(crcbl::audio::CHANNELS)
        .copied()
        .collect();
    (left.iter().map(|s| s * s).sum::<f32>() / left.len() as f32).sqrt()
}

/// **From the spawn both beacons are heard through a mound, and differently**:
/// the west one through the steep mound's earth, the east one through the
/// gentle mound's brush. They start there, on the first frame, rather than
/// starting clear and ramping down.
#[test]
fn from_the_spawn_each_beacon_is_muffled_by_its_own_mound() {
    let (mut world, materials) = the_map();
    let mut audio = Audio::offline(materials);
    assert_eq!(
        audio.voices(),
        None,
        "the beacons started before there was an ear"
    );
    audio.hear_from(&camera_at(spawn_eye()), &mut world);
    let [west, east] = audio.voices().expect("the first frame starts the beacons");

    let earth = Occlusion::through([AcousticMaterial::CONCRETE]);
    let brush = Occlusion::through([AcousticMaterial::FOLIAGE]);
    assert_eq!(audio.mixer().occlusion(west), Some(earth));
    assert_eq!(audio.mixer().occlusion(east), Some(brush));
    assert_ne!(earth, brush, "the two mounds would sound alike");
}

/// **Stepping round the steep mound brings its beacon clear**, through the
/// tracker on the frames after the first.
#[test]
fn stepping_round_the_mound_brings_its_beacon_clear() {
    let (mut world, materials) = the_map();
    let mut audio = Audio::offline(materials);
    audio.hear_from(&camera_at(spawn_eye()), &mut world);
    let [west, _] = audio.voices().expect("started");
    assert_ne!(audio.mixer().occlusion(west), Some(Occlusion::CLEAR));

    // Out on the far side of the steep mound, south of its beacon, looking at
    // it down open ground.
    let beside = BEACONS[0].at.as_vec3() + Vec3::new(0.0, EYE_HEIGHT, 6.0);
    audio.hear_from(&camera_at(beside), &mut world);
    assert_eq!(audio.mixer().occlusion(west), Some(Occlusion::CLEAR));
}

/// **The mounds make the mix quieter**: the same ear on the spawn, the same
/// beacons, heard through the blockout and through an empty world.
#[test]
fn the_mounds_take_level_out_of_the_mix() {
    let (mut world, materials) = the_map();
    let mut muffled = Audio::offline(materials.clone());
    muffled.hear_from(&camera_at(spawn_eye()), &mut world);

    let mut open_air = ClientQueryWorld::new(PhysicsWorld::new());
    let mut clear = Audio::offline(materials);
    clear.hear_from(&camera_at(spawn_eye()), &mut open_air);

    let (through, without) = (level(&muffled), level(&clear));
    assert!(without > 0.0, "the beacons are silent in the open");
    assert!(
        through < 0.5 * without,
        "through the mounds the mix kept {through} of {without}",
    );
}

/// **Every surface has a material, and only the gentle mound is brush.**
#[test]
fn every_surface_is_earth_but_the_gentle_mound() {
    let map = Map::built_in();
    for surface in map.surfaces() {
        let expected = if surface.label == "gentle mound" {
            AcousticMaterial::FOLIAGE
        } else {
            AcousticMaterial::CONCRETE
        };
        assert_eq!(material_of(surface), expected, "{}", surface.label);
    }
}

/// **A beacon is a whole number of loops of both its tones**: the hum and the
/// whistle join at the same frame, so the loop has no seam.
#[test]
fn a_beacon_loops_without_a_seam() {
    for beacon in BEACONS {
        let sound = waveform(&beacon);
        let hum = synth::looped_sine(beacon.low_hz, HUM_CYCLES, INTERNAL_SAMPLE_RATE);
        assert_eq!(
            sound.len(),
            hum.len(),
            "the whistle and the hum differ in length"
        );
        assert_eq!(sound[0], 0.0, "the loop does not start on its own seam");
    }
}
