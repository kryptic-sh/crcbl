use super::*;

use crcbl::engine::{ExitReason, Flow};
use crcbl::reflect::Value;
use crcbl::render::shadow::Cadence;
use crcbl::shell::HeadlessShell;

use crate::app::{Editor, tests::headless};
use crate::command::EditCommand;
use crate::keys::Action;

fn step(editor: &mut Editor<HeadlessShell>, cached: bool) {
    assert_eq!(editor.frame().expect("a presented frame"), Flow::Continue);
    assert_eq!(
        editor.renderer.shadow_atlas_cached(),
        cached,
        "unchanged descriptions must let shadow inputs become still"
    );
}

fn pose(
    editor: &Editor<HeadlessShell>,
    id: SceneEntityId,
    current: &InstanceDesc,
    previous: &InstanceDesc,
) {
    let placed = editor
        .instances
        .instances
        .iter()
        .find(|placed| placed.drawn == Drawn::Scene(id))
        .expect("placed");
    assert_eq!(
        placed.desc, *current,
        "the mirror holds the last publication"
    );
    let (records, _) = editor.renderer.cull_records();
    let index = usize::try_from(placed.handle.index()).expect("a host index");
    let actual = &records[index];
    assert_eq!(actual.transform, current.transform.to_cols_array());
    assert_eq!(
        actual.previous_transform,
        previous.transform.to_cols_array()
    );
}

fn edit<S: crcbl::shell::Shell + ?Sized>(
    editor: &mut Editor<S>,
    id: SceneEntityId,
    x: f64,
) -> InstanceDesc {
    editor
        .document
        .apply(EditCommand::SetProperty {
            entity: id,
            system: crate::scene::BLOCKS.to_owned(),
            path: "position.0".into(),
            value: Value::Float(x),
        })
        .expect("a position edit");
    instance_of(&mut editor.document, Drawn::Scene(id)).expect("bounds after the edit")
}

#[test]
fn unchanged_draws_settle_motion_and_edits_follow_undo_redo() {
    let mut editor = headless(64);
    editor
        .renderer
        .set_shadow_cadence(Some(Cadence::EVERY_FRAME));
    let id = SceneEntityId(0);
    let original = instance_of(&mut editor.document, Drawn::Scene(id)).expect("built-in bounds");
    let files = editor.document.files().expect("scene files");
    let Value::Float(x) = editor
        .document
        .read(id, crate::scene::BLOCKS, "position.0")
        .expect("position")
    else {
        panic!("position is a number");
    };
    step(&mut editor, false);
    pose(&editor, id, &original, &original);
    step(&mut editor, true);
    step(&mut editor, true);

    let moved = edit(&mut editor, id, x + 1.0);
    step(&mut editor, false);
    pose(&editor, id, &moved, &original);
    step(&mut editor, false);
    pose(&editor, id, &moved, &moved);
    step(&mut editor, true);

    editor.act(&Action::Undo);
    assert_eq!(editor.document.files().expect("scene files"), files);
    step(&mut editor, false);
    pose(&editor, id, &original, &moved);
    editor.act(&Action::Redo);
    step(&mut editor, false);
    pose(&editor, id, &moved, &original);
    editor.act(&Action::Undo);
    step(&mut editor, false);
    pose(&editor, id, &original, &moved);

    let replacement = edit(&mut editor, id, x + 2.0);
    assert_eq!(editor.document.log().position(), 1);
    assert!(!editor.document.redo().expect("discarded future"));
    step(&mut editor, false);
    pose(&editor, id, &replacement, &original);
    step(&mut editor, false);
    pose(&editor, id, &replacement, &replacement);
    step(&mut editor, true);
    editor.act(&Action::Undo);
    assert_eq!(editor.document.files().expect("scene files"), files);
    step(&mut editor, false);
    pose(&editor, id, &original, &replacement);
    step(&mut editor, false);
    pose(&editor, id, &original, &original);
    step(&mut editor, true);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// The scene ids with an instance and how many of the renderer's records are
/// live.
fn drawn(editor: &Editor<HeadlessShell>) -> (Vec<SceneEntityId>, usize) {
    let mut ids: Vec<_> = editor
        .instances
        .instances
        .iter()
        .filter_map(|each| match each.drawn {
            Drawn::Scene(id) => Some(id),
            Drawn::Spawned(_) => None,
        })
        .collect();
    ids.sort();
    let live = editor
        .renderer
        .cull_records()
        .0
        .iter()
        .filter(|record| record.flags & crcbl::shaders::mesh::GpuInstance::LIVE != 0)
        .count();
    (ids, live)
}

/// **An entity that leaves stops being drawn and one that arrives starts** —
/// read off the renderer's own live records, not the editor's list alone.
#[test]
fn deleted_and_spawned_entities_leave_and_join_the_drawn_instances() {
    let mut editor = headless(16);
    step_any(&mut editor);
    let ids = |range: &[u32]| range.iter().copied().map(SceneEntityId).collect::<Vec<_>>();
    assert_eq!(drawn(&editor), (ids(&[0, 1, 2, 3]), 4));

    editor
        .document
        .delete(SceneEntityId(2))
        .expect("in the scene");
    step_any(&mut editor);
    assert_eq!(
        drawn(&editor),
        (ids(&[0, 1, 3]), 3),
        "the deleted block is still drawn"
    );

    editor
        .document
        .duplicate(SceneEntityId(1))
        .expect("in the scene");
    step_any(&mut editor);
    assert_eq!(
        drawn(&editor),
        (ids(&[0, 1, 3, 4]), 4),
        "the copy is not drawn"
    );

    editor.act(&Action::Undo);
    editor.act(&Action::Undo);
    step_any(&mut editor);
    assert_eq!(drawn(&editor), (ids(&[0, 1, 2, 3]), 4));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **An entity whose placing component is detached stops being drawn**, and
/// the undo draws it again — a sun alone is no thing in space.
#[test]
fn an_entity_that_loses_its_placement_stops_being_drawn() {
    let mut editor = headless(16);
    step_any(&mut editor);
    editor.document = crate::document::systems_tests::two_systems();
    let ids = |range: &[u32]| range.iter().copied().map(SceneEntityId).collect::<Vec<_>>();
    // Any edit that moves the membership reconciles the swapped document; a
    // sun attached to a block leaves it drawn as the block.
    editor
        .document
        .attach(SceneEntityId(2), crate::document::systems_tests::SUN)
        .expect("2 has no sun");
    step_any(&mut editor);
    assert_eq!(
        drawn(&editor),
        (ids(&[0, 1, 2, 3]), 4),
        "a lone sun was drawn"
    );

    editor
        .document
        .detach(SceneEntityId(1), crate::scene::BLOCKS)
        .expect("1 keeps its sun");
    step_any(&mut editor);
    assert_eq!(
        drawn(&editor),
        (ids(&[0, 2, 3]), 3),
        "the detached block is still drawn"
    );

    editor.act(&Action::Undo);
    step_any(&mut editor);
    assert_eq!(drawn(&editor), (ids(&[0, 1, 2, 3]), 4));
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// A frame, whatever the shadow cache did.
fn step_any(editor: &mut Editor<HeadlessShell>) {
    assert_eq!(editor.frame().expect("a presented frame"), Flow::Continue);
}

/// **A block falling under its body moves its drawn instance**: the pose the
/// simulation writes into the block reaches the renderer's records through
/// the instances' ordinary publish, with nothing physics-specific drawing it.
#[test]
fn a_falling_body_moves_its_drawn_instance() {
    use crate::document::physics_tests::{FALLING, falling, play_ticks, ticks_in};

    let mut editor = headless(16);
    step_any(&mut editor);
    editor.document = falling();
    // Play moves the membership, so the next frame reconciles the swapped
    // document's instances.
    editor.document.play().expect("every body here is placed");
    step_any(&mut editor);
    let height = |editor: &Editor<HeadlessShell>| {
        let placed = editor
            .instances
            .instances
            .iter()
            .find(|placed| placed.drawn == Drawn::Scene(FALLING))
            .expect("the falling block is drawn");
        let (records, _) = editor.renderer.cull_records();
        let index = usize::try_from(placed.handle.index()).expect("a host index");
        assert_eq!(
            records[index].transform,
            placed.desc.transform.to_cols_array(),
            "the renderer holds another pose than the one published",
        );
        placed.desc.transform.w_axis.y
    };
    let before = height(&editor);
    let half = ticks_in(&editor.document, 0.5);
    play_ticks(&mut editor.document, half);
    step_any(&mut editor);
    let after = height(&editor);
    assert!(
        after < before - 0.5,
        "the instance stood at y = {before} and then {after}: it did not fall with its block",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

#[test]
fn missing_bounds_leave_the_published_instance_unchanged() {
    let mut editor = headless(16);
    editor
        .renderer
        .set_shadow_cadence(Some(Cadence::EVERY_FRAME));
    step(&mut editor, false);
    step(&mut editor, true);
    let (before, _) = editor.renderer.cull_records();
    let drawn = editor.instances.instances[0].drawn;
    let desc = editor.instances.instances[0].desc;
    editor.instances.instances[0].drawn = Drawn::Scene(SceneEntityId(u32::MAX));
    assert!(instance_of(&mut editor.document, editor.instances.instances[0].drawn).is_none());
    step(&mut editor, true);
    assert_eq!(editor.instances.instances[0].desc, desc);
    assert_eq!(editor.renderer.cull_records().0, before);
    editor.instances.instances[0].drawn = drawn;
    step(&mut editor, true);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a Vulkan adapter and screenshot readback"]
fn filtered_editor_images_match_eager_writes_through_history() {
    use crcbl::args::Invocation;
    use crcbl::engine::ScreenshotRequest;
    use crcbl::sprite::load::{Rgba8, decode_png};

    let _ = crcbl::core::log::init_logging();

    fn capture(eager: bool) -> Vec<Rgba8> {
        let Invocation::Run(options) = crate::args::parse(
            [
                "--headless",
                "--backend",
                "vk",
                "--size",
                "960x720",
                "--pacing",
                "off",
                "--fps",
                "0",
                "--frames",
                "64",
            ]
            .into_iter()
            .map(str::to_owned),
        ) else {
            panic!("valid headless Vulkan options");
        };
        let mut editor = Editor::start(&options).expect("Vulkan editor");
        editor
            .renderer
            .set_shadow_cadence(Some(Cadence::EVERY_FRAME));
        let directory = tempfile::tempdir().expect("capture directory");
        let id = SceneEntityId(0);
        let Value::Float(x) = editor
            .document
            .read(id, crate::scene::BLOCKS, "position.0")
            .expect("position")
        else {
            panic!("position is a number");
        };
        let mut images = Vec::new();
        for frame in 1..=14 {
            match frame {
                4 => {
                    edit(&mut editor, id, x + 1.0);
                }
                7 | 9 | 12 => editor.act(&Action::Undo),
                8 => editor.act(&Action::Redo),
                10 => {
                    edit(&mut editor, id, x + 2.0);
                    assert_eq!(editor.document.log().position(), 1);
                    assert!(!editor.document.redo().expect("discarded future"));
                }
                _ => {}
            }
            if eager {
                for placed in &editor.instances.instances {
                    if let Some(desc) = instance_of(&mut editor.document, placed.drawn) {
                        editor.renderer.set_instance(placed.handle, &desc);
                    }
                }
            }
            let path = directory.path().join(format!("{frame}.png"));
            editor.gpu.set_screenshot(ScreenshotRequest {
                path: path.clone(),
                frame,
            });
            assert_eq!(editor.frame().expect("captured frame"), Flow::Continue);
            images.push(
                decode_png(&std::fs::read(path).expect("written screenshot")).expect("RGBA image"),
            );
        }
        editor.finish(ExitReason::FrameBudget).expect("teardown");
        images
    }

    let eager = capture(true);
    let filtered = capture(false);
    assert_eq!(eager.len(), filtered.len());
    assert_ne!(
        eager[2], eager[3],
        "the position edit must change the rendered image"
    );
    for (frame, (expected, actual)) in eager.iter().zip(&filtered).enumerate() {
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        assert_eq!(actual.pixels.len(), expected.pixels.len());
        let differing = actual
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.pixels.as_chunks::<4>().0)
            .filter(|(actual, expected)| actual != expected)
            .count();
        println!("rendered frame {}: {differing} differing pixels", frame + 1);
        assert_eq!(differing, 0, "rendered frame {}", frame + 1);
    }
}

#[test]
fn filtered_instances_drain_uploads_and_settle_each_ring_slot() {
    use crcbl::hal::null::{Event, NullInstance, ObjectKind, Recorder};
    use crcbl::hal::{DeviceDesc, Format, Instance, QueueKind};
    use crcbl::render::{Camera, DirectionalLight};
    use crcbl::shaders::mesh::INSTANCE_STRIDE;
    use std::collections::HashSet;
    use std::path::Path;

    let recorder = Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc::for_adapter(adapter.id))
        .expect("null device");
    let queue = device.queue(QueueKind::Graphics).expect("graphics queue");
    let mut renderer = ForwardRenderer::with_scene(
        device.as_ref(),
        queue,
        Format::Rgba8UnormSrgb,
        &crcbl::greybox::scene3d(),
    )
    .expect("greybox renderer");
    let mut document = Document::open(
        &crate::scene::built_in_source(),
        Path::new(crate::scene::GREYBOX),
        crate::scene::vocabulary(),
    )
    .expect("greybox document");
    let mut placed = Placed::place(&mut renderer, &mut document).expect("placed entities");
    let buffers: HashSet<_> = recorder
        .events()
        .windows(2)
        .filter_map(|events| match (&events[0], &events[1]) {
            (
                Event::Created {
                    kind: ObjectKind::Buffer,
                    label: Some(label),
                },
                Event::BufferWritten { buffer, .. },
            ) if label.starts_with("forward instances instances ") => Some(*buffer),
            _ => None,
        })
        .collect();
    assert!(buffers.len() > 1, "observe the actual instance-buffer ring");
    let camera = Camera::default();
    let light = DirectionalLight::default();
    let tick = |renderer: &mut ForwardRenderer, placed: &mut Placed, document: &mut Document| {
        let seen = recorder.events().len();
        placed
            .update(renderer, document)
            .expect("no entity arrives");
        renderer
            .begin_frame(device.as_ref(), &camera, &light, (64, 64))
            .expect("frame uploads");
        recorder.events()[seen..]
            .iter()
            .filter_map(|event| match event {
                Event::BufferWritten { buffer, len, .. } if buffers.contains(buffer) => Some(*len),
                _ => None,
            })
            .sum::<usize>()
    };
    for _ in 0..buffers.len() {
        assert!(tick(&mut renderer, &mut placed, &mut document) > 0);
    }
    for _ in 0..buffers.len() {
        assert_eq!(tick(&mut renderer, &mut placed, &mut document), 0);
    }
    let Drawn::Scene(id) = placed.instances[0].drawn else {
        panic!("the greybox scene spawns nothing");
    };
    let original = placed.instances[0].desc;
    let Value::Float(x) = document
        .read(id, crate::scene::BLOCKS, "position.0")
        .expect("position")
    else {
        panic!("number");
    };
    document
        .apply(EditCommand::SetProperty {
            entity: id,
            system: crate::scene::BLOCKS.to_owned(),
            path: "position.0".into(),
            value: Value::Float(x + 1.0),
        })
        .expect("move");
    let moved = instance_of(&mut document, Drawn::Scene(id)).expect("moved bounds");
    assert_eq!(
        tick(&mut renderer, &mut placed, &mut document),
        INSTANCE_STRIDE
    );
    let index = usize::try_from(placed.instances[0].handle.index()).expect("host index");
    let moving = &renderer.cull_records().0[index];
    assert_eq!(moving.transform, moved.transform.to_cols_array());
    assert_eq!(
        moving.previous_transform,
        original.transform.to_cols_array()
    );
    assert_eq!(
        tick(&mut renderer, &mut placed, &mut document),
        INSTANCE_STRIDE
    );
    let settled = &renderer.cull_records().0[index];
    assert_eq!(settled.transform, moved.transform.to_cols_array());
    assert_eq!(settled.previous_transform, moved.transform.to_cols_array());
    for _ in 1..buffers.len() {
        assert_eq!(
            tick(&mut renderer, &mut placed, &mut document),
            INSTANCE_STRIDE
        );
    }
    for _ in 0..buffers.len() {
        assert_eq!(tick(&mut renderer, &mut placed, &mut document), 0);
    }
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
    assert_eq!(recorder.total_live_objects(), 0);
}
