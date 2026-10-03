//! The sandbox's scene — the cube it spins and the light over it, as entities
//! in a [`World`] — and the debug panel's entity selection over that world.
//!
//! # The world is what the frame draws
//!
//! The cube's angle is [`Spin::seconds`], advanced here on the fixed timestep
//! and handed to the GPU each tick; the light is [`Sun`], copied into the
//! renderer's [`DirectionalLight`] the same way. Neither is a mirror of state
//! kept elsewhere, so a value the panel shows is the value on screen. The
//! accumulation is the one `Gpu` used to do itself — `seconds += dt as f32`
//! from zero — so the picture a `--headless --frames N` run renders is
//! bit-identical to before the world owned it.
//!
//! # Selecting an entity
//!
//! [`SELECT_NEXT_KEY`] and [`SELECT_PREVIOUS_KEY`] step through
//! [`World::entities`], wrapping. The selection is held across frames and
//! dropped by the first tick after its entity is swept. While one is selected,
//! the panel's "scene" section names it and every system that holds it adds a
//! section of its own: whatever its
//! [`SystemTrait::debug_fields`](crcbl::ecs::SystemTrait::debug_fields) lends,
//! written by [`ReflectedSection`]. The scene knows no system's fields — a
//! system that lends nothing simply has no section.
//!
//! The keys work whether or not the panel is showing: a hosted game is not told
//! the panel's state, and a selection made blind is shown the moment F3 opens
//! it.

use crcbl::core::input::KeyCode;
use crcbl::ecs::{ComponentHash, Entity, System, World};
use crcbl::reflect::Reflect;
use crcbl::render::DirectionalLight;
use crcbl::ui::{DebugModule, DebugPanel, DebugSection, ReflectedSection};

/// Selects the entity after the selected one, or the first.
pub const SELECT_NEXT_KEY: KeyCode = KeyCode::PageDown;

/// Selects the entity before the selected one, or the last.
pub const SELECT_PREVIOUS_KEY: KeyCode = KeyCode::PageUp;

/// The system holding the cube's [`Spin`].
pub const SPIN: &str = "spin";

/// The system holding the [`Sun`].
pub const SUN: &str = "sun";

/// The scene section's title.
pub const SCENE_SECTION: &str = "scene";

/// How far the cube has spun, as the seconds of animation
/// `ForwardRenderer::spin` turns into its rotation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
pub struct Spin {
    /// Seconds of animation, advanced by the fixed timestep.
    pub seconds: f32,
}

impl ComponentHash for Spin {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        self.seconds.hash_component(hasher);
    }
}

/// The scene's one directional light, field for field the renderer's
/// [`DirectionalLight`] — as arrays, which reflect and hash where `Vec3` does
/// not hash.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
pub struct Sun {
    /// Towards the light, in render space.
    pub direction: [f32; 3],
    /// Colour premultiplied by intensity.
    pub color: [f32; 3],
    /// The flat ambient term.
    pub ambient: [f32; 3],
}

impl From<DirectionalLight> for Sun {
    fn from(light: DirectionalLight) -> Self {
        Self {
            direction: light.direction.to_array(),
            color: light.color.to_array(),
            ambient: light.ambient.to_array(),
        }
    }
}

impl From<Sun> for DirectionalLight {
    fn from(sun: Sun) -> Self {
        Self {
            direction: sun.direction.into(),
            color: sun.color.into(),
            ambient: sun.ambient.into(),
        }
    }
}

impl ComponentHash for Sun {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        self.direction.hash_component(hasher);
        self.color.hash_component(hasher);
        self.ambient.hash_component(hasher);
    }
}

/// The sandbox's world, the two entities in it, and which one the panel has
/// selected.
#[derive(Debug)]
pub struct Scene {
    world: World,
    cube: Entity,
    sun: Entity,
    selected: Option<Entity>,
}

impl Scene {
    /// A cube that has not spun yet and a sun shining as `light` does.
    #[must_use]
    pub fn new(light: DirectionalLight) -> Self {
        let mut world = World::new();
        let cube = world.spawn();
        let sun = world.spawn();
        let mut spins = System::<Spin>::reflected(SPIN);
        spins.attach(cube, Spin::default());
        let mut suns = System::<Sun>::reflected(SUN);
        suns.attach(sun, Sun::from(light));
        world.register_system(Box::new(spins));
        world.register_system(Box::new(suns));
        Self {
            world,
            cube,
            sun,
            selected: None,
        }
    }

    /// One fixed step of `dt` seconds: every spin advances, the world sweeps,
    /// and a selection whose entity was swept is dropped.
    ///
    /// `World::system_mut` scans the schedule, which its docs keep out of a
    /// tick in general; this schedule is two systems long.
    pub fn tick(&mut self, dt: f64) {
        // Narrowed before the add, as `Gpu` did: the same f32 sum from zero
        // is what keeps the headless picture bit-identical.
        #[allow(clippy::cast_possible_truncation)]
        let step = dt as f32;
        if let Some(spins) = self.world.system_mut::<System<Spin>>() {
            for spin in spins.iter_mut() {
                spin.seconds += step;
            }
        }
        self.world.tick_with_dt(dt);
        self.forget_swept();
    }

    /// The cube's seconds of animation, or `None` once it is gone.
    pub fn cube_seconds(&mut self) -> Option<f32> {
        let cube = self.cube;
        let spins = self.world.system_mut::<System<Spin>>()?;
        spins.get(cube).map(|spin| spin.seconds)
    }

    /// The light the sun casts, or `None` once it is gone.
    pub fn light(&mut self) -> Option<DirectionalLight> {
        let sun = self.sun;
        let suns = self.world.system_mut::<System<Sun>>()?;
        suns.get(sun).copied().map(DirectionalLight::from)
    }

    /// The selected entity, if any.
    #[cfg(test)]
    pub const fn selected(&self) -> Option<Entity> {
        self.selected
    }

    /// Steps the selection on a press of [`SELECT_NEXT_KEY`] or
    /// [`SELECT_PREVIOUS_KEY`]; `true` when `key` was one of them, pressed or
    /// released, so nothing else acts on it.
    pub fn key(&mut self, key: KeyCode, pressed: bool) -> bool {
        let forward = match key {
            SELECT_NEXT_KEY => true,
            SELECT_PREVIOUS_KEY => false,
            _ => return false,
        };
        if pressed {
            self.step(forward);
        }
        true
    }

    /// Moves the selection one entity on, or back, wrapping; from nothing,
    /// onto the first or the last.
    fn step(&mut self, forward: bool) {
        let entities: Vec<Entity> = self.world.entities().collect();
        let Some(last) = entities.len().checked_sub(1) else {
            self.selected = None;
            return;
        };
        let at = self
            .selected
            .and_then(|selected| entities.iter().position(|&entity| entity == selected));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(at), true) if at == last => 0,
            (Some(at), true) => at + 1,
            (Some(0), false) => last,
            (Some(at), false) => at - 1,
        };
        self.selected = Some(entities[next]);
    }

    /// Drops the selection if its entity has been swept.
    fn forget_swept(&mut self) {
        if self
            .selected
            .is_some_and(|entity| !self.world.is_alive(entity))
        {
            self.selected = None;
        }
    }

    /// The "scene" section, then one section per system that lends the
    /// selected entity's data.
    pub fn debug_sections(&self, panel: &mut DebugPanel) {
        panel.add(self);
        let Some(entity) = self.selected else {
            return;
        };
        for system in self.world.schedule().iter() {
            if let Some(value) = system.debug_fields(entity) {
                panel.add(&ReflectedSection {
                    title: system.name(),
                    value,
                });
            }
        }
    }

    /// The world, for a test to despawn from.
    #[cfg(test)]
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The cube's entity.
    #[cfg(test)]
    pub const fn cube(&self) -> Entity {
        self.cube
    }

    /// The sun's entity.
    #[cfg(test)]
    pub const fn sun(&self) -> Entity {
        self.sun
    }
}

/// The scene section: how many entities there are and which is selected.
impl DebugModule for Scene {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title(SCENE_SECTION);
        out.row("entities", format_args!("{}", self.world.entity_count()));
        match self.selected {
            Some(entity) => out.row(
                "selected",
                format_args!("{}v{}", entity.index(), entity.generation()),
            ),
            None => out.row_str("selected", "none"),
        }
        out.row_str("select", "PgUp / PgDn");
    }
}

#[cfg(test)]
mod tests;
