//! Snapping a drag to the absolute grid, and the two steps that grid has.
//!
//! **Absolute**, not relative: a snapped value is the multiple of the step
//! nearest to where the drag would put it, wherever the drag began. A block
//! standing at `x = -2.9` snaps to `-3.0` or `-2.75`, never to `-2.65` — which
//! is what stepping from the press would give, and what leaves a block that
//! was ever off the grid off it for good.
//!
//! The steps are the player's settings, in the editor's own `settings.toml`
//! beside the panel layout ([`crate::layout::APP_NAME`]):
//!
//! ```toml
//! [editor.snap]
//! grid = 0.25   # metres a translate snaps the centre to
//! scale = 0.125 # metres a scale snaps each half extent to
//! ```
//!
//! A scale step of half the grid's, by default, so a snapped box is a whole
//! number of grid cells across.

use crcbl::store::settings::SettingsStack;

/// The settings key holding the translate step.
pub const GRID_KEY: &str = "editor.snap.grid";

/// The settings key holding the scale step.
pub const SCALE_KEY: &str = "editor.snap.scale";

/// The translate step with nothing set, in metres.
pub const GRID_M: f64 = 0.25;

/// The scale step with nothing set, in metres of half extent: half
/// [`GRID_M`], so a snapped box spans whole grid cells.
pub const SCALE_M: f64 = GRID_M / 2.0;

/// The absolute grid a snapping drag lands on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    /// What a centre snaps to a multiple of, in metres.
    grid: f64,
    /// What a half extent snaps to a multiple of, in metres.
    scale: f64,
}

impl Default for Snap {
    fn default() -> Self {
        Self {
            grid: GRID_M,
            scale: SCALE_M,
        }
    }
}

impl Snap {
    /// The steps `stack` sets, each falling back to its default where it sets
    /// none.
    ///
    /// A value that is not a positive, finite number — `0`, `-1`, `"fine"` —
    /// is logged and passed over rather than refused: a settings file is a
    /// thing people edit, and a step of zero would divide by it.
    #[must_use]
    pub fn load(stack: &SettingsStack) -> Self {
        let step = |key: &str, default: f64| {
            if !stack.contains(key) {
                return default;
            }
            match stack.get::<f64>(key) {
                Some(step) if step.is_finite() && step > 0.0 => step,
                _ => {
                    crcbl::log::warn!(
                        "editor: {key} is not a positive number of metres; snapping to \
                         {default} m"
                    );
                    default
                }
            }
        };
        Self {
            grid: step(GRID_KEY, GRID_M),
            scale: step(SCALE_KEY, SCALE_M),
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
}

/// The multiple of `step` nearest to `value`.
fn nearest_multiple(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}
