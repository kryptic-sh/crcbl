//! Audio for puppet: two beacons humming behind the mounds, muffled by
//! whatever stands between them and the camera — rule 5 of the cue grammar,
//! through the same query world the camera's boom sweeps.
//!
//! ```text
//!   Map::world_with_ids ──▶ ClientQueryWorld (the boom's, and the ears')
//!                     └──▶ AcousticMaterials: each surface's collider, by row
//!
//!   each frame: camera ──▶ Audio::hear_from ──▶ listener, beacon pan and level
//!                                           └──▶ OcclusionTracker::update
//!                                                  ray ear → beacon, per mound
//!                                                  ──▶ Mixer::set_occlusion
//! ```
//!
//! # Two beacons, two materials
//!
//! [`BEACONS`] stand one behind each mound, level with the spawn, so the walk
//! out from it is a walk around them: from the spawn both are heard through a
//! mound, and from either side the near one comes clear. The steep mound is
//! packed earth ([`AcousticMaterial::CONCRETE`]) and the gentle one, tinted
//! green, is brush ([`AcousticMaterial::FOLIAGE`]), so the two are muffled
//! differently from the same spot — which is the "material-distinct" half of
//! the audio exit criterion, heard rather than asserted. Each beacon is a low
//! hum with a whistle [`WHISTLE_RATIO`] times above it, because a lowpass is
//! only audible on a sound with highs to take away.
//!
//! # The beacons start on the first frame that has an ear
//!
//! Not at start-up: until a camera exists there is nowhere to hear them from,
//! and a voice started clear and loud and then pulled down would be a click and
//! a sweep the player never asked for. The first [`Audio::hear_from`] walks each
//! beacon's ray once, outside the tracker's budget, and starts it already at
//! that occlusion and that pan; every frame after re-aims it and leaves the
//! re-occluding to the tracker.
//!
//! # Where the ear is
//!
//! On the camera, at its eye and facing what it looks at, as towers does: the
//! one convention where the picture and the sound never disagree. The boom pulls
//! the eye in when a mound is behind the character, and the ear goes with it.
//!
//! # Headless plays nothing
//!
//! A headless run builds no `Audio` (see `crate::app`): no device is opened and
//! no ray is cast for a listener who is not there. This module's tests build
//! one without a stream, through [`Audio::offline`].

use std::sync::Arc;

use crcbl::audio::AudioStream;
use crcbl::audio::mixer::{Bus, Mixer, Voice, VoiceId, VoiceMix};
use crcbl::audio::occlusion::AcousticMaterial;
use crcbl::audio::spatial::Listener;
use crcbl::audio::{AudioSample, INTERNAL_SAMPLE_RATE, synth};
use crcbl::client::ClientQueryWorld;
use crcbl::math::DVec3;
use crcbl::occlusion::{AcousticMaterials, OcclusionTracker, occlusion_between};
use crcbl::phys::ColliderId;
use crcbl::render::Camera;

use crate::map::{STEEP_MOUND, Surface};

/// How loud a beacon is against the volume the grammar asks for: under half,
/// because it hums for the whole run.
const BEACON_GAIN: f32 = 0.35;

/// The settings directory this sample reads its volumes out of — the label
/// `crate::gpu` gives its context, because it is the same directory.
const APP_NAME: &str = "puppet";

/// How high above the ground a beacon stands, in metres: low enough that a
/// mound between it and a standing eye is in the way.
const BEACON_HEIGHT: f64 = 0.5;

/// How far past a mound's far rim a beacon stands, in metres along `X`.
const BEACON_CLEARANCE: f64 = 2.5;

/// One humming emitter: where it stands and the hum it is built on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beacon {
    /// Where it stands, in metres.
    pub at: DVec3,
    /// The hum, in hertz.
    pub low_hz: f32,
}

/// The whistle's frequency over the hum's: three octaves, so it sits where any
/// of the presets' cutoffs takes it away and the hum sits under most of them.
pub const WHISTLE_RATIO: u32 = 8;

/// How many of the hum's cycles one loop of a beacon's sound holds. The
/// whistle takes [`WHISTLE_RATIO`] times as many, so both join at the same
/// frame.
const HUM_CYCLES: u32 = 1;

/// The beacons: one behind the steep mound to the west, one behind the gentle
/// mound to the east, both level with the spawn. See the module docs.
pub const BEACONS: [Beacon; 2] = [
    Beacon {
        at: DVec3::new(
            STEEP_MOUND.0 - STEEP_MOUND.2 - BEACON_CLEARANCE,
            BEACON_HEIGHT,
            STEEP_MOUND.1,
        ),
        low_hz: 220.0,
    },
    Beacon {
        at: DVec3::new(
            crate::map::GENTLE_MOUND.0 + crate::map::GENTLE_MOUND.2 + BEACON_CLEARANCE,
            BEACON_HEIGHT,
            crate::map::GENTLE_MOUND.1,
        ),
        low_hz: 330.0,
    },
];

/// What a surface is made of, to a sound: the gentle mound is brush, which its
/// green tint already says, and every other row — the ground, the steps, the
/// steep mound — is packed earth or concrete.
///
/// Keyed by the row's label because the label is the map's own name for the
/// thing; a scene that renames a row hears it as earth.
#[must_use]
pub fn material_of(surface: &Surface) -> AcousticMaterial {
    match surface.label.as_str() {
        "gentle mound" => AcousticMaterial::FOLIAGE,
        _ => AcousticMaterial::CONCRETE,
    }
}

/// The acoustic table for a map: each surface's collider named with
/// [`material_of`] its row. `ids` are [`Map::world_with_ids`]'s, one per row.
///
/// [`Map::world_with_ids`]: crate::map::Map::world_with_ids
#[must_use]
pub fn materials(surfaces: &[Surface], ids: &[ColliderId]) -> AcousticMaterials {
    let mut table = AcousticMaterials::new(AcousticMaterial::CONCRETE);
    for (surface, id) in surfaces.iter().zip(ids) {
        table.set(*id, material_of(surface));
    }
    table
}

/// `beacon`'s sound: its hum and the whistle above it, summed, as a loop.
fn waveform(beacon: &Beacon) -> Vec<AudioSample> {
    let hum = synth::looped_sine(beacon.low_hz, HUM_CYCLES, INTERNAL_SAMPLE_RATE);
    let whistle = synth::looped_sine(
        beacon.low_hz * WHISTLE_RATIO as f32,
        HUM_CYCLES * WHISTLE_RATIO,
        INTERNAL_SAMPLE_RATE,
    );
    // Half of each, so the sum peaks where one tone alone would.
    hum.iter()
        .zip(&whistle)
        .map(|(low, high)| 0.5 * (low + high))
        .collect()
}

/// The beacons, the mixer they hum into, and what keeps them occluded.
#[derive(Debug)]
pub struct Audio {
    mixer: Arc<Mixer>,
    /// One voice per [`BEACONS`] entry, in its order, once the first frame has
    /// started them. See the module docs.
    voices: Option<[VoiceId; BEACONS.len()]>,
    tracker: OcclusionTracker,
    materials: AcousticMaterials,
    _stream: Option<AudioStream>,
}

impl Audio {
    /// Opens the default output device, to hear the beacons through
    /// `materials` from the first frame on. The player's `[engine.audio]`
    /// volumes are read from their settings file; a machine with no device
    /// plays nothing and says so once.
    #[must_use]
    pub fn open(materials: AcousticMaterials) -> Self {
        let audio = Self::silent(crcbl::engine::SettingsSource::Platform, materials);
        let stream = AudioStream::open(Arc::clone(&audio.mixer));
        if stream.is_none() {
            crcbl::log::info!("audio: no output device available; the beacons will be silent");
        }
        Self {
            _stream: stream,
            ..audio
        }
    }

    /// The mixer with no stream: what this crate's tests fill by hand.
    #[must_use]
    pub fn offline(materials: AcousticMaterials) -> Self {
        Self::silent(crcbl::engine::SettingsSource::None, materials)
    }

    /// Everything but the stream, with the volumes `settings` holds.
    fn silent(settings: crcbl::engine::SettingsSource<'_>, materials: AcousticMaterials) -> Self {
        let mixer = Arc::new(Mixer::new());
        // Before the beacons start, so they start at the player's volumes.
        settings.apply_audio_gains(APP_NAME, &mixer);
        Self {
            mixer,
            voices: None,
            tracker: OcclusionTracker::default(),
            materials,
            _stream: None,
        }
    }

    /// Puts the ear on `camera` — at its eye, facing what it looks at — and
    /// hears the beacons from there through `world`: started on the first call,
    /// re-aimed and re-occluded on every one after.
    pub fn hear_from(&mut self, camera: &Camera, world: &mut ClientQueryWorld) {
        self.mixer.set_listener(Listener::facing(
            camera.eye.to_array(),
            (camera.target - camera.eye).to_array(),
        ));
        let ear = camera.eye.as_dvec3();
        let Some(voices) = self.voices else {
            self.voices = Some(self.start(ear, world));
            return;
        };
        for (beacon, id) in BEACONS.iter().zip(voices) {
            self.mixer.set_mix(id, self.mix_for(beacon));
        }
        self.tracker
            .update(world, ear, &self.materials, &self.mixer);
    }

    /// Start every beacon at the pan, level and occlusion it is heard at from
    /// `ear`, and track it from there.
    fn start(&mut self, ear: DVec3, world: &mut ClientQueryWorld) -> [VoiceId; BEACONS.len()] {
        BEACONS.map(|beacon| {
            let occlusion = occlusion_between(
                |ray, filter| world.cast_ray(ray, filter),
                ear,
                beacon.at,
                &self.materials,
            )
            .occlusion;
            let id = self.mixer.play(
                Voice::new(waveform(&beacon))
                    .with_looping()
                    .with_mix(self.mix_for(&beacon))
                    .with_occlusion(occlusion),
            );
            self.tracker.track_at(id, beacon.at, occlusion);
            id
        })
    }

    /// The pan and level `beacon` is heard at from the mixer's listener.
    fn mix_for(&self, beacon: &Beacon) -> VoiceMix {
        let cue = self.mixer.cue(beacon.at.as_vec3().to_array());
        VoiceMix {
            volume: cue.volume * BEACON_GAIN,
            ..VoiceMix::from(&cue)
        }
    }

    /// Moves `bus`'s gain on the mixer already playing — what the console's
    /// `[engine.audio]` keys reach through
    /// [`crcbl::engine::HostedGame::set_bus_gain`].
    pub fn set_bus_gain(&self, bus: Bus, gain: f32) {
        self.mixer.set_bus_gain(bus, gain);
    }

    /// The mixer the beacons hum into, for this crate's tests to fill and read.
    #[must_use]
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// The beacons' voices, in [`BEACONS`]' order, once the first frame has
    /// started them.
    #[must_use]
    pub const fn voices(&self) -> Option<[VoiceId; BEACONS.len()]> {
        self.voices
    }
}

#[cfg(test)]
mod tests;
