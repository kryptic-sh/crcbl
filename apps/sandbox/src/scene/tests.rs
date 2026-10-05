//! The scene's tests: the selection's keys, its life across a despawn, and
//! the sections a selected entity puts in the panel.

use super::*;

const TICK: f64 = 1.0 / 60.0;

fn scene() -> Scene {
    Scene::new(DirectionalLight::default(), None)
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
        [SCENE_SECTION, SYSTEMS_SECTION],
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
        [SCENE_SECTION, SYSTEMS_SECTION, SPIN],
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
    assert_eq!(titles(&panel), [SCENE_SECTION, SYSTEMS_SECTION, SUN]);
    let light = DirectionalLight::default();
    assert_eq!(
        value(&panel, SUN, "direction"),
        format!(
            "{:.3}, {:.3}, {:.3}",
            light.direction.x, light.direction.y, light.direction.z
        ),
    );
}

/// A clock that answers each read with the next of a scripted list of
/// readings — which is how long every system's tick took, two reads a tick.
#[derive(Debug)]
struct Readings(std::cell::RefCell<std::collections::VecDeque<u64>>);

impl Readings {
    fn millis(readings: &[u64]) -> Self {
        Self(std::cell::RefCell::new(readings.iter().copied().collect()))
    }
}

impl TimeSource for Readings {
    fn elapsed(&self) -> core::time::Duration {
        let reading = self
            .0
            .borrow_mut()
            .pop_front()
            .expect("the test scripted every read the schedule takes");
        core::time::Duration::from_millis(reading)
    }
}

/// The rows of the section titled `title`, as `(label, value)` pairs.
fn rows<'p>(panel: &'p DebugPanel, title: &str) -> Vec<(&'p str, &'p str)> {
    panel
        .sections()
        .iter()
        .find(|section| section.title() == title)
        .unwrap_or_else(|| panic!("no {title} section: {:?}", titles(panel)))
        .rows()
        .iter()
        .map(|row| (row.label.as_str(), row.value.as_str()))
        .collect()
}

#[test]
fn the_systems_section_has_a_row_per_system_with_its_tick_time() {
    // Two ticks of two systems: spin takes 2 then 4 ms, the sun 3 then 1.
    let clock = Readings::millis(&[0, 2, 2, 5, 10, 14, 14, 15]);
    let mut scene = Scene::new(DirectionalLight::default(), Some(Box::new(clock)));
    assert_eq!(
        rows(&panel_of(&scene), SYSTEMS_SECTION),
        [(SPIN, PENDING), (SUN, PENDING)],
        "timed, but not ticked yet"
    );

    scene.tick(TICK);
    scene.tick(TICK);
    assert_eq!(
        rows(&panel_of(&scene), SYSTEMS_SECTION),
        [
            (SPIN, "4.000 ms, avg 3.000 ms"),
            (SUN, "1.000 ms, avg 2.000 ms")
        ],
        "in schedule order, each the clock's own measure"
    );
}

/// A system that declares what it was built with and does nothing.
struct Declaring {
    name: &'static str,
    access: crcbl::ecs::Access,
}

impl crcbl::ecs::SystemTrait for Declaring {
    fn name(&self) -> &str {
        self.name
    }
    fn access(&self) -> crcbl::ecs::Access {
        self.access.clone()
    }
    fn tick(&mut self, _dt: f64) {}
    fn entity_count(&self) -> usize {
        0
    }
    fn sweep(&mut self, _dead: &[Entity]) {}
    fn debug_draw(&mut self, _ctx: &crcbl::ecs::DebugCtx) {}
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A system whose declared access conflicts with earlier ones says which, and
/// one that conflicts with nothing — every system of the scene's own — says
/// nothing more.
#[test]
fn a_system_that_must_run_after_others_names_them_in_its_row() {
    use crcbl::ecs::{Access, Shared};

    let mut scene = scene();
    let world = scene.world_mut();
    world
        .share(&Shared::new("wind", 0.0_f32))
        .expect("the world's only resource");
    for (name, access) in [
        ("gust", Access::none().writes("wind")),
        ("sail", Access::none().reads("wind")),
        ("kite", Access::none().writes("wind")),
    ] {
        world.register_system(Box::new(Declaring { name, access }));
    }

    assert_eq!(
        rows(&panel_of(&scene), SYSTEMS_SECTION),
        [
            (SPIN, UNTIMED),
            (SUN, UNTIMED),
            ("gust", UNTIMED),
            ("sail", "untimed, after gust"),
            ("kite", "untimed, after gust, sail"),
        ]
    );
}

#[test]
fn a_scene_given_no_clock_says_its_systems_are_untimed() {
    let mut scene = scene();
    scene.tick(TICK);
    assert_eq!(
        rows(&panel_of(&scene), SYSTEMS_SECTION),
        [(SPIN, UNTIMED), (SUN, UNTIMED)]
    );
}

#[test]
fn the_world_is_what_the_frame_draws() {
    let light = DirectionalLight {
        ambient: crcbl::math::Vec3::new(0.1, 0.2, 0.3),
        ..DirectionalLight::default()
    };
    let mut scene = Scene::new(light, None);
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

/// A checked `sv_spin_rate` set, as the console hands one over.
fn spin_rate(value: &str) -> crcbl::console::SimSet {
    crate::spin::sim_registry()
        .sim_set("sv_spin_rate", value)
        .expect("in range")
}

#[test]
fn an_offline_set_waits_for_the_next_tick_and_that_tick_spins_at_it() {
    let mut scene = scene();
    scene.tick(TICK);
    scene.submit_sim_set(spin_rate("3"));
    assert_eq!(
        scene.sim_vars().f32(&crate::spin::sv_spin_rate),
        1.0,
        "submitted, not applied"
    );
    assert!(scene.take_replies().is_empty(), "nothing answered yet");

    let before = scene.cube_seconds().expect("a cube");
    scene.tick(TICK);
    #[allow(clippy::cast_possible_truncation)]
    let step = TICK as f32 * 3.0;
    assert_eq!(
        scene.cube_seconds().map(f32::to_bits),
        Some((before + step).to_bits()),
        "the boundary is the tick's start: this tick already spun at 3"
    );
    let replies = scene.take_replies();
    assert_eq!(replies.len(), 1);
    assert_eq!(
        replies[0].to_string(),
        "sv_spin_rate = 3, applied at tick 2"
    );
}

#[test]
fn two_offline_sets_in_one_tick_apply_in_the_order_submitted() {
    let mut scene = scene();
    scene.submit_sim_set(spin_rate("4"));
    scene.submit_sim_set(spin_rate("0.5"));
    scene.tick(TICK);
    assert_eq!(scene.sim_vars().f32(&crate::spin::sv_spin_rate), 0.5);
    let lines: Vec<String> = scene
        .take_replies()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        lines,
        [
            "sv_spin_rate = 4, applied at tick 1",
            "sv_spin_rate = 0.5, applied at tick 1"
        ]
    );
}

#[test]
fn every_console_entry_this_sample_declares_is_in_its_table() {
    let declared =
        crcbl::console::guard::declared_names("src").expect("this sample's src directory");
    assert!(
        !declared.is_empty(),
        "the scan found nothing, so proves nothing"
    );
    let table = crate::spin::console_table();
    for name in &declared {
        assert!(
            table.vars().iter().any(|var| var.name() == name),
            "`{name}` is declared in this sample's source and missing from its table"
        );
    }
}
