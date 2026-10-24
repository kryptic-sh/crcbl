//! Fixed sustained-load scenes for measuring the forward renderer.
//!
//! These are named fixtures rather than count knobs. A benchmark whose object
//! or light count changes between runs is two workloads sharing a label, so
//! `--stress` selects a shape and this module owns every parameter behind it.
//!
//! Every fixture is placed through [`ForwardRenderer::add_instance`] and
//! [`ForwardRenderer::set_lights`], the calls an application makes, and stays
//! live for the whole run so a tail percentile over its last frames measures
//! the same work as one over its first. The grid takes the cube's place; the
//! scene's sun still lights it and its spin still turns the animated cells,
//! so the clock that drives the cube drives the fixture too.
//!
//! One fixture, [`Stress::Combined`], also loads the CPU on the fixed tick: a
//! crowd of broadphase bodies, owned by [`Simulation`] rather than by the
//! renderer.

use crcbl::math::{Mat4, Quat, Vec3};
use crcbl::render::{
    ForwardRenderer, InstanceDesc, InstanceHandle, Light, PointLight, RectLight, RenderEffects,
    scene::{DEMO_CUBE, DEMO_UNTINTED},
};
use crcbl::shaders::light::CLUSTER_LIGHT_CAPACITY;

const INSTANCE_COLUMNS: usize = 100;
const INSTANCE_ROWS: usize = 100;
/// Instances every fixture holds live.
pub const INSTANCE_COUNT: usize = INSTANCE_COLUMNS * INSTANCE_ROWS;
const LIGHT_COLUMNS: usize = 16;
const LIGHT_ROWS: usize = 16;
/// Local lights the overflowing [`Stress::Lights`] fixture holds.
pub const LIGHT_COUNT: usize = LIGHT_COLUMNS * LIGHT_ROWS;
/// Local lights that, with the sun, exactly fill one froxel's list.
const WITHIN_BUDGET_LIGHT_COUNT: usize = CLUSTER_LIGHT_CAPACITY as usize - 1;

mod simulation;

pub use simulation::{Simulation, SimulationTimings};

/// A fixed renderer workload `--stress` can select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stress {
    /// A screen-filling grid of static instances, with one animated cell.
    Instances,
    /// The instance grid with every transform rewritten each frame.
    Moving,
    /// The instance grid with the froxel budget filled but not exceeded.
    LightsFit,
    /// The point-light grid with ordinary specular shading enabled.
    SpecularLightsFit,
    /// The instance grid with the froxel budget filled by rectangular lights.
    AreaLightsFit,
    /// The rectangular-light grid with specular disabled by the fill flag.
    AreaFillLightsFit,
    /// The instance grid under an overlapping field of local lights.
    Lights,
    /// The instance grid with auto-exposure metering every frame, which the
    /// sandbox's default camera layer leaves off.
    Exposure,
    /// The [`LightsFit`](Self::LightsFit) frame while a dense crowd of
    /// broadphase bodies moves, refits and queries on every fixed tick — see
    /// [`Simulation`].
    Combined,
}

impl Stress {
    /// Every fixture, in the order `--help` lists them.
    pub const ALL: [Self; 9] = [
        Self::Instances,
        Self::Moving,
        Self::LightsFit,
        Self::SpecularLightsFit,
        Self::AreaLightsFit,
        Self::AreaFillLightsFit,
        Self::Lights,
        Self::Exposure,
        Self::Combined,
    ];

    /// Parses the spelling accepted by `sandbox --stress`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stress| stress.name() == name)
    }

    /// The stable name `--stress` takes and the summary prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Instances => "instances",
            Self::Moving => "moving",
            Self::LightsFit => "lights-fit",
            Self::SpecularLightsFit => "specular-lights-fit",
            Self::AreaLightsFit => "area-lights-fit",
            Self::AreaFillLightsFit => "area-fill-lights-fit",
            Self::Lights => "lights",
            Self::Exposure => "exposure",
            Self::Combined => "combined",
        }
    }

    /// Every fixture's name, quoted and comma-separated, for a rejection.
    #[must_use]
    pub fn name_list() -> String {
        Self::ALL
            .iter()
            .map(|stress| format!("`{}`", stress.name()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Local lights held live for the entire run, not counting the sun.
    #[must_use]
    pub const fn light_count(self) -> usize {
        match self {
            Self::Instances | Self::Moving | Self::Exposure => 0,
            Self::LightsFit
            | Self::SpecularLightsFit
            | Self::AreaLightsFit
            | Self::AreaFillLightsFit
            | Self::Combined => WITHIN_BUDGET_LIGHT_COUNT,
            Self::Lights => LIGHT_COUNT,
        }
    }

    /// Whether the fixture runs a [`Simulation`] on the fixed tick.
    #[must_use]
    pub const fn simulates(self) -> bool {
        matches!(self, Self::Combined)
    }
}

/// A fixture placed in a renderer: which one, and the instances it holds.
#[derive(Debug)]
pub struct Fixture {
    stress: Stress,
    instances: Vec<InstanceHandle>,
}

impl Fixture {
    /// Places `stress`'s instances and lights in `renderer`.
    ///
    /// # Panics
    ///
    /// If the renderer's instance pool or light list is too small for the
    /// fixture. The sandbox's renderer is built from the demo scene, whose
    /// capacities hold every fixture.
    pub fn populate(renderer: &mut ForwardRenderer, stress: Stress) -> Self {
        let instances = (0..INSTANCE_COUNT)
            .map(|index| {
                renderer
                    .add_instance(&instance(index, 0.0))
                    .expect("the demo instance pool holds the stress grid")
            })
            .collect();
        renderer.set_lights(&lights(stress));
        if stress == Stress::Exposure {
            // The camera layer, because auto-exposure is a lens effect a view
            // asks for; the player's clamp and the rest stay as opened.
            let mut request = renderer.effect_request();
            request.camera = request.camera.union(RenderEffects::AUTO_EXPOSURE);
            renderer.set_effect_request(request);
        }
        Self { stress, instances }
    }

    /// Rewrites the transforms this fixture animates, at `seconds` of spin.
    ///
    /// One cell for every fixture but [`Stress::Moving`], which rewrites the
    /// whole grid so the update and upload cost stays live.
    pub fn animate(&self, renderer: &mut ForwardRenderer, seconds: f32) {
        let count = if self.stress == Stress::Moving {
            self.instances.len()
        } else {
            1
        };
        for (index, handle) in self.instances.iter().copied().take(count).enumerate() {
            renderer.set_instance(handle, &instance(index, seconds));
        }
    }
}

fn instance(index: usize, seconds: f32) -> InstanceDesc {
    InstanceDesc {
        mesh: DEMO_CUBE,
        material: DEMO_UNTINTED,
        transform: grid_transform(index) * Mat4::from_quat(Quat::from_rotation_y(seconds * 0.9)),
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

fn lights(stress: Stress) -> Vec<Light> {
    let count = stress.light_count();
    match stress {
        Stress::AreaLightsFit | Stress::AreaFillLightsFit => {
            let fill = stress == Stress::AreaFillLightsFit;
            (0..count).map(|index| area_light(index, fill)).collect()
        }
        _ => {
            let fill = stress != Stress::SpecularLightsFit;
            (0..count).map(|index| point_light(index, fill)).collect()
        }
    }
}

fn point_light(index: usize, fill: bool) -> Light {
    Light::Point(PointLight {
        position: light_position(index),
        radius: 4.0,
        color: Vec3::splat(0.004),
        fill,
    })
}

fn area_light(index: usize, fill: bool) -> Light {
    Light::Rect(RectLight {
        position: light_position(index),
        radius: 4.0,
        color: Vec3::splat(0.004),
        direction: -Vec3::Z,
        tangent: Vec3::X,
        half_width: 0.08,
        half_height: 0.04,
        fill,
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
    fn every_fixture_name_round_trips_and_unknown_names_are_refused() {
        for stress in Stress::ALL {
            assert_eq!(Stress::from_name(stress.name()), Some(stress));
            assert!(
                Stress::name_list().contains(&format!("`{}`", stress.name())),
                "the rejection must offer `{}`",
                stress.name(),
            );
        }
        let names: HashSet<_> = Stress::ALL.iter().map(|stress| stress.name()).collect();
        assert_eq!(names.len(), Stress::ALL.len(), "two fixtures share a name");
        assert_eq!(Stress::from_name("cube"), None);
    }

    #[test]
    fn the_light_counts_hold_the_froxel_budget_where_they_say_they_do() {
        assert_eq!(
            Stress::LightsFit.light_count() + 1,
            usize::try_from(CLUSTER_LIGHT_CAPACITY).expect("the light budget fits usize"),
            "the local rows and the sun fill one froxel without overflowing it",
        );
        for matched in [
            Stress::SpecularLightsFit,
            Stress::AreaLightsFit,
            Stress::AreaFillLightsFit,
        ] {
            assert_eq!(
                matched.light_count(),
                Stress::LightsFit.light_count(),
                "{} must fill the same froxel prefix as lights-fit",
                matched.name(),
            );
        }
        assert!(Stress::Lights.light_count() > Stress::LightsFit.light_count());
        assert_eq!(Stress::Instances.light_count(), 0);
        assert_eq!(Stress::Moving.light_count(), 0);
    }

    #[test]
    fn only_the_combined_fixture_simulates_and_it_draws_the_lights_fit_frame() {
        for stress in Stress::ALL {
            assert_eq!(
                stress.simulates(),
                stress == Stress::Combined,
                "{}",
                stress.name()
            );
            assert_eq!(
                Simulation::for_stress(stress, false)
                    .expect("the query pool starts")
                    .is_some(),
                stress.simulates(),
                "{}",
                stress.name()
            );
        }
        assert_eq!(
            lights(Stress::Combined),
            lights(Stress::LightsFit),
            "the combined frame must be the lights-fit frame, or its CPU cost \
             is not the only difference",
        );
    }

    #[test]
    fn the_instance_fixture_fills_distinct_cells_and_animates_only_in_place() {
        let first = instance(0, 0.0).transform;
        let second = instance(1, 0.0).transform;
        let animated = instance(0, 1.0).transform;
        assert_ne!(first, second, "neighbouring cells must not overlap exactly");
        assert_ne!(first, animated, "the animated cell must actually move");
        assert_eq!(
            first.transform_point3(Vec3::ZERO),
            animated.transform_point3(Vec3::ZERO),
            "animation must not move the cell through the grid",
        );
    }

    /// Bytes written into the instance ring by one frame after `animate`.
    fn instance_bytes_uploaded_after_animating(stress: Stress) -> usize {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null device opens");
        let queue = device.queue(QueueKind::Graphics).expect("graphics queue");
        let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
            .expect("the demo renderer builds");
        let fixture = Fixture::populate(&mut renderer, stress);
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

        // One frame per ring slot, so every slot holds the initial grid and
        // the frame below uploads only what `animate` changed.
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
        fixture.animate(&mut renderer, 1.0);
        renderer
            .begin_frame(
                device.as_ref(),
                &Camera::default(),
                &DirectionalLight::default(),
                (64, 64),
            )
            .expect("the animated grid uploads");
        let written = recorder.events()[seen..]
            .iter()
            .filter_map(|event| match event {
                Event::BufferWritten { buffer, len, .. } if instance_buffers.contains(buffer) => {
                    Some(*len)
                }
                _ => None,
            })
            .sum::<usize>();

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
        written
    }

    #[test]
    fn the_moving_fixture_uploads_the_complete_grid_and_the_others_one_cell() {
        assert_eq!(
            instance_bytes_uploaded_after_animating(Stress::Moving),
            INSTANCE_COUNT * INSTANCE_STRIDE,
            "every moving instance must reach the current frame's device buffer",
        );
        assert_eq!(
            instance_bytes_uploaded_after_animating(Stress::Instances),
            INSTANCE_STRIDE,
            "the static grid must upload only its animated cell",
        );
    }

    #[test]
    fn the_light_fixture_builds_the_count_and_kind_the_fixture_reports() {
        for stress in Stress::ALL {
            assert_eq!(lights(stress).len(), stress.light_count(), "{stress:?}");
        }
        let within_budget = lights(Stress::LightsFit);
        let specular_lights = lights(Stress::SpecularLightsFit);
        let area_lights = lights(Stress::AreaLightsFit);
        let area_fill_lights = lights(Stress::AreaFillLightsFit);
        let local_lights = lights(Stress::Lights);
        assert_eq!(
            within_budget,
            local_lights[..within_budget.len()],
            "the budget control must shade the same prefix as the overflow scene",
        );
        assert!(
            local_lights
                .iter()
                .all(|light| matches!(light, Light::Point(point) if point.fill))
        );
        for (fill, ordinary) in within_budget.iter().zip(&specular_lights) {
            let (Light::Point(fill), Light::Point(ordinary)) = (fill, ordinary) else {
                panic!("both point controls must contain only point lights");
            };
            assert!(!ordinary.fill, "the matched control must shade specular");
            assert_eq!(
                ordinary,
                &PointLight {
                    fill: false,
                    ..*fill
                }
            );
        }
        assert!(
            area_lights
                .iter()
                .all(|light| matches!(light, Light::Rect(rect) if !rect.fill))
        );
        for (ordinary, fill) in area_lights.iter().zip(&area_fill_lights) {
            let (Light::Rect(ordinary), Light::Rect(fill)) = (ordinary, fill) else {
                panic!("both area controls must contain only rectangles");
            };
            assert!(fill.fill, "the matched control must skip specular");
            assert_eq!(
                ordinary,
                &RectLight {
                    fill: false,
                    ..*fill
                }
            );
        }
    }
}
