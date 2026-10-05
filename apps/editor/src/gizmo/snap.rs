//! Snapping a drag: to the absolute grid for a place or a size, and to an
//! angle step for a turn — and the three steps those are.
//!
//! **Absolute**, not relative, for a place and a size: a snapped value is the
//! multiple of the step nearest to where the drag would put it, wherever the
//! drag began. A block standing at `x = -2.9` snaps to `-3.0` or `-2.75`,
//! never to `-2.65` — which is what stepping from the press would give, and
//! what leaves a block that was ever off the grid off it for good.
//!
//! **Relative, for a turn**: the angle swept since the press snaps to a
//! multiple of the step. An orientation has no absolute grid about one axis
//! unless it is turned about that axis alone, and a turn measured from the
//! press is what every editor with angle snapping does; a block that starts
//! unturned, as most do, lands on the step's multiples all the same.
//!
//! The steps are the player's settings, in the editor's own `settings.toml`
//! beside the panel layout ([`crate::layout::APP_NAME`]), under
//! `[editor.snap]` as `grid`, `scale` and `angle`. Their defaults, and why
//! each is the number it is, are the editor's `defaults.toml`
//! ([`crate::defaults`]).

use crcbl::store::settings::SettingsStack;

/// The settings key holding the translate step.
pub const GRID_KEY: &str = "editor.snap.grid";

/// The settings key holding the scale step.
pub const SCALE_KEY: &str = "editor.snap.scale";

/// The settings key holding the turn step.
pub const ANGLE_KEY: &str = "editor.snap.angle";

/// The steps a snapping drag lands on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    /// What a centre snaps to a multiple of, in metres.
    grid: f64,
    /// What a half extent snaps to a multiple of, in metres.
    scale: f64,
    /// What a turn snaps to a multiple of, in degrees.
    angle: f64,
}

impl Default for Snap {
    /// The editor's default steps, from its `defaults.toml`.
    fn default() -> Self {
        Self::load(&crate::defaults::stack())
    }
}

impl Snap {
    /// The steps `stack` sets, each falling through to the editor's default
    /// where it sets none — on [`crate::defaults::positive`]'s terms, which
    /// also say what becomes of a step that is not a positive number.
    ///
    /// # Panics
    ///
    /// For a stack without the editor's defaults under it, as
    /// [`crate::defaults::positive`] does.
    #[must_use]
    pub fn load(stack: &SettingsStack) -> Self {
        let step = |key: &str, unit: &str| crate::defaults::positive(stack, key, unit);
        Self {
            grid: step(GRID_KEY, "metres"),
            scale: step(SCALE_KEY, "metres"),
            angle: step(ANGLE_KEY, "degrees"),
        }
    }

    /// The translate step, in metres.
    #[must_use]
    pub const fn grid_step(&self) -> f64 {
        self.grid
    }

    /// The scale step, in metres.
    #[must_use]
    pub const fn scale_step(&self) -> f64 {
        self.scale
    }

    /// The turn step, in degrees.
    #[must_use]
    pub const fn angle_step(&self) -> f64 {
        self.angle
    }

    /// `value` — a centre, in metres — on the nearest line of the grid.
    #[must_use]
    pub fn grid(&self, value: f64) -> f64 {
        nearest_multiple(value, self.grid)
    }

    /// `value` — a half extent, in metres — at the nearest multiple of the
    /// scale step.
    #[must_use]
    pub fn scale(&self, value: f64) -> f64 {
        nearest_multiple(value, self.scale)
    }

    /// `radians` — a turn — at the nearest multiple of the angle step, in
    /// radians.
    #[must_use]
    pub fn angle(&self, radians: f64) -> f64 {
        nearest_multiple(radians, self.angle.to_radians())
    }
}

/// The multiple of `step` nearest to `value`.
fn nearest_multiple(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}
