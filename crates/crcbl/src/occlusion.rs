//! Occlusion rays: rule 5 of the cue grammar, cast through the client's query
//! world and handed to the mixer as a per-voice filter target.
//!
//! ```text
//! each frame:  OcclusionTracker::update(world, ear, materials, mixer)
//!                 │  round robin over the tracked voices, under a ray budget
//!                 ▼
//!              occlusion_between(cast, ear, emitter, materials)
//!                 │  ray ear → emitter; each collider it meets, once,
//!                 │  looked up in AcousticMaterials
//!                 ▼
//!              Occlusion::through(materials) ──▶ Mixer::set_occlusion
//! ```
//!
//! **Here, in the umbrella, for [`crate::acoustic_path`]'s reason**: it joins
//! `crcbl-client`'s query world and `crcbl-phys`'s rays to `crcbl-audio`'s
//! filter, and `crcbl-audio` is pure DSP that takes no physics dependency.
//! `acoustic_path` answers the same question for a route a game authors
//! through its own boxes; this answers it for the straight line, against the
//! colliders the client already holds.
//!
//! # What a collider is made of is the game's table
//!
//! [`AcousticMaterials`] is a side table from [`ColliderId`] to
//! [`AcousticMaterial`], with a fallback for every collider it does not name.
//! **Not a field on the collider**: `crcbl-phys` has no business knowing about
//! sound, and a generic per-collider tag would be one more thing every
//! physics caller carries for one consumer. A game fills the table from the
//! same scene rows it built the colliders from, which is where it knows what
//! each one is.
//!
//! # The walk, and the rule its hits combine by
//!
//! One ray from the ear to the emitter, cast again past each collider it meets,
//! so every collider between the two is found in the order the sound crosses
//! it, each once. The materials combine by [`Occlusion::through`]'s rule:
//! **attenuations add and the lowest cutoff wins.** The walk stops after
//! [`MAX_OCCLUDERS`] colliders; a sound behind that many is already as muffled
//! as the preset table goes.
//!
//! # A budget of rays per frame, spread round robin
//!
//! A ray per voice per frame is a cost that grows with the mix. The tracker
//! spends at most its budget ([`DEFAULT_OCCLUSION_RAYS_PER_FRAME`] unless the
//! game says otherwise) each frame, starting where the last frame stopped, and
//! a voice it does not reach keeps the occlusion it last had. Each voice's walk
//! may cost up to [`MAX_OCCLUDERS`] casts, so a voice is begun only while that
//! many are left: the budget is a bound, never a target overshot by the last
//! voice.
//!
//! # Determinism
//!
//! The walk is a pure function of the world, the two points and the table: the
//! casts are `crcbl-phys`'s exact queries and the table is only looked up,
//! never iterated. The order the tracker visits voices is the order they were
//! tracked in.

use std::collections::HashMap;

use crcbl_audio::mixer::{Mixer, VoiceId};
use crcbl_audio::occlusion::{AcousticMaterial, Occlusion};
use crcbl_client::ClientQueryWorld;
use crcbl_phys::{ColliderId, QueryFilter, Ray, ShapeHit};
use glam::DVec3;

/// The most colliders one walk counts between an ear and an emitter, and so
/// the most casts it spends.
pub const MAX_OCCLUDERS: usize = 4;

/// The ray budget [`OcclusionTracker::default`] spends each frame.
///
/// Enough to walk every voice of a modest mix each frame through a wall or
/// two, and a bounded cost for a busy one, which then refreshes each voice
/// every few frames instead.
pub const DEFAULT_OCCLUSION_RAYS_PER_FRAME: usize = 32;

/// How far apart an ear and an emitter must be for a ray between them to have
/// a direction, in metres. Closer than this the sound is at the ear and nothing
/// can stand between them.
const MIN_PATH_M: f64 = 1e-6;

/// What each collider is made of, for occlusion: a side table, with a fallback
/// for every collider it does not name. See the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct AcousticMaterials {
    by_collider: HashMap<ColliderId, AcousticMaterial>,
    fallback: AcousticMaterial,
}

impl AcousticMaterials {
    /// An empty table, answering `fallback` for every collider.
    #[must_use]
    pub fn new(fallback: AcousticMaterial) -> Self {
        Self {
            by_collider: HashMap::new(),
            fallback,
        }
    }

    /// Say `collider` is made of `material`.
    pub fn set(&mut self, collider: ColliderId, material: AcousticMaterial) {
        self.by_collider.insert(collider, material);
    }

    /// Forget `collider`, which then answers the fallback. Answers whether it
    /// was named.
    pub fn remove(&mut self, collider: ColliderId) -> bool {
        self.by_collider.remove(&collider).is_some()
    }

    /// What `collider` is made of.
    #[must_use]
    pub fn of(&self, collider: ColliderId) -> AcousticMaterial {
        self.by_collider
            .get(&collider)
            .copied()
            .unwrap_or(self.fallback)
    }
}

/// What one walk found: the occlusion, and the casts it spent finding it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OcclusionQuery {
    /// What stands between the two points, by [`Occlusion::through`]'s rule.
    pub occlusion: Occlusion,
    /// How many colliders the walk counted, at most [`MAX_OCCLUDERS`].
    pub occluders: usize,
    /// How many rays it cast, at most [`MAX_OCCLUDERS`].
    pub casts: usize,
}

/// What stands between `ear` and `emitter`, found by casting through `cast`.
///
/// `cast` is a filtered ray query — [`ClientQueryWorld::cast_ray`] on a client,
/// [`crcbl_phys::PhysicsWorld::cast_ray_filtered`] on a server, which is the
/// form `docs/plan/32-voip.md`'s world-voice audibility test reuses this
/// through. It is asked for the closest solid hit on the segment past the last
/// collider found, leaving that collider out, until nothing is left or
/// [`MAX_OCCLUDERS`] are counted. A collider met a second time — a concave mesh
/// crossed twice — counts once and ends the walk.
pub fn occlusion_between(
    mut cast: impl FnMut(&Ray, QueryFilter) -> Option<(ColliderId, ShapeHit)>,
    ear: DVec3,
    emitter: DVec3,
    materials: &AcousticMaterials,
) -> OcclusionQuery {
    let path = emitter - ear;
    let mut query = OcclusionQuery {
        occlusion: Occlusion::CLEAR,
        occluders: 0,
        casts: 0,
    };
    if !path.is_finite() || path.length() < MIN_PATH_M {
        return query;
    }
    let mut crossed: [Option<(ColliderId, AcousticMaterial)>; MAX_OCCLUDERS] =
        [None; MAX_OCCLUDERS];
    let mut t_min = 0.0;
    let mut exclude = None;
    while query.occluders < MAX_OCCLUDERS {
        // `t` is in units of the path, so the emitter is at one.
        let ray = Ray::new(ear, path).with_bounds(t_min, 1.0);
        query.casts += 1;
        let Some((collider, hit)) = cast(&ray, QueryFilter::excluding(exclude)) else {
            break;
        };
        if crossed.iter().flatten().any(|(seen, _)| *seen == collider) {
            break;
        }
        crossed[query.occluders] = Some((collider, materials.of(collider)));
        query.occluders += 1;
        t_min = hit.t;
        exclude = Some(collider);
    }
    query.occlusion = Occlusion::through(crossed.iter().flatten().map(|(_, material)| *material));
    query
}

/// One voice the tracker keeps occluded.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tracked {
    voice: VoiceId,
    emitter: DVec3,
    /// What the tracker last handed the mixer for it.
    occlusion: Occlusion,
}

/// Keeps a set of playing voices occluded, a bounded number of rays a frame.
///
/// A game tracks a voice with its emitter's position, moves it as the emitter
/// moves, and calls [`update`](Self::update) once a frame with where the ear
/// is. See the [module docs](self) for the budget and the order.
#[derive(Clone, Debug, PartialEq)]
pub struct OcclusionTracker {
    voices: Vec<Tracked>,
    /// The voice the next update starts at.
    cursor: usize,
    rays_per_frame: usize,
}

impl OcclusionTracker {
    /// A tracker spending at most `rays_per_frame` casts each update, raised to
    /// [`MAX_OCCLUDERS`] if lower, since a budget under one walk's worth would
    /// never begin a voice.
    #[must_use]
    pub fn new(rays_per_frame: usize) -> Self {
        Self {
            voices: Vec::new(),
            cursor: 0,
            rays_per_frame: rays_per_frame.max(MAX_OCCLUDERS),
        }
    }

    /// The casts each update may spend.
    #[must_use]
    pub const fn rays_per_frame(&self) -> usize {
        self.rays_per_frame
    }

    /// Keep `voice` occluded from an emitter at `emitter`, or move it there if
    /// it is already tracked.
    ///
    /// A newly tracked voice is taken to be at [`Occlusion::CLEAR`] until an
    /// update reaches it; one started behind a wall should be started with
    /// [`Voice::with_occlusion`](crcbl_audio::mixer::Voice::with_occlusion)
    /// from an [`occlusion_between`] of its own, and tracked with
    /// [`track_at`](Self::track_at).
    pub fn track(&mut self, voice: VoiceId, emitter: DVec3) {
        self.track_at(voice, emitter, Occlusion::CLEAR);
    }

    /// [`track`](Self::track), for a voice already playing at `occlusion`.
    pub fn track_at(&mut self, voice: VoiceId, emitter: DVec3, occlusion: Occlusion) {
        if let Some(tracked) = self.voices.iter_mut().find(|t| t.voice == voice) {
            tracked.emitter = emitter;
        } else {
            self.voices.push(Tracked {
                voice,
                emitter,
                occlusion,
            });
        }
    }

    /// Stop tracking `voice`. Answers whether it was tracked. A voice that has
    /// stopped playing is dropped by the next update without this.
    pub fn untrack(&mut self, voice: VoiceId) -> bool {
        let Some(index) = self.voices.iter().position(|t| t.voice == voice) else {
            return false;
        };
        self.voices.remove(index);
        if self.cursor > index {
            self.cursor -= 1;
        }
        true
    }

    /// How many voices are tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.voices.len()
    }

    /// Whether no voice is tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// The occlusion last handed the mixer for `voice`, or `None` if it is not
    /// tracked.
    #[must_use]
    pub fn occlusion(&self, voice: VoiceId) -> Option<Occlusion> {
        self.voices
            .iter()
            .find(|t| t.voice == voice)
            .map(|t| t.occlusion)
    }

    /// Re-occlude as many tracked voices as the budget allows, heard from
    /// `ear`, and answer how many casts that spent.
    ///
    /// Voices `mixer` no longer plays are dropped first. Then, from where the
    /// last update stopped, each voice in turn is walked through `world` and
    /// its result handed to [`Mixer::set_occlusion`] if it changed, while at
    /// least [`MAX_OCCLUDERS`] casts of the budget are left and no voice has
    /// been walked twice. A voice not reached keeps what it had.
    pub fn update(
        &mut self,
        world: &mut ClientQueryWorld,
        ear: DVec3,
        materials: &AcousticMaterials,
        mixer: &Mixer,
    ) -> usize {
        // The cursor follows the voice it pointed at: it becomes the count of
        // voices kept ahead of it.
        let mut index = 0;
        let cursor = self.cursor;
        let mut kept_before_cursor = 0;
        self.voices.retain(|tracked| {
            let playing = mixer.is_playing(tracked.voice);
            if playing && index < cursor {
                kept_before_cursor += 1;
            }
            index += 1;
            playing
        });
        self.cursor = kept_before_cursor;
        if self.voices.is_empty() {
            self.cursor = 0;
            return 0;
        }
        self.cursor %= self.voices.len();

        let mut casts = 0;
        let mut walked = 0;
        while walked < self.voices.len() && self.rays_per_frame - casts >= MAX_OCCLUDERS {
            let tracked = &mut self.voices[self.cursor];
            let query = occlusion_between(
                |ray, filter| world.cast_ray(ray, filter),
                ear,
                tracked.emitter,
                materials,
            );
            casts += query.casts;
            if query.occlusion != tracked.occlusion {
                mixer.set_occlusion(tracked.voice, query.occlusion);
                tracked.occlusion = query.occlusion;
            }
            self.cursor = (self.cursor + 1) % self.voices.len();
            walked += 1;
        }
        casts
    }
}

impl Default for OcclusionTracker {
    fn default() -> Self {
        Self::new(DEFAULT_OCCLUSION_RAYS_PER_FRAME)
    }
}

#[cfg(test)]
mod tests;
