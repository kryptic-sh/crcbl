//! The scene's tests: the selection's keys, its life across a despawn, and
//! the sections a selected entity puts in the panel.

use super::*;

const TICK: f64 = 1.0 / 60.0;

fn scene() -> Scene {
    Scene::new(DirectionalLight::default())
}

fn press(scene: &mut Scene, key: KeyCode) {
    assert!(scene.key(key, true), "{key:?} is a selection key");
    assert!(scene.key(key, false), "and so is its release");
}

/// The panel the scene fills, shown and gathered for one frame.
fn panel_of(scene: &Scene) -> DebugPanel {
    let mut panel = DebugPanel::new();
    panel.set_visible(true);
    panel.begin_frame();
    scene.debug_sections(&mut panel);
    panel
}

fn titles(panel: &DebugPanel) -> Vec<&str> {
    panel.sections().iter().map(DebugSection::title).collect()
}

/// `label`'s value in the section titled `title`.
fn value<'p>(panel: &'p DebugPanel, title: &str, label: &str) -> &'p str {
    let section = panel
        .sections()
        .iter()
        .find(|section| section.title() == title)
        .unwrap_or_else(|| panic!("no {title} section: {:?}", titles(panel)));
    section
        .rows()
        .iter()
        .find(|row| row.label == label)
        .unwrap_or_else(|| panic!("no {label} row in {title}: {:?}", section.rows()))
        .value
        .as_str()
}

#[test]
fn the_keys_step_through_the_entities_and_wrap() {
    let mut scene = scene();
    let (cube, sun) = (scene.cube(), scene.sun());
    assert_eq!(scene.selected(), None, "nothing is selected at the start");

    assert!(scene.key(SELECT_NEXT_KEY, true));
    assert_eq!(scene.selected(), Some(cube), "the press steps");
    assert!(scene.key(SELECT_NEXT_KEY, false));
    assert_eq!(scene.selected(), Some(cube), "the release does not");
    press(&mut scene, SELECT_NEXT_KEY);
    assert_eq!(scene.selected(), Some(sun));
    press(&mut scene, SELECT_NEXT_KEY);
    assert_eq!(
        scene.selected(),
        Some(cube),
        "past the last wraps to the first"
    );
    press(&mut scene, SELECT_PREVIOUS_KEY);
    assert_eq!(
        scene.selected(),
        Some(sun),
        "before the first wraps to the last"
    );
    press(&mut scene, SELECT_PREVIOUS_KEY);
    assert_eq!(scene.selected(), Some(cube));

    assert!(
        !scene.key(KeyCode::KeyA, true),
        "any other key is left for the rest of the sandbox"
    );
    assert_eq!(scene.selected(), Some(cube));
}

#[test]
fn the_selection_survives_ticks_and_clears_when_its_entity_is_swept() {
    let mut scene = scene();
    press(&mut scene, SELECT_NEXT_KEY);
    let cube = scene.cube();
    scene.tick(TICK);
    scene.tick(TICK);
    assert_eq!(scene.selected(), Some(cube), "a live selection is kept");

    scene.world_mut().despawn(cube);
    scene.tick(TICK);
    assert_eq!(scene.selected(), None, "a swept entity is not selected");
    assert_eq!(scene.cube_seconds(), None, "and its spin went with it");
    assert_eq!(
        value(&panel_of(&scene), SCENE_SECTION, "entities"),
        "1",
        "the sun is still there"
    );
}

#[test]
fn a_selected_entity_lists_each_owning_systems_fields() {
    let mut scene = scene();
    assert_eq!(
        titles(&panel_of(&scene)),
        [SCENE_SECTION],
        "no selection, no system sections"
    );
    assert_eq!(value(&panel_of(&scene), SCENE_SECTION, "selected"), "none");

    for _ in 0..30 {
        scene.tick(TICK);
    }
    press(&mut scene, SELECT_NEXT_KEY);
    let panel = panel_of(&scene);
    assert_eq!(
        titles(&panel),
        [SCENE_SECTION, SPIN],
        "the cube is in the spin system only"
    );
    let cube = scene.cube();
    assert_eq!(
        value(&panel, SCENE_SECTION, "selected"),
        format!("{}v{}", cube.index(), cube.generation()),
    );
    assert_eq!(
        value(&panel, SPIN, "seconds"),
        "0.500",
        "half a second of 60 Hz ticks, read live off the component"
    );

    press(&mut scene, SELECT_NEXT_KEY);
    let panel = panel_of(&scene);
    assert_eq!(titles(&panel), [SCENE_SECTION, SUN]);
    let light = DirectionalLight::default();
    assert_eq!(
        value(&panel, SUN, "direction"),
        format!(
            "{:.3}, {:.3}, {:.3}",
            light.direction.x, light.direction.y, light.direction.z
        ),
    );
}

#[test]
fn the_world_is_what_the_frame_draws() {
    let light = DirectionalLight {
        ambient: crcbl::math::Vec3::new(0.1, 0.2, 0.3),
        ..DirectionalLight::default()
    };
    let mut scene = Scene::new(light);
    assert_eq!(scene.light(), Some(light), "the sun round-trips exactly");

    let mut expected = 0.0f32;
    for _ in 0..7 {
        scene.tick(TICK);
        #[allow(clippy::cast_possible_truncation)]
        let step = TICK as f32;
        expected += step;
    }
    assert_eq!(
        scene.cube_seconds().map(f32::to_bits),
        Some(expected.to_bits()),
        "the same f32 sum the GPU used to keep, bit for bit"
    );
}
