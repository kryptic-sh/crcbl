//! The stock effect library, and the colour palettes the instance path needs.
//!
//! Three effects, which is what `docs/plan/sample/10-sparks.md`'s first
//! milestone asks of this sample: two from its stock list — impact sparks and a
//! smoke puff — and the "deliberately hostile effect (max spam)" its budget
//! claim is about. Every one is a value in this file rather than a `.ron` on
//! disk, because the asset format is a later slice; `docs/backlog.md` says so.
//!
//! # Colour over lifetime arrives as a material row, and that is a finding
//!
//! `docs/plan/20-particles.md` draws mesh particles by injecting transforms
//! into the stage 3 instance path, and that path has **no per-instance tint**:
//! a `crcbl::render::scene::InstanceDesc` carries a mesh, a material row and a
//! transform, and the shader's albedo is the per-*vertex* colour times the
//! per-*material* one. So a gradient reaches the screen quantised —
//! [`PALETTE_STEPS`] material rows baked from it, and each particle drawn
//! through the row nearest the colour the simulation gave it.
//!
//! That is a real limit rather than a shortcut, and it is the right one for
//! this slice: adding a colour word to the instance record is a change to the
//! shader contract and to every golden frame, and
//! `docs/plan/sample/10-sparks.md`'s hard cap is explicit that the gallery does
//! not get to smuggle engine features in. `docs/backlog.md` carries it.
//!
//! # Alpha is not how anything fades here
//!
//! The forward pass draws these instances opaque, so a gradient's alpha channel
//! changes nothing on screen. Every effect below fades by **shrinking** —
//! [`Modifiers::size`] reaching zero — which is what the instance path can
//! actually show. The alpha in the gradients is still authored, because it is
//! what a billboard pass will read the day there is one, and because a gradient
//! whose alpha said one thing while the picture did another would be worse than
//! either.

use crcbl::math::{Vec3, Vec4};
use crcbl::shaders::mesh::GpuMaterial;
use crcbl::vfx::{Curve, EffectDesc, Gradient, Modifiers, Shape, Spawn};

/// The whole pool, in particles.
///
/// Every effect's share comes out of this, and the shares below add up to well
/// under it — the headroom is what a second burst of sparks lands in while the
/// first is still alight.
pub const POOL: u32 = 2048;

/// The impact sparks' share of the pool.
pub const SPARK_SHARE: u32 = 256;

/// The smoke puff's share.
pub const PUFF_SHARE: u32 = 384;

/// **The hostile effect's share, and the number its whole point is that it
/// cannot exceed.**
///
/// Small on purpose, and far below what [`spam`]'s emitter asks for every step:
/// a budget that is never reached is not a budget being tested.
pub const SPAM_SHARE: u32 = 64;

/// How many material rows a gradient is baked into.
///
/// Eight, which is enough that a spark's white-to-orange run reads as a run
/// rather than as two colours, and few enough that the three effects' palettes
/// and the stage's own rows fit a material table this sample can size by hand.
pub const PALETTE_STEPS: usize = 8;

/// Impact sparks: a cone burst that arcs and burns out.
///
/// The parameters are `docs/plan/20-particles.md`'s own worked example — a
/// burst of 64 into a 30° cone, a lifetime of 0.2 to 0.6 seconds, 4 to 9 metres
/// a second, gravity and drag, and a size curve falling to a fifth. What
/// differs is that the curve falls all the way to zero rather than to 0.2,
/// because shrinking is this path's fade.
#[must_use]
pub fn impact_sparks() -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Burst { count: 96 },
        shape: Shape::Cone {
            axis: Vec3::Y,
            half_angle: std::f32::consts::FRAC_PI_6,
        },
        lifetime: (0.25, 0.7),
        speed: (4.0, 9.0),
        size: (0.03, 0.07),
        // Symmetric, so debris tumbles both ways.
        spin: (-14.0, 14.0),
        modifiers: Modifiers {
            gravity: Vec3::new(0.0, -9.8, 0.0),
            drag: 1.2,
            size: Curve::new(vec![(0.0, 1.0), (0.7, 0.7), (1.0, 0.0)])
                .expect("the size curve's keys ascend"),
            color: Gradient::new(vec![
                (0.0, Vec4::new(1.6, 1.5, 1.1, 1.0)),
                (0.45, Vec4::new(1.5, 0.7, 0.15, 1.0)),
                (1.0, Vec4::new(0.7, 0.16, 0.03, 0.0)),
            ])
            .expect("the gradient's keys ascend"),
        },
        max_particles: SPARK_SHARE,
    }
}

/// A smoke puff: a slow omnidirectional stream that rises, spreads and thins.
///
/// The emitter this sample stops and starts, so the browser gate has a count
/// that climbs while something is running and falls to nothing after it is not.
/// Its lifetime is short enough that it drains inside
/// [`crate::show::PUFF_OFF_TICKS`], which is what makes the "after it stops"
/// half of that check reachable.
#[must_use]
pub fn smoke_puff() -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Rate { per_second: 120.0 },
        shape: Shape::Point,
        lifetime: (0.6, 1.1),
        speed: (0.4, 1.4),
        size: (0.12, 0.26),
        spin: (-0.8, 0.8),
        modifiers: Modifiers {
            // Upwards: smoke is buoyant, and a puff that fell would read as
            // dust.
            gravity: Vec3::new(0.0, 1.4, 0.0),
            drag: 1.6,
            size: Curve::new(vec![(0.0, 0.6), (0.35, 1.4), (1.0, 0.0)])
                .expect("the size curve's keys ascend"),
            color: Gradient::new(vec![
                (0.0, Vec4::new(0.55, 0.56, 0.60, 0.85)),
                (1.0, Vec4::new(0.18, 0.18, 0.21, 0.0)),
            ])
            .expect("the gradient's keys ascend"),
        },
        max_particles: PUFF_SHARE,
    }
}

/// **The hostile effect.**
///
/// An emitter asking for a hundred thousand particles a second out of a share
/// of [`SPAM_SHARE`], for ever. `docs/plan/sample/10-sparks.md` asks the sample
/// to prove that such a thing "clamps to its pool share and the panel shows it
/// — never a frame-rate cliff", and there is no way to show that without one on
/// the page: an effect nobody wrote is a budget nobody tested.
///
/// Its particles are short-lived, so the clamp is doing work on every step
/// rather than saturating once and going quiet — slots free up constantly and
/// the emitter takes every one of them and is refused the rest.
#[must_use]
pub fn spam() -> EffectDesc {
    EffectDesc {
        spawn: Spawn::Rate {
            per_second: 100_000.0,
        },
        shape: Shape::Cone {
            axis: Vec3::new(0.0, 1.0, 0.0),
            half_angle: 1.1,
        },
        lifetime: (0.2, 0.5),
        speed: (1.5, 4.0),
        size: (0.04, 0.08),
        spin: (-6.0, 6.0),
        modifiers: Modifiers {
            gravity: Vec3::new(0.0, -4.0, 0.0),
            drag: 0.6,
            size: Curve::new(vec![(0.0, 1.0), (1.0, 0.0)]).expect("the size curve's keys ascend"),
            color: Gradient::new(vec![
                (0.0, Vec4::new(0.35, 1.1, 1.4, 1.0)),
                (1.0, Vec4::new(0.10, 0.28, 0.55, 0.0)),
            ])
            .expect("the gradient's keys ascend"),
        },
        max_particles: SPAM_SHARE,
    }
}

/// Bake a gradient into [`PALETTE_STEPS`] material rows, evenly over lifetime.
///
/// The rows are unlit-ish rather than shaded like the stage: a particle is a
/// speck a few centimetres across and a diffuse response on one would be a
/// gradient nobody can see, so the colour goes into `base_color` at full
/// strength and the roughness is left where it makes the least of the sun.
/// Values above one are deliberate on the sparks — the scene target is
/// `Rgba16Float`, and an ember brighter than white is what a later bloom pass
/// is meant to find.
#[must_use]
pub fn palette(gradient: &Gradient) -> Vec<GpuMaterial> {
    (0..PALETTE_STEPS)
        .map(|step| {
            let t = step as f32 / (PALETTE_STEPS - 1) as f32;
            let color = gradient.eval(t);
            GpuMaterial {
                base_color: [color.x, color.y, color.z, 1.0],
                roughness: 0.9,
                ..GpuMaterial::UNTINTED
            }
        })
        .collect()
}

/// Which of an effect's [`PALETTE_STEPS`] rows is nearest `color`.
///
/// The reverse of [`palette`], and the whole of what quantising costs. Nearest
/// in linear RGB, ignoring alpha — the palette carries no alpha, because the
/// pass draws these opaque.
///
/// A search rather than arithmetic on the particle's age, and deliberately: the
/// simulation hands a renderer a **colour**, which is what a pass that could
/// take one would use directly. Turning that colour back into an index is this
/// path's problem, so it is solved here rather than by asking the simulation
/// for a number it only has because this sample wants a row.
#[must_use]
pub fn nearest_row(palette: &[GpuMaterial], color: Vec4) -> usize {
    let mut best = 0;
    let mut best_distance = f32::INFINITY;
    for (row, material) in palette.iter().enumerate() {
        let rgb = Vec3::new(
            material.base_color[0],
            material.base_color[1],
            material.base_color[2],
        );
        let distance = (rgb - color.truncate()).length_squared();
        if distance < best_distance {
            best_distance = distance;
            best = row;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every effect on the page is one the simulation will accept, and every
    /// share is one the pool can actually give out.
    ///
    /// A description the simulation refuses is a demo that opens with an empty
    /// scene and a log line nobody reads, which no picture would show.
    #[test]
    fn every_stock_effect_is_one_the_simulation_accepts() {
        let mut total = 0;
        for (name, effect) in [
            ("impact sparks", impact_sparks()),
            ("smoke puff", smoke_puff()),
            ("spam", spam()),
        ] {
            effect
                .validated()
                .unwrap_or_else(|error| panic!("{name} is not simulable: {error}"));
            total += effect.max_particles;
        }
        assert!(
            total < POOL,
            "the three shares come to {total} of a pool of {POOL}, leaving no headroom \
             for a second burst"
        );
    }

    /// **The hostile effect is hostile.** An emitter asking for less than its
    /// share every step would never be clamped, and the budget claim on the
    /// page would be a label over nothing.
    #[test]
    fn the_hostile_effect_asks_for_far_more_than_its_share() {
        let Spawn::Rate { per_second } = spam().spawn else {
            panic!("the hostile effect is not a rate emitter");
        };
        let per_tick = per_second / crate::show::TICK_HZ as f32;
        assert!(
            per_tick > f64::from(SPAM_SHARE) as f32 * 10.0,
            "the hostile emitter asks for {per_tick} a tick against a share of \
             {SPAM_SHARE}, which is not spam"
        );
    }

    /// A round trip through the palette lands on the row the colour came from,
    /// which is what says the quantisation is a quantisation rather than a
    /// constant.
    #[test]
    fn a_baked_colour_finds_its_own_row_again() {
        for effect in [impact_sparks(), smoke_puff(), spam()] {
            let gradient = &effect.modifiers.color;
            let palette = palette(gradient);
            assert_eq!(palette.len(), PALETTE_STEPS);
            for step in 0..PALETTE_STEPS {
                let t = step as f32 / (PALETTE_STEPS - 1) as f32;
                assert_eq!(
                    nearest_row(&palette, gradient.eval(t)),
                    step,
                    "the colour at {t} does not find the row it was baked into"
                );
            }
        }
    }

    /// And two different colours find two different rows — the control, without
    /// which the test above passes for a palette of eight identical entries.
    #[test]
    fn different_points_of_a_gradient_find_different_rows() {
        let effect = impact_sparks();
        let gradient = &effect.modifiers.color;
        let palette = palette(gradient);
        assert_ne!(
            nearest_row(&palette, gradient.eval(0.0)),
            nearest_row(&palette, gradient.eval(1.0)),
            "the two ends of the gradient quantise to the same material row"
        );
    }

    /// Every effect fades by shrinking, because alpha does nothing on this
    /// path. A particle that reached the end of its life at full size would
    /// vanish as a hard pop.
    #[test]
    fn every_effect_shrinks_to_nothing_rather_than_fading_out() {
        for (name, effect) in [
            ("impact sparks", impact_sparks()),
            ("smoke puff", smoke_puff()),
            ("spam", spam()),
        ] {
            assert_eq!(
                effect.modifiers.size.eval(1.0),
                0.0,
                "{name} is still {} of its size when it retires",
                effect.modifiers.size.eval(1.0)
            );
            assert!(
                effect.modifiers.size.eval(0.0) > 0.0,
                "{name} is born with no size at all"
            );
        }
    }
}
