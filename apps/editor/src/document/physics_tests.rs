//! Physics on scene components in play mode: a block with a body falls,
//! collides and comes to rest; the picking boxes follow it and are never the
//! simulated bodies; stop puts the scene back; and the refusals.

use std::time::Duration;

use super::*;

use crcbl::registry::Rotation;
use crcbl::scene_physics::{BODIES, Simulation};

use crate::scene::{BLOCKS, GREYBOX};

/// The header: the greybox scene's blocks with a bodies chunk beside them.
const HEADER: &str = "Scene(\n    format: 0,\n    name: \"falling\",\n    systems: [\n        \"blocks\",\n        \"bodies\",\n    ],\n)";

/// The block that falls: lifted over the middle step, which it lands on.
pub(crate) const FALLING: SceneEntityId = SceneEntityId(4);

/// The middle step, which [`FALLING`] lands on.
const STEP: SceneEntityId = SceneEntityId(2);

/// Where [`FALLING`]'s centre starts, in metres up.
const DROP_Y: f64 = 4.0;

/// [`FALLING`]'s half extent on every axis.
const HALF: f64 = 0.5;

/// The middle step's top, which the compiled-in scene puts at its centre plus
/// its half height: 0.75 + 0.75.
const STEP_TOP: f64 = 1.5;

/// How far a resting box may sit off the surface under it, in metres.
const REST_TOLERANCE: f64 = 0.01;

/// The compiled-in blocks with [`FALLING`] added over the middle step.
fn blocks() -> String {
    falling_blocks("")
}

/// The compiled-in blocks with [`FALLING`] added over the middle step, its row
/// ending in `rotation` — a `rotation` line as the writer spells it, or
/// nothing for an unturned block.
fn falling_blocks(rotation: &str) -> String {
    let built_in = crate::scene::built_in_source();
    let text = String::from_utf8(
        built_in
            .read(Path::new(&format!("{GREYBOX}/sys/blocks.ron")))
            .expect("the compiled-in scene holds its blocks"),
    )
    .expect("utf-8");
    let falling = format!(
        "        ({}, Block(\n            position: (0.0, {DROP_Y:?}, 0.0),\n            \
         half_extents: ({HALF:?}, {HALF:?}, {HALF:?}),\n{rotation}        )),\n    ],\n)",
        FALLING.0
    );
    let text = text
        .strip_suffix("    ],\n)")
        .expect("the chunk ends with its list");
    format!("{text}{falling}")
}

/// A body row of `kind`, as the writer spells it.
fn body(id: SceneEntityId, kind: &str) -> String {
    format!(
        "        ({}, Body(\n            kind: {kind},\n            mass: 1.0,\n            \
         friction: 0.6,\n            restitution: 0.0,\n        )),\n",
        id.0
    )
}

/// A document of the greybox blocks plus [`FALLING`], with a bodies chunk of
/// `rows`.
fn document_with(rows: &str) -> Document {
    document_of(&blocks(), rows)
}

/// A document of the blocks chunk `blocks`, with a bodies chunk of `rows`.
fn document_of(blocks: &str, rows: &str) -> Document {
    let mut source = MemorySource::new();
    let bodies = format!("Chunk(\n    system: \"bodies\",\n    entities: [\n{rows}    ],\n)");
    for (key, text) in [
        ("scene.ron", HEADER.to_owned()),
        ("env.ron", {
            let built_in = crate::scene::built_in_source();
            String::from_utf8(
                built_in
                    .read(Path::new(&format!("{GREYBOX}/env.ron")))
                    .expect("the compiled-in scene holds its env"),
            )
            .expect("utf-8")
        }),
        ("sys/blocks.ron", blocks.to_owned()),
        ("sys/bodies.ron", bodies),
    ] {
        source
            .insert(Path::new(&format!("falling.scn/{key}")), text.into_bytes())
            .expect("a nested scene key is a legal asset key");
    }
    Document::open(
        &source,
        Path::new("falling.scn"),
        crate::scene::vocabulary(),
    )
    .expect("blocks with bodies is a scene")
}

/// The ground slab and the steps static, and [`FALLING`] dynamic.
pub(crate) fn falling() -> Document {
    document_with(&falling_rows())
}

/// [`falling`]'s bodies chunk rows.
fn falling_rows() -> String {
    [
        body(SceneEntityId(0), "Static"),
        body(SceneEntityId(1), "Static"),
        body(STEP, "Static"),
        body(SceneEntityId(3), "Static"),
        body(FALLING, "Dynamic"),
    ]
    .concat()
}

/// Runs `ticks` ticks of play, a tick's time at a time: one call handed it
/// all would be cut short by the clock's catch-up cap.
pub(crate) fn play_ticks(document: &mut Document, ticks: u32) {
    let period = Duration::from_secs_f64(document.world.tick_dt());
    let mut ran = 0;
    while ran < ticks {
        ran += document.advance(period);
    }
    assert_eq!(ran, ticks, "a tick's time ran more than one tick");
}

/// The ticks in `seconds` of play at the world's rate.
pub(crate) fn ticks_in(document: &Document, seconds: f64) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = (seconds / document.world.tick_dt()).round() as u32;
    ticks
}

/// `id`'s block's `position`, read the way an inspector reads it.
fn position(document: &mut Document, id: SceneEntityId) -> [f64; 3] {
    std::array::from_fn(|axis| {
        match document
            .read(id, BLOCKS, &format!("position.{axis}"))
            .expect("a block has a position")
        {
            Value::Float(value) => value,
            other => panic!("a position leaf is a number, not {other:?}"),
        }
    })
}

/// The centre of `id`'s bounds — what the picture is drawn from.
fn bounds_centre(document: &mut Document, id: SceneEntityId) -> Vec3 {
    let (min, max) = document.bounds(id).expect("the entity is placed");
    (min + max) * 0.5
}

/// A ray straight down `-Z` at `(x, y)`.
fn ray_at(x: f64, y: f64) -> Ray {
    Ray::new(DVec3::new(x, y, 20.0), DVec3::NEG_Z)
}

/// **In play, a dynamic body falls and its block's position moves, the
/// bounds and the pick follow, and it comes to rest on the static step below
/// it** — which never moves. Stop puts every file back byte for byte, and the
/// block picks where it was placed again.
#[test]
fn a_block_with_a_body_falls_in_play_and_stop_puts_it_back() {
    let mut document = falling();
    let before = document.files().expect("the scene saves");
    let step = position(&mut document, STEP);
    assert_eq!(
        document.pick(&ray_at(0.0, DROP_Y)),
        Some(FALLING),
        "the falling block does not pick where it was placed",
    );

    document.play().expect("every body here is placed");
    assert_eq!(document.playing_modules(), [BODIES]);
    let quarter = ticks_in(&document, 0.25);
    play_ticks(&mut document, quarter);
    let fell = position(&mut document, FALLING);
    let expected = DROP_Y - 0.5 * 9.81 * 0.25 * 0.25;
    assert!(
        (fell[1] - expected).abs() < 0.05,
        "a quarter second in, the block is at {fell:?}, not near y = {expected}",
    );
    let centre = bounds_centre(&mut document, FALLING);
    assert!(
        (f64::from(centre.y) - fell[1]).abs() < 1e-5,
        "the bounds stand at {centre:?} while the block is at {fell:?}",
    );

    let rest = ticks_in(&document, 3.0);
    play_ticks(&mut document, rest);
    let rested = position(&mut document, FALLING);
    assert!(
        (rested[1] - (STEP_TOP + HALF)).abs() < REST_TOLERANCE,
        "the block came to rest at {rested:?}, not on the step's top",
    );
    assert_eq!(position(&mut document, STEP), step, "the static step moved");
    assert_eq!(
        document.pick(&ray_at(0.0, rested[1])),
        Some(FALLING),
        "the block does not pick where it came to rest",
    );
    assert_eq!(
        document.pick(&ray_at(0.0, DROP_Y)),
        None,
        "the block still picks where it fell from",
    );

    assert!(document.stop().expect("the snapshot reloads"));
    assert_eq!(
        document.files().expect("the scene saves"),
        before,
        "stop did not put the scene back byte for byte",
    );
    assert_eq!(document.pick(&ray_at(0.0, DROP_Y)), Some(FALLING));
}

/// `id`'s block's orientation, read the way an inspector reads it.
fn rotation(document: &mut Document, id: SceneEntityId) -> DQuat {
    let leaves = Rotation::LEAVES.map(|leaf| {
        match document
            .read(id, BLOCKS, &format!("rotation.{leaf}"))
            .expect("a block has a rotation")
        {
            Value::Float(value) => value,
            other => panic!("a rotation leaf is a number, not {other:?}"),
        }
    });
    DQuat::from_array(leaves)
}

/// **In play, a turned block with a body lands on its edge and tips onto a
/// face, its block's `rotation` written back as it turns, and the block picks
/// where it rests; stop puts every file back byte for byte**, the turn in
/// the file included.
#[test]
fn a_turned_block_tips_in_play_and_stop_puts_its_rotation_back() {
    let tipped = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_6);
    let [x, y, z, w] = tipped.to_array();
    let line = format!("            rotation: ({x:?}, {y:?}, {z:?}, {w:?}),\n");
    let mut document = document_of(&falling_blocks(&line), &falling_rows());
    let before = document.files().expect("the scene saves");
    assert_eq!(rotation(&mut document, FALLING), tipped);

    document.play().expect("every body here is placed");
    let rest = ticks_in(&document, 3.0);
    play_ticks(&mut document, rest);
    let turned = rotation(&mut document, FALLING);
    let upright = [DVec3::X, DVec3::Y, DVec3::Z]
        .map(|axis| (turned * axis).y.abs())
        .into_iter()
        .fold(0.0, f64::max);
    assert!(
        upright > 1.0 - 1e-3,
        "the block came to rest turned {turned:?}, not on a face",
    );
    let rested = position(&mut document, FALLING);
    assert!(
        (rested[1] - (STEP_TOP + HALF)).abs() < REST_TOLERANCE,
        "the block came to rest at {rested:?}, not flat on the step's top",
    );
    assert_eq!(
        document.pick(&ray_at(rested[0], rested[1])),
        Some(FALLING),
        "the block does not pick where it came to rest",
    );
    assert_ne!(
        document.files().expect("the scene saves"),
        before,
        "play changed nothing, so the restore below proves nothing",
    );

    assert!(document.stop().expect("the snapshot reloads"));
    assert_eq!(
        document.files().expect("the scene saves"),
        before,
        "stop did not put the scene back byte for byte",
    );
    assert_eq!(rotation(&mut document, FALLING), tipped);
}

/// **The picking boxes are never the simulated bodies**: in edit mode and in
/// play, the document's own `PhysicsSystem` holds a kinematic box for every
/// placed entity and nothing else, and the simulation is a system of its own
/// that exists only while the scene plays.
#[test]
fn the_picking_boxes_stay_kinematic_and_apart_from_the_simulation() {
    let mut document = falling();
    let placed = document.entity_count();
    let check = |document: &mut Document| {
        let entities: Vec<Entity> = (0..=FALLING.0)
            .map(|id| {
                document
                    .ids
                    .entity(SceneEntityId(id))
                    .expect("in the scene")
            })
            .collect();
        let picking = document
            .world
            .system_mut::<PhysicsSystem>()
            .expect("the document's picking physics");
        assert_eq!(picking.collider_count(), placed);
        for entity in entities {
            let body = picking.body(entity).expect("a picking box has a body");
            assert!(
                !body.is_dynamic(),
                "{entity:?}'s picking box is a simulated body"
            );
        }
    };

    check(&mut document);
    assert!(document.world.system_mut::<Simulation>().is_none());
    document.play().expect("plays");
    let quarter = ticks_in(&document, 0.25);
    play_ticks(&mut document, quarter);
    check(&mut document);
    let simulation = document
        .world
        .system_mut::<Simulation>()
        .expect("the bodies' module registered its simulation");
    assert_eq!(simulation.physics().collider_count(), placed);
    document.stop().expect("stops");
    check(&mut document);
    assert!(
        document.world.system_mut::<Simulation>().is_none(),
        "the simulation outlived play",
    );
}

/// **A body with nothing placing it refuses play, naming the entity**, and
/// the document goes on editing.
#[test]
fn a_body_with_no_placement_refuses_play_by_name() {
    let rows = [
        body(SceneEntityId(0), "Static"),
        body(SceneEntityId(9), "Dynamic"),
    ]
    .concat();
    let mut document = document_with(&rows);
    let error = document
        .play()
        .expect_err("entity 9 has a body and no block");
    assert!(
        matches!(&error, EditError::Unplayable(reason)
            if reason.contains("#9") && reason.contains("nothing that places it")),
        "{error}",
    );
    assert_eq!(document.play_state(), PlayState::Editing);
}

/// **A body attached in the editor makes its block fall when the scene
/// plays**: the add button's command, then play.
#[test]
fn a_body_attached_in_the_editor_falls_in_play() {
    let mut document = document_with(&body(SceneEntityId(0), "Static"));
    assert!(document.attachable(FALLING).contains(&BODIES.to_owned()));
    document
        .attach(FALLING, BODIES)
        .expect("a new body attaches");
    document.play().expect("plays");
    let second = ticks_in(&document, 1.0);
    play_ticks(&mut document, second);
    assert!(
        position(&mut document, FALLING)[1] < DROP_Y - 1.0,
        "a block with a newly attached body did not fall",
    );
}

/// The positions of the blocks filed under `ids` after `seconds` of play,
/// as bits.
fn played_bits(document: &mut Document, ids: &[u32], seconds: f64) -> Vec<u64> {
    document.play().expect("plays");
    let ticks = ticks_in(document, seconds);
    play_ticks(document, ticks);
    let bits = ids
        .iter()
        .flat_map(|&id| position(document, SceneEntityId(id)))
        .map(f64::to_bits)
        .collect();
    document.stop().expect("stops");
    bits
}

/// How many blocks [`two_plays_of_one_scene_are_identical_even_after_edits`]
/// piles up: enough that they land on each other in several contacts at once,
/// where the order bodies are solved in shows in the result.
const PILE: u32 = 8;

/// Where the `n`th block of the pile starts, `(x, y, z)`: staggered so the
/// blocks tumble into each other, over the ground slab clear of the steps.
fn pile_at(n: u32) -> [f64; 3] {
    [
        f64::from(n % 3) * 0.37 - 0.4,
        0.6 + f64::from(n) * 0.9,
        4.0 + f64::from(n % 2) * 0.29,
    ]
}

/// **Two plays of the same scene end in the same poses, to the bit — even
/// when an edit history between them left the bodies stored in another
/// order**, because play runs the world its snapshot loads into.
#[test]
fn two_plays_of_one_scene_are_identical_even_after_edits() {
    let rows = [body(SceneEntityId(0), "Static"), body(FALLING, "Dynamic")].concat();
    let mut document = document_with(&rows);
    let ids: Vec<u32> = (0..PILE).map(|n| FALLING.0 + n).collect();
    for (n, &id) in (0..).zip(&ids) {
        let id = SceneEntityId(id);
        if id != FALLING {
            // Masses and frictions that differ, so no two bodies are
            // interchangeable.
            let rows = vec![
                SystemRow {
                    system: BLOCKS.to_owned(),
                    row: "(position:(0.0,0.0,0.0),half_extents:(0.4,0.4,0.4))".to_owned(),
                },
                SystemRow {
                    system: BODIES.to_owned(),
                    row: format!(
                        "(kind:Dynamic,mass:{:?},friction:0.{},restitution:0.1)",
                        1.0 + f64::from(n),
                        n % 9 + 1,
                    ),
                },
            ];
            document
                .apply(EditCommand::Spawn {
                    entity: id,
                    rows,
                    name: None,
                })
                .expect("a free id");
        }
        for (axis, value) in pile_at(n).into_iter().enumerate() {
            document
                .apply(EditCommand::SetProperty {
                    entity: id,
                    system: BLOCKS.to_owned(),
                    path: format!("position.{axis}"),
                    value: Value::Float(value),
                })
                .expect("a block has a position");
        }
    }
    let first = played_bits(&mut document, &ids, 3.0);
    let top = first.len() - 2;
    assert_ne!(
        f64::from_bits(first[top]),
        pile_at(PILE - 1)[1],
        "the top of the pile never fell, so nothing was compared",
    );
    assert_eq!(
        played_bits(&mut document, &ids, 3.0),
        first,
        "a second play of the same scene ended elsewhere",
    );

    // Delete the first block of the pile and bring it back: its body is stored
    // last now, and the last one's in its place, where the files spell them
    // in id order.
    document.delete(FALLING).expect("deletes");
    assert!(document.undo().expect("undoes"));
    assert_eq!(
        played_bits(&mut document, &ids, 3.0),
        first,
        "a play after an edit history ended elsewhere than a play of its files",
    );
}
