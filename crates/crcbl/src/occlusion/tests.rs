use super::*;
use crcbl_audio::mixer::Voice;
use crcbl_phys::{BoxCollider, PhysicsWorld};

/// A wall across the `X` axis at `x`, a decimetre thick and wide enough that a
/// ray along the axis cannot miss it.
fn wall(world: &mut PhysicsWorld, x: f64) -> ColliderId {
    world.add_box(BoxCollider::new(
        DVec3::new(x, 0.0, 0.0),
        DVec3::new(0.05, 4.0, 4.0),
    ))
}

/// A voice that plays until stopped, so `Mixer::is_playing` holds without a
/// fill.
fn held(mixer: &Mixer) -> VoiceId {
    mixer.play(Voice::new(vec![0.0; 64]).with_looping())
}

/// The walk over a plain [`PhysicsWorld`], as a server would cast it.
fn walk(
    world: &mut PhysicsWorld,
    ear: DVec3,
    emitter: DVec3,
    materials: &AcousticMaterials,
) -> OcclusionQuery {
    occlusion_between(
        |ray, filter| world.cast_ray_filtered(ray, filter),
        ear,
        emitter,
        materials,
    )
}

/// **A wall between the ear and the emitter lowers the cutoff and the gain**
/// to the wall's material, and the same emitter on the open side is clear —
/// through the client's query world, the tracker and the mixer, end to end.
#[test]
fn a_wall_between_lowers_the_cutoff_and_the_gain() {
    let mut statics = PhysicsWorld::new();
    let concrete = wall(&mut statics, 5.0);
    let mut materials = AcousticMaterials::new(AcousticMaterial::FOLIAGE);
    materials.set(concrete, AcousticMaterial::CONCRETE);
    let mut world = ClientQueryWorld::new(statics);

    let mixer = Mixer::new();
    let behind = held(&mixer);
    let open = held(&mixer);
    let mut tracker = OcclusionTracker::default();
    tracker.track(behind, DVec3::new(10.0, 0.0, 0.0));
    tracker.track(open, DVec3::new(-10.0, 0.0, 0.0));
    tracker.update(&mut world, DVec3::ZERO, &materials, &mixer);

    let muffled = mixer.occlusion(behind).expect("the voice is playing");
    assert_eq!(muffled, Occlusion::through([AcousticMaterial::CONCRETE]));
    assert!(muffled.cutoff_hz < Occlusion::CLEAR.cutoff_hz && muffled.gain < 1.0);
    assert_eq!(mixer.occlusion(open), Some(Occlusion::CLEAR));
    assert_eq!(tracker.occlusion(behind), Some(muffled));
}

/// **Two materials give two targets**: the same wall, named as thin wood in one
/// table and concrete in another, occludes differently, and a collider the
/// table does not name answers the fallback.
#[test]
fn two_materials_give_distinct_targets() {
    let mut world = PhysicsWorld::new();
    let door = wall(&mut world, 5.0);
    let ear = DVec3::ZERO;
    let emitter = DVec3::new(10.0, 0.0, 0.0);

    let mut wood = AcousticMaterials::new(AcousticMaterial::GLASS);
    wood.set(door, AcousticMaterial::THIN_WOOD);
    let mut concrete = AcousticMaterials::new(AcousticMaterial::GLASS);
    concrete.set(door, AcousticMaterial::CONCRETE);
    let unnamed = AcousticMaterials::new(AcousticMaterial::GLASS);

    let through_wood = walk(&mut world, ear, emitter, &wood).occlusion;
    let through_concrete = walk(&mut world, ear, emitter, &concrete).occlusion;
    let through_unnamed = walk(&mut world, ear, emitter, &unnamed).occlusion;
    assert_eq!(
        through_wood,
        Occlusion::through([AcousticMaterial::THIN_WOOD])
    );
    assert_eq!(
        through_concrete,
        Occlusion::through([AcousticMaterial::CONCRETE])
    );
    assert_eq!(
        through_unnamed,
        Occlusion::through([AcousticMaterial::GLASS])
    );
    assert_ne!(through_wood.cutoff_hz, through_concrete.cutoff_hz);
    assert_ne!(through_wood.gain, through_concrete.gain);
}

/// **Several walls combine by the stated rule**, in the order the sound
/// crosses them, each counted once — and nothing behind the emitter or behind
/// the ear counts.
#[test]
fn several_walls_combine_by_the_rule() {
    let mut world = PhysicsWorld::new();
    let first = wall(&mut world, 2.0);
    let second = wall(&mut world, 4.0);
    let third = wall(&mut world, 6.0);
    let past_the_emitter = wall(&mut world, 12.0);
    let behind_the_ear = wall(&mut world, -3.0);
    let mut materials = AcousticMaterials::new(AcousticMaterial::FOLIAGE);
    materials.set(first, AcousticMaterial::GLASS);
    materials.set(second, AcousticMaterial::THIN_WOOD);
    materials.set(third, AcousticMaterial::METAL);
    materials.set(past_the_emitter, AcousticMaterial::CONCRETE);
    materials.set(behind_the_ear, AcousticMaterial::CONCRETE);

    let query = walk(
        &mut world,
        DVec3::ZERO,
        DVec3::new(10.0, 0.0, 0.0),
        &materials,
    );
    assert_eq!(query.occluders, 3);
    // Three hits, and the cast that found nothing past the third.
    assert_eq!(query.casts, 4);
    assert_eq!(
        query.occlusion,
        Occlusion::through([
            AcousticMaterial::GLASS,
            AcousticMaterial::THIN_WOOD,
            AcousticMaterial::METAL,
        ]),
    );
    assert_eq!(
        query.occlusion.cutoff_hz,
        AcousticMaterial::METAL.muffle_cutoff_hz
    );
}

/// **The walk stops at [`MAX_OCCLUDERS`]**, spending no more casts than that.
#[test]
fn the_walk_stops_at_the_occluder_cap() {
    let mut world = PhysicsWorld::new();
    for k in 0..MAX_OCCLUDERS + 2 {
        wall(&mut world, 1.0 + k as f64);
    }
    let materials = AcousticMaterials::new(AcousticMaterial::THIN_WOOD);
    let query = walk(
        &mut world,
        DVec3::ZERO,
        DVec3::new(20.0, 0.0, 0.0),
        &materials,
    );
    assert_eq!(query.occluders, MAX_OCCLUDERS);
    assert_eq!(query.casts, MAX_OCCLUDERS);
    assert_eq!(
        query.occlusion,
        Occlusion::through([AcousticMaterial::THIN_WOOD; MAX_OCCLUDERS]),
    );
}

/// **An ear on the emitter has nothing between them** and casts nothing.
#[test]
fn an_ear_on_the_emitter_is_clear() {
    let mut world = PhysicsWorld::new();
    wall(&mut world, 0.0);
    let materials = AcousticMaterials::new(AcousticMaterial::CONCRETE);
    let query = walk(&mut world, DVec3::ZERO, DVec3::ZERO, &materials);
    assert_eq!(query.occlusion, Occlusion::CLEAR);
    assert_eq!(query.casts, 0);
}

/// **The ray budget is honoured, and a voice it does not reach keeps its last
/// value.** Each voice behind the one wall costs two casts — the hit and the
/// miss past it — so a budget of two walks' worth reaches three of six voices
/// a frame, round robin.
#[test]
fn the_ray_budget_is_honoured_and_unreached_voices_keep_their_last_value() {
    let mut statics = PhysicsWorld::new();
    wall(&mut statics, 5.0);
    let materials = AcousticMaterials::new(AcousticMaterial::CONCRETE);
    let mut world = ClientQueryWorld::new(statics);
    let mixer = Mixer::new();
    let budget = 2 * MAX_OCCLUDERS;
    let mut tracker = OcclusionTracker::new(budget);
    let voices: Vec<VoiceId> = (0..6)
        .map(|k| {
            let id = held(&mixer);
            tracker.track(id, DVec3::new(10.0, f64::from(k) * 0.1, 0.0));
            id
        })
        .collect();
    let concrete = Occlusion::through([AcousticMaterial::CONCRETE]);
    let heard = || {
        voices
            .iter()
            .map(|id| mixer.occlusion(*id).expect("playing"))
            .collect::<Vec<_>>()
    };

    let ear = DVec3::ZERO;
    let casts = tracker.update(&mut world, ear, &materials, &mixer);
    assert!(
        casts <= budget,
        "{casts} casts against a budget of {budget}"
    );
    assert_eq!(casts, 6);
    assert_eq!(
        heard(),
        [
            concrete,
            concrete,
            concrete,
            Occlusion::CLEAR,
            Occlusion::CLEAR,
            Occlusion::CLEAR
        ]
    );

    let casts = tracker.update(&mut world, ear, &materials, &mixer);
    assert!(casts <= budget);
    assert_eq!(heard(), [concrete; 6]);

    // The ear steps past the wall, so a walk now costs the one cast that
    // finds nothing: the budget reaches five voices from the first, and the
    // sixth keeps the concrete it last had.
    let ear = DVec3::new(8.0, 0.0, 0.0);
    let casts = tracker.update(&mut world, ear, &materials, &mixer);
    assert_eq!(casts, 5);
    let clear = Occlusion::CLEAR;
    assert_eq!(heard(), [clear, clear, clear, clear, clear, concrete]);
}

/// **A budget under one walk's worth is raised to one**, or no voice would
/// ever be begun.
#[test]
fn a_budget_under_one_walk_is_raised_to_one() {
    assert_eq!(OcclusionTracker::new(0).rays_per_frame(), MAX_OCCLUDERS);
}

/// **A voice that stopped playing is dropped**, and the round robin carries on
/// at the voice it would have reached.
#[test]
fn a_voice_that_stopped_is_dropped_and_the_cursor_follows() {
    let mut statics = PhysicsWorld::new();
    wall(&mut statics, 5.0);
    let materials = AcousticMaterials::new(AcousticMaterial::CONCRETE);
    let mut world = ClientQueryWorld::new(statics);
    let mixer = Mixer::new();
    // One walk a frame: the hit and the miss, and not another walk's worth.
    let mut tracker = OcclusionTracker::new(MAX_OCCLUDERS + 1);
    let voices: Vec<VoiceId> = (0..3)
        .map(|_| {
            let id = held(&mixer);
            tracker.track(id, DVec3::new(10.0, 0.0, 0.0));
            id
        })
        .collect();
    tracker.update(&mut world, DVec3::ZERO, &materials, &mixer);
    assert_ne!(mixer.occlusion(voices[0]), Some(Occlusion::CLEAR));

    // The first voice stops; the next update must reach the second, not skip
    // to the third.
    mixer.stop(voices[0]);
    tracker.update(&mut world, DVec3::ZERO, &materials, &mixer);
    assert_eq!(tracker.len(), 2);
    assert_ne!(mixer.occlusion(voices[1]), Some(Occlusion::CLEAR));
    assert_eq!(mixer.occlusion(voices[2]), Some(Occlusion::CLEAR));
    assert!(tracker.untrack(voices[2]));
    assert!(!tracker.untrack(voices[0]));
}
