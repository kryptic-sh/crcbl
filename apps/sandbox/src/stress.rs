//! Fixed sustained-load scenes for measuring the forward renderer.
//!
//! These are named fixtures rather than count knobs. A benchmark whose object
//! or light count changes between runs is two workloads sharing a label, so the
//! command line selects a shape and this module owns every parameter behind it.

use crcbl::math::{Mat4, Quat, Vec3};
use crcbl::render::{
    ForwardRenderer, InstanceDesc, InstanceHandle, Light, PointLight, RectLight,
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

mod simulation;

pub use simulation::{Simulation, SimulationTimings};

/// The fixed scene the sandbox presents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    /// The sandbox's original spinning cube.
    #[default]
    Cube,
    /// A screen-filling grid of static instances, with one animated cell.
    Instances,
    /// The instance grid with every transform rewritten each frame.
    Moving,
    /// The instance grid with the froxel budget filled but not exceeded.
    LightsFit,
    /// The instance grid with the froxel budget filled by rectangular lights.
    AreaLightsFit,
    /// The instance grid under an overlapping field of local lights.
    Lights,
    /// The instance and within-budget light grid while dense broadphase bodies
    /// refit and query on the simulation clock.
    Combined,
}

impl Scene {
    /// Parses the spelling accepted by `sandbox --scene`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "cube" => Some(Self::Cube),
            "instances" => Some(Self::Instances),
            "moving" => Some(Self::Moving),
            "lights-fit" => Some(Self::LightsFit),
            "area-lights-fit" => Some(Self::AreaLightsFit),
            "lights" => Some(Self::Lights),
            "combined" => Some(Self::Combined),
            _ => None,
        }
    }

    /// The stable name printed beside a completed run.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cube => "cube",
            Self::Instances => "instances",
            Self::Moving => "moving",
            Self::LightsFit => "lights-fit",
            Self::AreaLightsFit => "area-lights-fit",
            Self::Lights => "lights",
            Self::Combined => "combined",
        }
    }

    /// Instances held live for the entire run.
    #[must_use]
    pub const fn instance_count(self) -> usize {
        match self {
            Self::Cube => 1,
            Self::Instances
            | Self::Moving
            | Self::LightsFit
            | Self::AreaLightsFit
            | Self::Lights
            | Self::Combined => INSTANCE_COUNT,
        }
    }

    /// Local lights held live for the entire run, not counting the sun.
    #[must_use]
    pub const fn light_count(self) -> usize {
        match self {
            Self::Cube | Self::Instances | Self::Moving => 0,
            Self::LightsFit | Self::AreaLightsFit | Self::Combined => WITHIN_BUDGET_LIGHT_COUNT,
            Self::Lights => LIGHT_COUNT,
        }
    }

    /// Whether the scene runs the dense broadphase workload on each tick.
    #[must_use]
    pub const fn simulates(self) -> bool {
        matches!(self, Self::Combined)
    }
}

/// Populates `renderer` and returns the instances the fixture may animate.
pub fn populate(renderer: &mut ForwardRenderer, scene: Scene) -> Vec<InstanceHandle> {
    let mut instances = Vec::with_capacity(scene.instance_count());
    for index in 0..scene.instance_count() {
        instances.push(
            renderer
                .add_instance(&instance(scene, index, 0.0))
                .expect("the fixed sandbox scene fits the demo instance pool"),
        );
    }

    let lights = lights(scene);
    renderer.set_lights(&lights);
    instances
}

/// Rewrites the transforms this fixture animates.
pub fn animate(
    renderer: &mut ForwardRenderer,
    scene: Scene,
    handles: &[InstanceHandle],
    seconds: f32,
) {
    let count = if scene == Scene::Moving {
        handles.len()
    } else {
        1
    };
    for (index, handle) in handles.iter().copied().take(count).enumerate() {
        renderer.set_instance(handle, &instance(scene, index, seconds));
    }
}

fn instance(scene: Scene, index: usize, seconds: f32) -> InstanceDesc {
    let transform = match scene {
        Scene::Cube => ForwardRenderer::spin(seconds),
        Scene::Instances
        | Scene::Moving
        | Scene::LightsFit
        | Scene::AreaLightsFit
        | Scene::Lights
        | Scene::Combined => {
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

    if scene == Scene::AreaLightsFit {
        (0..count).map(area_light).collect()
    } else {
        (0..count).map(point_light).collect()
    }
}

fn point_light(index: usize) -> Light {
    Light::Point(PointLight {
        position: light_position(index),
        radius: 4.0,
        color: Vec3::splat(0.004),
        fill: true,
    })
}

fn area_light(index: usize) -> Light {
    Light::Rect(RectLight {
        position: light_position(index),
        radius: 4.0,
        color: Vec3::splat(0.004),
        direction: -Vec3::Z,
        tangent: Vec3::X,
        half_width: 0.08,
        half_height: 0.04,
        fill: false,
    })
}

#[allow(clippy::cast_precision_loss)]
fn light_position(index: usize) -> Vec3 {
    let column = index % LIGHT_COLUMNS;
    let row = index / LIGHT_COLUMNS;
    let x = (column as f32 + 0.5) / LIGHT_COLUMNS as f32 * 2.4 - 1.2;
    let y = (row as f32 + 0.5) / LIGHT_ROWS as f32 * 1.35 - 0.675;
    Vec3::new(x, y, 0.35)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    use crcbl::hal::null::{Event, NullInstance, ObjectKind, Recorder};
    use crcbl::hal::{DeviceDesc, Format, Instance, QueueKind};
    use crcbl::render::{Camera, DirectionalLight};
    use crcbl::shaders::mesh::INSTANCE_STRIDE;

    #[test]
    fn every_scene_name_round_trips_and_names_a_fixed_load() {
        assert!(
            Scene::from_name("moving").is_some(),
            "the moving-instance workload must be selectable",
        );
        assert!(
            Scene::from_name("combined").is_some(),
            "the combined simulation and renderer workload must be selectable",
        );
        for scene in [
            Scene::Cube,
            Scene::Instances,
            Scene::Moving,
            Scene::LightsFit,
            Scene::AreaLightsFit,
            Scene::Lights,
            Scene::Combined,
        ] {
            assert_eq!(Scene::from_name(scene.name()), Some(scene));
            assert!(scene.instance_count() > 0);
        }
        assert_eq!(Scene::Instances.instance_count(), INSTANCE_COUNT);
        assert_eq!(Scene::Moving.instance_count(), INSTANCE_COUNT);
        assert_eq!(Scene::LightsFit.instance_count(), INSTANCE_COUNT);
        assert_eq!(Scene::AreaLightsFit.instance_count(), INSTANCE_COUNT);
        assert_eq!(Scene::Lights.instance_count(), INSTANCE_COUNT);
        assert_eq!(Scene::Combined.instance_count(), INSTANCE_COUNT);
        assert_eq!(
            Scene::LightsFit.light_count() + 1,
            usize::try_from(CLUSTER_LIGHT_CAPACITY).expect("the light budget fits usize"),
            "the local rows and the sun fill one froxel without overflowing it",
        );
        assert_eq!(Scene::Lights.light_count(), LIGHT_COUNT);
        assert_eq!(
            Scene::AreaLightsFit.light_count(),
            Scene::LightsFit.light_count(),
            "both within-budget controls must fill the same froxel prefix",
        );
        assert_eq!(
            Scene::Combined.light_count(),
            Scene::LightsFit.light_count()
        );
        assert_eq!(Scene::Instances.light_count(), 0);
        assert_eq!(Scene::Moving.light_count(), 0);
        assert!(Scene::Combined.simulates());
        assert!(!Scene::Lights.simulates());
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
    fn the_moving_fixture_rewrites_the_complete_instance_grid() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null device opens");
        let queue = device.queue(QueueKind::Graphics).expect("graphics queue");
        let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
            .expect("the demo renderer builds");
        let handles = populate(&mut renderer, Scene::Moving);
        let instance_buffers: HashSet<_> = recorder
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
        assert!(
            instance_buffers.len() > 1,
            "the observer must cover the complete instance-buffer ring",
        );

        for _ in 0..instance_buffers.len() {
            renderer
                .begin_frame(
                    device.as_ref(),
                    &Camera::default(),
                    &DirectionalLight::default(),
                    (64, 64),
                )
                .expect("the initial grid uploads");
        }
        let seen = recorder.events().len();
        animate(&mut renderer, Scene::Moving, &handles, 1.0);
        renderer
            .begin_frame(
                device.as_ref(),
                &Camera::default(),
                &DirectionalLight::default(),
                (64, 64),
            )
            .expect("the moving grid uploads");
        let written = recorder.events()[seen..]
            .iter()
            .filter_map(|event| match event {
                Event::BufferWritten { buffer, len, .. } if instance_buffers.contains(buffer) => {
                    Some(*len)
                }
                _ => None,
            })
            .sum::<usize>();
        assert_eq!(
            written,
            INSTANCE_COUNT * INSTANCE_STRIDE,
            "every moving instance must reach the current frame's device buffer",
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }

    #[test]
    fn the_light_fixture_builds_the_count_the_scene_reports() {
        let within_budget = lights(Scene::LightsFit);
        let area_lights = lights(Scene::AreaLightsFit);
        let local_lights = lights(Scene::Lights);
        assert_eq!(within_budget.len(), Scene::LightsFit.light_count());
        assert_eq!(area_lights.len(), Scene::AreaLightsFit.light_count());
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
        assert!(
            area_lights
                .iter()
                .all(|light| matches!(light, Light::Rect(rect) if !rect.fill))
        );
        assert!(lights(Scene::Cube).is_empty());
    }
}
