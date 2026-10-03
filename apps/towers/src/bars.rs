//! World-space health bars: one over every creep on the field, filled to the
//! health it has left and tinted while a slow tower holds it.
//!
//! ```text
//!        ┌──────────┐   ← drawn LIFT_M above the creep's top
//!        │█████░░░░░│      fill = health left, slow tint when held
//!        └──────────┘
//!            (●)        ← the creep
//! ```
//!
//! # Through the UI, placed by the camera
//!
//! The engine's world-space UI path is [`Camera::pixel_of`]: the point over
//! the creep is projected through the very camera the frame is drawn with,
//! and the bar is two rectangles in the frame's draw list at that pixel. The
//! other way — two more instance pools in the forward renderer, a flat slab
//! per creep scaled to its health — was weighed and left: a slab lying on the
//! field is foreshortened by the overhead view's tilt and edge-on to a walking
//! eye, where a UI rectangle is the same size and square to the screen from
//! anywhere, which is what a bar is for. It costs no instances and no
//! materials, so `crate::map`'s reservation is unchanged.
//!
//! # From the field the frame draws, so a joiner's bars are the host's
//!
//! The bars read [`RenderState`], which is the stage's own on a solo run or a
//! host and the host's snapshots on a joiner — [`CreepView::health`] and
//! [`CreepView::slowed`] cross the wire for exactly this (`crate::replica`).
//!
//! # Not in the walk
//!
//! A UI rectangle has no depth: it is drawn over everything, so a creep behind
//! a tower or a kerb would show its bar through them, and at a walking eye's
//! height every creep near the camera would wear a bar the width of the
//! window's corner. The walk is a dev camera for trying the controller on the
//! field, not a way to play, so `crate::app` draws no bars in it. The overhead
//! view and the fly camera — both looking down on the field — draw them.

use crcbl::math::{DVec3, Vec2, Vec3};
use crcbl::render::Camera;
use crcbl::ui::draw_list::DrawList;

use crate::creep::CreepView;
use crate::game::RenderState;
use crate::map::CREEP_RADIUS;

/// How far above a creep's top its bar is drawn, in metres.
pub const LIFT_M: f64 = 0.35;

/// How wide a bar is, in pixels, and how tall.
pub const WIDTH_PX: f32 = 28.0;
pub const HEIGHT_PX: f32 = 4.0;

/// What the empty part of a bar is drawn in: dark, so a nearly dead creep's
/// sliver of fill reads against it.
pub const TRACK: [f32; 4] = [0.06, 0.07, 0.06, 0.85];
/// What a free creep's health is drawn in.
pub const FILL: [f32; 4] = [0.45, 0.88, 0.40, 1.0];
/// …and a held creep's: the slow tower's cold blue, the hue its hold tints
/// the creep itself (`crate::map::CREEP_SLOWED_MATERIAL`).
pub const SLOWED_FILL: [f32; 4] = [0.40, 0.72, 0.88, 1.0];

/// The point in the world a creep at `centre` has its bar centred on.
#[must_use]
pub fn anchor(centre: DVec3) -> Vec3 {
    (centre + DVec3::Y * (CREEP_RADIUS + LIFT_M)).as_vec3()
}

/// One creep's bar, in the frame's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// The pixel [`anchor`] lands on: the bar's centre.
    pub at: Vec2,
    /// How much of it is filled, from zero to one — the creep's health.
    pub fill: f32,
    /// Whether a slow tower holds the creep, which tints the fill.
    pub slowed: bool,
}

impl Bar {
    /// The whole bar's corners, top-left then bottom-right.
    #[must_use]
    pub fn track(&self) -> (Vec2, Vec2) {
        let half = Vec2::new(WIDTH_PX, HEIGHT_PX) * 0.5;
        (self.at - half, self.at + half)
    }

    /// The filled part's corners: the track from its left edge, as far across
    /// as the creep has health.
    #[must_use]
    pub fn filled(&self) -> (Vec2, Vec2) {
        let (min, max) = self.track();
        (
            min,
            Vec2::new(min.x + WIDTH_PX * self.fill.clamp(0.0, 1.0), max.y),
        )
    }

    /// What the fill is drawn in.
    #[must_use]
    pub const fn colour(&self) -> [f32; 4] {
        if self.slowed { SLOWED_FILL } else { FILL }
    }
}

/// The bar of every live creep in `state` that `camera` sees, in the field's
/// order, for a frame `extent` pixels across. A creep on or behind the eye
/// has no pixel and no bar.
#[must_use]
pub fn bars(state: &RenderState, camera: &Camera, extent: (u32, u32)) -> Vec<Bar> {
    if extent.0 == 0 || extent.1 == 0 {
        return Vec::new();
    }
    state.creeps[..state.creeps_alive]
        .iter()
        .filter_map(|creep: &CreepView| {
            Some(Bar {
                at: camera.pixel_of(anchor(creep.centre), extent)?,
                fill: creep.health,
                slowed: creep.slowed,
            })
        })
        .collect()
}

/// Draws `bars` into `list`: the track, then the fill over it.
pub fn draw(list: &mut DrawList, bars: &[Bar]) {
    for bar in bars {
        let (min, max) = bar.track();
        list.rect(min, max, TRACK);
        let (min, max) = bar.filled();
        if max.x > min.x {
            list.rect(min, max, bar.colour());
        }
    }
}

#[cfg(test)]
mod tests;
