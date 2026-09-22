//! Fixed sustained-load scenes for measuring the forward renderer.
//!
//! These are named fixtures rather than count knobs. A benchmark whose object
//! or light count changes between runs is two workloads sharing a label, so the
//! command line selects a shape and this module owns every parameter behind it.

use crcbl::math::{Mat4, Quat, Vec3};
use crcbl::render::{
    ForwardRenderer, InstanceDesc, InstanceHandle, Light, PointLight,
    scene::{DEMO_CUBE, DEMO_UNTINTED},
};
use crcbl::shaders::light::CLUSTER_LIGHT_CAPACITY;

const INSTANCE_COLUMNS: usize = 100;
const INSTANCE_ROWS: usize = 100;
pub const INSTANCE_COUNT: usize = INSTANCE_COLUMNS * INSTANCE_ROWS;
const LIGHT_COLUMNS: usize = 16;
const LIGHT_ROWS: usize = 16;
pub const LIGHT_COUNT: usize = LIGHT_COLUMNS * LIGHT_ROWS;
const WITHIN_BUDGET_LIGHT_COUNT: usize = CLUSTER_LIGHT_CAPACITY as usize - 1;

/// The fixed scene the sandbox presents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    /// The sandbox's original spinning cube.
    #[default]
    Cube,
    /// A screen-filling grid of static instances, with one animated cell.
    Instances,
    /// The instance grid with the froxel budget filled but not exceeded.
    LightsFit,
    /// The instance grid under an overlapping field of local lights.
    Lights,
}

impl Scene {
    /// Parses the spelling accepted by `sandbox --scene`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "cube" => Some(Self::Cube),
            "instances" => Some(Self::Instances),
            "lights-fit" => Some(Self::LightsFit),
            "lights" => Some(Self::Lights),
            _ => None,
        }
    }

    /// The stable name printed beside a completed run.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cube => "cube",
            Self::Instances => "instances",
            Self::LightsFit => "lights-fit",
            Self::Lights => "lights",
        }
    }

    /// Instances held live for the entire run.
    #[must_use]
    pub const fn instance_count(self) -> usize {
        match self {
            Self::Cube => 1,
            Self::Instances | Self::LightsFit | Self::Lights => INSTANCE_COUNT,
        }
    }

    /// Local lights held live for the entire run, not counting the sun.
    #[must_use]
    pub const fn light_count(self) -> usize {
        match self {
            Self::Cube | Self::Instances => 0,
            Self::LightsFit => WITHIN_BUDGET_LIGHT_COUNT,
            Self::Lights => LIGHT_COUNT,
        }
    }
}

/// Populates `renderer` and returns the one instance animated each frame.
pub fn populate(renderer: &mut ForwardRenderer, scene: Scene) -> InstanceHandle {
    let animated = renderer
        .add_instance(&instance(scene, 0, 0.0))
        .expect("the fixed sandbox scene fits the demo instance pool");

    for index in 1..scene.instance_count() {
        renderer
            .add_instance(&instance(scene, index, 0.0))
            .expect("the fixed sandbox scene fits the demo instance pool");
    }

    let lights = lights(scene);
    renderer.set_lights(&lights);
    animated
}

/// The animated instance's next description.
#[must_use]
pub fn animated(scene: Scene, seconds: f32) -> InstanceDesc {
    instance(scene, 0, seconds)
}

fn instance(scene: Scene, index: usize, seconds: f32) -> InstanceDesc {
    let transform = match scene {
        Scene::Cube => ForwardRenderer::spin(seconds),
        Scene::Instances | Scene::LightsFit | Scene::Lights => {
            grid_transform(index) * Mat4::from_quat(Quat::from_rotation_y(seconds * 0.9))
        }
    };
    InstanceDesc {
        mesh: DEMO_CUBE,
        material: DEMO_UNTINTED,
        transform,
    }
}

#[allow(clippy::cast_precision_loss)]
fn grid_transform(index: usize) -> Mat4 {
    let column = index % INSTANCE_COLUMNS;
    let row = index / INSTANCE_COLUMNS;
    let x = (column as f32 + 0.5) / INSTANCE_COLUMNS as f32 * 2.4 - 1.2;
    let y = (row as f32 + 0.5) / INSTANCE_ROWS as f32 * 1.35 - 0.675;
    let depth = (index % 7) as f32 * -0.015;
    Mat4::from_scale_rotation_translation(
        Vec3::splat(0.009),
        Quat::IDENTITY,
        Vec3::new(x, y, depth),
    )
}

fn lights(scene: Scene) -> Vec<Light> {
    let count = scene.light_count();
    if count == 0 {
        return Vec::new();
    }

    (0..count).map(light).collect()
}

#[allow(clippy::cast_precision_loss)]
fn light(index: usize) -> Light {
    let column = index % LIGHT_COLUMNS;
    let row = index / LIGHT_COLUMNS;
    let x = (column as f32 + 0.5) / LIGHT_COLUMNS as f32 * 2.4 - 1.2;
    let y = (row as f32 + 0.5) / LIGHT_ROWS as f32 * 1.35 - 0.675;
    Light::Point(PointLight {
        position: Vec3::new(x, y, 0.35),
        radius: 4.0,
        color: Vec3::splat(0.004),
        fill: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_name_round_trips_and_names_a_fixed_load() {
        for scene in [
            Scene::Cube,
            Scene::Instances,
            Scene::LightsFit,
            Scene::Lights,
        ] {
            assert_eq!(Scene::from_name(scene.name()), Some(scene));
            assert!(scene.instance_count() > 0);
        }
        assert_eq!(Scene::Instances.instance_count(), INSTANCE_COUNT);
        assert_eq!(Scene::LightsFit.instance_count(), INSTANCE_COUNT);
        assert_eq!(Scene::Lights.instance_count(), INSTANCE_COUNT);
        assert_eq!(
            Scene::LightsFit.light_count() + 1,
            usize::try_from(CLUSTER_LIGHT_CAPACITY).expect("the light budget fits usize"),
            "the local rows and the sun fill one froxel without overflowing it",
        );
        assert_eq!(Scene::Lights.light_count(), LIGHT_COUNT);
        assert_eq!(Scene::Instances.light_count(), 0);
    }

    #[test]
    fn the_instance_fixture_fills_distinct_cells_and_animates_only_in_place() {
        let first = instance(Scene::Instances, 0, 0.0).transform;
        let second = instance(Scene::Instances, 1, 0.0).transform;
        let animated = instance(Scene::Instances, 0, 1.0).transform;
        assert_ne!(first, second, "neighbouring cells must not overlap exactly");
        assert_ne!(first, animated, "the animated cell must actually move");
        assert_eq!(
            first.transform_point3(Vec3::ZERO),
            animated.transform_point3(Vec3::ZERO),
            "animation must not move the cell through the grid",
        );
    }

    #[test]
    fn the_light_fixture_builds_the_count_the_scene_reports() {
        let within_budget = lights(Scene::LightsFit);
        let local_lights = lights(Scene::Lights);
        assert_eq!(within_budget.len(), Scene::LightsFit.light_count());
        assert_eq!(local_lights.len(), Scene::Lights.light_count());
        assert_eq!(
            within_budget,
            local_lights[..within_budget.len()],
            "the budget control must shade the same prefix as the overflow scene",
        );
        assert!(
            local_lights
                .iter()
                .all(|light| matches!(light, Light::Point(_)))
        );
        assert!(lights(Scene::Cube).is_empty());
    }
}
