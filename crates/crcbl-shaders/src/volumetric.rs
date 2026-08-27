//! Single scattering in a homogeneous slice of participating medium: how much
//! light a slab of fog sends toward the eye, and how much of what is behind it
//! survives.
//!
//! [`crate::fog`] answers the second question alone, in closed form, for a
//! medium that only *absorbs*. This module is the other half of the standard
//! froxel volumetric pass described in `docs/plan/43-render-standards.md` §4:
//! the frustum is cut into slices along `z`, each slice is shaded once against
//! the lights its froxel already lists, and the column is composited front to
//! back. What a slice owes that composite is two numbers —
//!
//! ```text
//! scattering    = the radiance the slice itself adds, self-attenuated
//! transmittance = the fraction of everything behind it that gets through
//! ```
//!
//! — and [`integrate_slice`] is both, in one closed form. [`phase`] is the
//! angular half: how a scattering event redistributes light, which is what
//! makes fog glow around a light rather than uniformly.
//!
//! # The integral, and why it is not `source * thickness`
//!
//! A slice of extinction `sigma` and thickness `dt` scatters `source` per unit
//! length toward the eye, but light scattered at the *far* end of the slice
//! still has to cross the rest of the slice to leave it. Integrating that:
//!
//! ```text
//! L = the integral of source * exp(-sigma * s) ds, over s in [0, dt]
//!   = source * (1 - exp(-sigma * dt)) / sigma
//!   = source * dt * one_minus_exp_over(sigma * dt)
//! ```
//!
//! The last form is the one written here, for the reason
//! [`crate::fog::one_minus_exp_over`] exists: as `sigma * dt` goes to zero the
//! middle form is a division of nothing by nothing, and a thin slice is the
//! common case in a froxel grid rather than the edge one. Taking `source * dt`
//! instead — the form that ignores self-attenuation — is the classic way a
//! volumetric pass gets brighter every time someone adds slices, because the
//! error is per slice and there are more of them.
//!
//! `splitting_a_slice_composites_to_the_same_radiance` is what pins that: a
//! homogeneous column cut into any number of slices composites to what one
//! slice of the whole depth gives.
//!
//! # The shading rule
//!
//! `docs/plan/44-lighting.md`: **no transcendental function may reach a
//! colour.** Neither of these two calls one. The exponential is
//! [`crate::fog::exp_neg`], built from operations IEEE-754 pins down; and
//! [`phase`]'s three-halves power is written `d * sqrt(d)` rather than
//! `pow(d, 1.5)`, because IEEE-754 requires a correctly rounded `sqrt` and
//! specifies nothing at all about `pow`.

use crate::fog::{exp_neg, one_minus_exp_over};

/// `1 / (4 pi)`, the phase function of a medium that scatters every direction
/// alike — and the factor every other phase function is a redistribution of.
///
/// Named rather than spelled at the call site because the Slang mirror has to
/// carry it as a literal, there being no `#include` in these shaders, and a
/// named constant is what a drift guard can hold that literal against.
pub const INV_FOUR_PI: f32 = 0.079_577_47;

/// The largest anisotropy [`phase`] will accept, in either direction.
///
/// At `|g| = 1` the Henyey-Greenstein lobe is a delta function and the
/// denominator is exactly zero in the forward direction, so the clamp is what
/// keeps a division by zero out of a colour rather than a tuning knob. It sits
/// well past any medium worth naming — Mie scattering in fog is around `0.8`,
/// smoke lower still — and the lobe it admits is finite and still integrates
/// to one, which `anisotropy_is_clamped_short_of_the_singularity` checks at
/// the clamp itself.
pub const MAX_ANISOTROPY: f32 = 0.99;

/// The Henyey-Greenstein phase function: the fraction of light scattered from
/// one direction into another, per steradian.
///
/// `cos_theta` is the cosine of the angle between the direction the light was
/// travelling and the direction it leaves in, so `1.0` is straight on and
/// `-1.0` is straight back. `g` is the anisotropy — positive scatters forward,
/// zero scatters evenly, negative scatters back — and is clamped to
/// [`MAX_ANISOTROPY`].
///
/// ```text
/// p(g, c) = (1 - g^2) / (4 pi * d * sqrt(d)),   d = 1 + g^2 - 2 g c
/// ```
///
/// It integrates to one over the sphere for every `g`, which is what makes a
/// medium redistribute light rather than create or destroy it, and is checked
/// by `the_phase_integrates_to_one_over_the_sphere`.
#[must_use]
pub fn phase(g: f32, cos_theta: f32) -> f32 {
    let g = g.clamp(-MAX_ANISOTROPY, MAX_ANISOTROPY);
    let denominator = 1.0 + g * g - 2.0 * g * cos_theta.clamp(-1.0, 1.0);
    INV_FOUR_PI * (1.0 - g * g) / (denominator * denominator.sqrt())
}

/// What one slice of medium contributes to the column it sits in.
///
/// Both fields are per colour channel where the caller's medium is coloured;
/// [`integrate_slice`] takes and returns scalars, and a caller with a coloured
/// medium calls it once per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliceIntegral {
    /// The radiance this slice adds, already attenuated by the part of itself
    /// in front of each scattering event.
    pub scattering: f32,
    /// The fraction of the radiance arriving from behind this slice that
    /// leaves the front of it.
    pub transmittance: f32,
}

/// Integrates single scattering across one homogeneous slice.
///
/// `source` is the radiance scattered toward the eye per unit length — the
/// product of the medium's scattering coefficient, [`phase`] at this froxel's
/// geometry, and the light reaching it. `extinction` is the medium's total
/// extinction coefficient, absorption plus scattering, per unit length.
/// `thickness` is how deep the slice is along the view ray.
///
/// A caller composites a column front to back:
///
/// ```
/// use crcbl_shaders::volumetric::integrate_slice;
///
/// let mut radiance = 0.0;
/// let mut through = 1.0_f32;
/// for _ in 0..4 {
///     let slice = integrate_slice(2.0, 0.5, 0.25);
///     radiance += through * slice.scattering;
///     through *= slice.transmittance;
/// }
/// // Which is the same column one slice deep, to within a rounding step.
/// let whole = integrate_slice(2.0, 0.5, 1.0);
/// assert!((radiance - whole.scattering).abs() < 1e-6);
/// ```
///
/// A non-positive `extinction` or `thickness` is a medium that does nothing:
/// the slice transmits exactly one and adds `source * thickness`, which is the
/// limit the integral approaches rather than a special case.
#[must_use]
pub fn integrate_slice(source: f32, extinction: f32, thickness: f32) -> SliceIntegral {
    let thickness = thickness.max(0.0);
    let depth = extinction.max(0.0) * thickness;
    SliceIntegral {
        scattering: source * thickness * one_minus_exp_over(depth),
        transmittance: exp_neg(depth),
    }
}

/// Bytes in the volumetric passes' parameter block, matching
/// `struct VolumetricParams` in `shaders/volumetric.slang` and the identical
/// declaration in `shaders/volumetric_composite.slang`.
///
/// One `float4x4` (64), [`crate::mesh::SHADOW_CASCADES`] more of them, eight
/// `float4` (16 each) and eight `uint`s (32).
pub const PARAMS_SIZE: usize = 64 + crate::mesh::SHADOW_CASCADES * 64 + 8 * 16 + 8 * 4;

/// Bytes one froxel occupies in the column buffer: one `float4`.
///
/// It carries two different pairs at two different times — a slice's own
/// radiance and transmittance after `scatterMain`, the column in front of that
/// slice after `integrateMain` — which `volumetric.slang`'s header argues.
pub const FROXEL_STRIDE: usize = 16;

/// Bytes one froxel occupies in the visibility buffer: one `float`.
///
/// **A second buffer rather than a component of the first**, and the reason is
/// what each of the two is for. The column buffer's four components are all
/// spoken for twice over — a slice's radiance and transmittance before the
/// scan, the prefix and its transmittance after — so there is nowhere in it to
/// put a number that has to survive the scan.
///
/// And it has to survive the scan because `volumetric_composite.slang`
/// integrates the last partial slice along the pixel's own ray, which needs the
/// same scattering source `scatterMain` used for the whole froxel. Reading a
/// scalar back is what lets the cascade lookup exist in exactly one shader:
/// the alternative is a second copy of the atlas walk in the composite, run
/// per pixel, which is the cost the froxel grid exists to avoid.
pub const VISIBILITY_STRIDE: usize = 4;

/// Invocations per workgroup in both of `volumetric.slang`'s entry points.
pub const WORKGROUP_SIZE: u32 = 64;

/// What the scattering, integration and composite passes cannot derive for
/// themselves.
///
/// Written once per frame by `crcbl_render::volumetric` and bound by all three,
/// so a pixel's froxel and the froxel a column was written into come from one
/// set of numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumetricParams {
    /// Clip → world, column-major, for unprojecting a tile's screen centre and
    /// a pixel's own ray.
    pub inverse_view_proj: [f32; 16],
    /// World-space eye in `xyz`; `w` unused.
    pub eye: [f32; 4],
    /// Row 3 of the view-projection, so `dot(depth_row, (p, 1))` is `p`'s view
    /// depth — the row `crcbl_shaders::light::ClusterParams` is handed, from
    /// the same matrix.
    pub depth_row: [f32; 4],
    /// Density, scale height, reference height, unused — the row
    /// `crcbl_shaders::mesh::FrameUniforms::fog_params` carries, handed to
    /// exactly one of the two paths.
    pub fog_params: [f32; 4],
    /// The radiance the medium scatters towards the eye in `rgb`; `w` unused.
    ///
    /// The environment term: what the medium sends towards the eye from every
    /// direction alike, which is the whole of what the closed form in
    /// `mesh.slang` can answer. [`VolumetricParams::sun_radiance`] is what the
    /// froxel path adds to it.
    pub fog_color: [f32; 4],
    /// The unit vector **towards** the sun in `xyz` — `mesh.slang`'s `to_light`
    /// — and the medium's anisotropy in `w`.
    ///
    /// The two ride one row because a phase function is not evaluable without
    /// both, and a frame that had one from this camera and the other from the
    /// last would light a shaft that points nowhere.
    pub sun_direction: [f32; 4],
    /// What the medium scatters out of the sun, per unit length, in `rgb`; `w`
    /// unused.
    ///
    /// **Zero is exactly no sun**, which is what makes this row an off-switch:
    /// the scattering source is a sum, and a zero term leaves the environment
    /// term it is added to bit for bit. That is the state every frame is in
    /// until a caller sets `crcbl_render::Fog`'s `sun_scattering` — named
    /// rather than linked, this crate having no dependencies at all — and it is
    /// what lets the column stay algebraically equal to the closed form.
    pub sun_radiance: [f32; 4],
    /// World → cascade `i`'s shadow clip, column-major, one matrix per cascade
    /// — `crcbl_shaders::mesh::FrameUniforms::shadow_view_proj`, handed to the
    /// scatter pass so a froxel and a fragment ask the same atlas the same
    /// question.
    ///
    /// The scatter pass reads these and the composite does not: the whole point
    /// of the buffer [`VISIBILITY_STRIDE`] measures is that the shadow lookup
    /// happens once per froxel rather than once per pixel. They are in the
    /// shared block anyway because there is one block, and a second one
    /// declared by one shader alone is a layout the two could disagree about.
    pub shadow_view_proj: [[f32; 16]; crate::mesh::SHADOW_CASCADES],
    /// Component `i` is how far from the eye cascade `i` reaches, in world
    /// units — `FrameUniforms::cascade_far`, and the same walk picks the first
    /// cascade that covers a point.
    pub cascade_far: [f32; 4],
    /// `xy`: one shadow-atlas texel in `u` and in `v`, which is what the PCF
    /// kernel steps by. `zw` are the receiver biases `mesh.slang` applies and
    /// **the medium does not**: a bias exists to stop a facet shadowing itself,
    /// and a froxel is a volume with no facet in it.
    ///
    /// Carried whole rather than trimmed to the two components read, because
    /// the row is `crcbl_render::shadow::Cascades::params`' own and a second
    /// spelling of an atlas's texel size is a thing that can drift from the
    /// first.
    pub shadow_params: [f32; 4],
    /// Tiles across.
    pub grid_x: u32,
    /// Tiles down.
    pub grid_y: u32,
    /// Depth slices: `crcbl_shaders::light::CLUSTER_DEPTH_SLICES` for a
    /// perspective camera, `1` for an orthographic one.
    pub slices: u32,
    /// Pixels per tile edge.
    pub tile_pixels: u32,
    /// Viewport width in pixels.
    pub viewport_x: u32,
    /// Viewport height in pixels.
    pub viewport_y: u32,
    /// Froxels the column buffer holds, which every pass bounds its indices by.
    pub froxel_count: u32,
}

impl VolumetricParams {
    /// The block as the bytes the shaders read.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self
            .inverse_view_proj
            .into_iter()
            .chain(self.eye)
            .chain(self.depth_row)
            .chain(self.fog_params)
            .chain(self.fog_color)
            .chain(self.sun_direction)
            .chain(self.sun_radiance)
            .chain(self.shadow_view_proj.into_iter().flatten())
            .chain(self.cascade_far)
            .chain(self.shadow_params)
        {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in [
            self.grid_x,
            self.grid_y,
            self.slices,
            self.tile_pixels,
            self.viewport_x,
            self.viewport_y,
            self.froxel_count,
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        // The trailing `pad0` stays zero: it exists so the block's size is the
        // multiple of 16 bytes a uniform buffer needs, and nothing reads it.
        debug_assert_eq!(at + 4, PARAMS_SIZE, "a field escaped the writer");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anisotropies that cover both lobes, the isotropic case, and the clamp.
    const ANISOTROPIES: [f32; 7] = [-0.9, -0.5, -0.1, 0.0, 0.3, 0.8, MAX_ANISOTROPY];

    /// The two shaders that read [`VolumetricParams`].
    const SHADERS: [(&str, &str); 2] = [
        (
            "volumetric.slang",
            include_str!("../shaders/volumetric.slang"),
        ),
        (
            "volumetric_composite.slang",
            include_str!("../shaders/volumetric_composite.slang"),
        ),
    ];

    /// The literal a shader assigns to a `static const float`, parsed as a
    /// value — `crate::fog`'s `shader_scalar`, and it compares numbers rather
    /// than text for that function's reason.
    fn shader_scalar(source: &str, name: &str) -> f32 {
        let declaration = format!("static const float {name} = ");
        let at = source
            .find(&declaration)
            .unwrap_or_else(|| panic!("the shader declares no {name}"))
            + declaration.len();
        let rest = &source[at..];
        let end = rest.find(';').expect("a declaration ends in a semicolon");
        rest[..end]
            .trim()
            .parse()
            .unwrap_or_else(|error| panic!("{name} is not a float: {error}"))
    }

    /// `2 pi` times Simpson's rule over `cos_theta`, which is the solid-angle
    /// integral of any function that depends on direction only through it.
    ///
    /// `f64` throughout and a step far finer than the lobe: a test is not
    /// shading, so the rule this module's construction exists to satisfy does
    /// not bind here.
    fn over_the_sphere(g: f32) -> f64 {
        const PANELS: usize = 200_000;
        let at = |c: f64| f64::from(phase(g, c as f32));
        let step = 2.0 / PANELS as f64;
        let mut total = at(-1.0) + at(1.0);
        for panel in 1..PANELS {
            let c = -1.0 + step * panel as f64;
            total += at(c) * if panel % 2 == 0 { 2.0 } else { 4.0 };
        }
        total * step / 3.0 * 2.0 * core::f64::consts::PI
    }

    /// An isotropic medium scatters every direction alike, and the constant it
    /// scatters them by is the one the sphere's solid angle names.
    ///
    /// Exactly, not nearly: at `g = 0` the whole numerator and denominator are
    /// one, so a construction that reached this value by any other route than
    /// the reciprocal it claims would show up here.
    #[test]
    fn an_isotropic_phase_is_the_reciprocal_of_the_sphere() {
        for step in 0..=20u8 {
            let cos_theta = -1.0 + f32::from(step) * 0.1;
            assert_eq!(
                phase(0.0, cos_theta),
                INV_FOUR_PI,
                "an isotropic lobe varied with direction at {cos_theta}"
            );
        }
        let ulps = (f64::from(INV_FOUR_PI) - 1.0 / (4.0 * core::f64::consts::PI)).abs()
            / f64::from(f32::EPSILON / 2.0)
            * 4.0
            * core::f64::consts::PI;
        assert!(ulps <= 1.0, "INV_FOUR_PI is {ulps} ulp from 1/(4 pi)");
    }

    /// Every lobe redistributes light rather than making or destroying it.
    ///
    /// This is the property the whole model rests on, and the one a mistyped
    /// normalisation breaks silently: dropping the `1 - g^2` factor still
    /// gives a plausible forward lobe, and still tonemaps to something that
    /// looks like fog.
    #[test]
    fn the_phase_integrates_to_one_over_the_sphere() {
        for g in ANISOTROPIES {
            let total = over_the_sphere(g);
            assert!(
                (total - 1.0).abs() < 1e-3,
                "a lobe at g = {g} integrated to {total}, not one"
            );
        }
    }

    /// Reversing the anisotropy mirrors the lobe, exactly.
    ///
    /// The sign of `g` is the whole difference between fog that glows around a
    /// light and fog that glows away from one, and this is the cheapest way to
    /// catch it having been folded into the denominator the wrong way round.
    #[test]
    fn the_sign_of_g_mirrors_the_lobe() {
        for g in ANISOTROPIES {
            for step in 0..=20u8 {
                let cos_theta = -1.0 + f32::from(step) * 0.1;
                assert_eq!(
                    phase(g, cos_theta),
                    phase(-g, -cos_theta),
                    "the lobe at g = {g} was not the mirror of the one at {}",
                    -g
                );
            }
        }
    }

    /// A positive anisotropy peaks straight ahead and falls off all the way
    /// back, with no flat stretch anywhere in between.
    ///
    /// Monotonicity is what rules out the denominator's two `g` terms having
    /// swapped places: `1 + g^2 - 2 g c` and `1 + g^2 + 2 g c` both peak, and
    /// they peak at opposite ends.
    #[test]
    fn a_forward_lobe_peaks_ahead_and_falls_off_behind() {
        for g in [0.3, 0.8, MAX_ANISOTROPY] {
            let mut previous = phase(g, -1.0);
            for step in 1..=200u8 {
                let cos_theta = -1.0 + f32::from(step) * 0.01;
                let here = phase(g, cos_theta);
                assert!(
                    here > previous,
                    "the lobe at g = {g} did not rise at {cos_theta}: {here} after {previous}"
                );
                previous = here;
            }
            assert!(
                phase(g, 1.0) > phase(g, -1.0) * 4.0,
                "a lobe at g = {g} was barely forward at all"
            );
        }
    }

    /// The singularity at `|g| = 1` is not reachable, and what the clamp
    /// admits is still a phase function.
    ///
    /// A caller handed the anisotropy straight through would divide by zero
    /// looking straight down a forward lobe, and the frame would carry an
    /// infinity from a value that reads as physical.
    #[test]
    fn anisotropy_is_clamped_short_of_the_singularity() {
        for g in [1.0, -1.0, 4.0, -4.0, f32::MAX] {
            let peak = phase(g, g.signum());
            assert!(peak.is_finite(), "g = {g} produced {peak} straight ahead");
            assert_eq!(
                peak,
                phase(g.signum() * MAX_ANISOTROPY, g.signum()),
                "g = {g} was not clamped to MAX_ANISOTROPY"
            );
        }
        let total = over_the_sphere(MAX_ANISOTROPY);
        assert!(
            (total - 1.0).abs() < 1e-3,
            "the lobe at the clamp integrated to {total}, not one"
        );
    }

    /// A medium with no extinction hides nothing and attenuates nothing, so
    /// its slice is its source over its length and no less.
    ///
    /// Exact on both counts: `exp_neg(0.0)` is `1.0` and
    /// `one_minus_exp_over(0.0)` is `1.0`, which is what makes switching the
    /// medium off a no-op rather than nearly one.
    #[test]
    fn a_slice_of_nothing_is_its_source_over_its_length() {
        for thickness in [0.0, 0.25, 1.0, 40.0] {
            let slice = integrate_slice(3.0, 0.0, thickness);
            assert_eq!(slice.transmittance, 1.0);
            assert_eq!(slice.scattering, 3.0 * thickness);
        }
        // And a negative coefficient is the same medium, not an amplifier.
        let negative = integrate_slice(3.0, -2.0, 1.0);
        assert_eq!(negative, integrate_slice(3.0, 0.0, 1.0));
        assert_eq!(
            integrate_slice(3.0, 2.0, -1.0),
            integrate_slice(3.0, 2.0, 0.0)
        );
    }

    /// Cutting a column into more slices does not change what it looks like.
    ///
    /// **The property the whole integral exists for.** Dropping the
    /// self-attenuation — `source * thickness` alone, which is the form that
    /// reads correctly and is what a froxel pass reaches for first — passes
    /// every other test in this module and fails only this one, and it fails
    /// it in the direction that matters: more slices, more light.
    #[test]
    fn splitting_a_slice_composites_to_the_same_radiance() {
        for (source, extinction, depth) in [
            (2.0_f32, 0.5_f32, 1.0_f32),
            (0.25, 4.0, 8.0),
            (10.0, 0.001, 0.5),
            (1.0, 1.0, 30.0),
        ] {
            let whole = integrate_slice(source, extinction, depth);
            for slices in [1_u32, 2, 7, 64, 512] {
                let thickness = depth / slices as f32;
                let mut radiance = 0.0_f32;
                let mut through = 1.0_f32;
                for _ in 0..slices {
                    let slice = integrate_slice(source, extinction, thickness);
                    radiance += through * slice.scattering;
                    through *= slice.transmittance;
                }
                let error = (radiance - whole.scattering).abs() / whole.scattering.max(1e-6);
                assert!(
                    error < 1e-3,
                    "{slices} slices of a column {depth} deep gave {radiance}, \
                     against {} for one",
                    whole.scattering
                );
                let transmitted = (through - whole.transmittance).abs();
                assert!(
                    transmitted < 1e-4,
                    "{slices} slices transmitted {through}, against {} for one",
                    whole.transmittance
                );
            }
        }
    }

    /// What a slice scatters and what it hides are the same integral read two
    /// ways: a slice that lets nothing through has scattered every unit of
    /// source it could.
    ///
    /// `1 - T = sigma * dt * one_minus_exp_over(sigma * dt)`, so a unit source
    /// in a medium that scatters everything it extinguishes gives back exactly
    /// the light the medium took out. An integral off by any factor at all
    /// separates the two sides.
    ///
    /// The oracle is `f64`, not `1.0 - slice.transmittance`. That difference
    /// is the cancellation [`crate::fog::one_minus_exp_over`] exists to avoid
    /// — at an optical depth of `1e-6` it keeps four significant digits — so
    /// checking against it would be holding the accurate side to the
    /// inaccurate one.
    #[test]
    fn a_slice_scatters_exactly_what_it_extinguishes() {
        for extinction in [0.001_f32, 0.1, 1.0, 5.0, 40.0] {
            for thickness in [0.001_f32, 0.05, 1.0, 3.0] {
                let slice = integrate_slice(extinction, extinction, thickness);
                let hidden = -(-f64::from(extinction) * f64::from(thickness)).exp_m1();
                let error = (f64::from(slice.scattering) - hidden).abs() / hidden;
                assert!(
                    error < 1e-5,
                    "a slice of {extinction} over {thickness} scattered {} \
                     while hiding {hidden}",
                    slice.scattering
                );
            }
        }
    }

    /// Both shaders cut the frustum at the same depths, and those depths are
    /// the grid `crcbl_shaders::light` describes.
    ///
    /// The pair is one structure written in two files: the scatter pass walks
    /// the boundaries forward to place a slice, and the composite walks the
    /// same chain to find which slice a pixel is in. A constant that differs
    /// between them puts a pixel's partial segment in a slice whose prefix
    /// belongs to a different one — a frame that is plausibly foggy and wrong
    /// by a whole slice at the far end.
    #[test]
    fn the_shaders_cut_the_frustum_where_the_light_grid_does() {
        for (file, source) in SHADERS {
            for (value, name) in [
                (crate::light::CLUSTER_NEAR, "CLUSTER_NEAR"),
                (crate::light::CLUSTER_FAR, "CLUSTER_FAR"),
                (crate::light::SLICE_RATIO, "CLUSTER_SLICE_RATIO"),
            ] {
                assert_eq!(
                    shader_scalar(source, name),
                    value,
                    "{file}'s {name} is not crcbl_shaders::light's"
                );
            }
            let slices = format!(
                "static const uint CLUSTER_DEPTH_SLICES = {};",
                crate::light::CLUSTER_DEPTH_SLICES
            );
            assert!(
                source.contains(&slices),
                "{file} does not declare {} depth slices",
                crate::light::CLUSTER_DEPTH_SLICES
            );
        }
    }

    /// Both shaders declare the same parameter block, field for field.
    ///
    /// Compared as text with the whitespace collapsed, because that is exactly
    /// what has to match: `crcbl_shaders::declaration_order` records that Metal
    /// and D3D12 lay a block out in declaration order, so two shaders bound to
    /// **one** buffer with two orderings read each other's fields — an eye
    /// where a fog colour should be, which renders rather than failing.
    #[test]
    fn the_two_shaders_declare_one_block() {
        let block = |source: &str| {
            let at = source
                .find("struct VolumetricParams")
                .expect("the block is declared");
            let rest = &source[at..];
            let end = rest.find("\n};").expect("the block is closed");
            rest[..end]
                .lines()
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty() && !line.starts_with("///") && !line.starts_with("//")
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(
            block(SHADERS[0].1),
            block(SHADERS[1].1),
            "{} and {} declare different parameter blocks",
            SHADERS[0].0,
            SHADERS[1].0
        );
    }

    /// The block's bytes are its fields in declaration order, and the writer
    /// covers every one of them.
    ///
    /// Every field is given a distinct value, so a writer that skipped one or
    /// swapped two lands a plausible number in the wrong place — which is the
    /// failure that renders instead of erroring.
    #[test]
    fn the_params_block_writes_its_fields_in_declaration_order() {
        let params = VolumetricParams {
            inverse_view_proj: [0.5; 16],
            eye: [1.5; 4],
            depth_row: [2.5; 4],
            fog_params: [3.5; 4],
            fog_color: [4.5; 4],
            sun_direction: [5.5; 4],
            sun_radiance: [6.5; 4],
            shadow_view_proj: [[7.5; 16], [8.5; 16]],
            cascade_far: [9.5; 4],
            shadow_params: [10.5; 4],
            grid_x: 7,
            grid_y: 11,
            slices: 13,
            tile_pixels: 17,
            viewport_x: 19,
            viewport_y: 23,
            froxel_count: 29,
        };
        let bytes = params.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);

        let float_at = |offset: usize| {
            f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
        };
        let uint_at = |offset: usize| {
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
        };
        for (offset, value) in [
            (0, 0.5),
            (64, 1.5),
            (80, 2.5),
            (96, 3.5),
            (112, 4.5),
            (128, 5.5),
            (144, 6.5),
            (160, 7.5),
            (224, 8.5),
            (288, 9.5),
            (304, 10.5),
        ] {
            assert_eq!(float_at(offset), value, "the row at byte {offset}");
        }
        for (offset, value) in [
            (320, 7),
            (324, 11),
            (328, 13),
            (332, 17),
            (336, 19),
            (340, 23),
            (344, 29),
        ] {
            assert_eq!(uint_at(offset), value, "the word at byte {offset}");
        }
        assert_eq!(uint_at(348), 0, "the pad word is not written");
    }

    /// Both shaders spell this module's phase function, with this module's
    /// constants.
    ///
    /// It exists twice because there is no `#include` in these shaders and both
    /// passes need it: `scatterMain` shades a whole slice along its tile's
    /// centre ray, and the composite shades the partial slice along the pixel's
    /// own. A copy edited in one file and not the other lights a shaft that
    /// changes shape at every slice boundary — a picture, and a wrong one.
    ///
    /// The numbers are compared as values rather than as text, for
    /// `shader_scalar`'s reason; the signature is compared as text, because
    /// what it pins is that the copy is the same *function* and takes its
    /// arguments in the same order.
    #[test]
    fn both_shaders_spell_this_module_s_phase_function() {
        for (file, source) in SHADERS {
            for (value, name) in [
                (INV_FOUR_PI, "VOLUMETRIC_INV_FOUR_PI"),
                (MAX_ANISOTROPY, "VOLUMETRIC_MAX_ANISOTROPY"),
            ] {
                assert_eq!(
                    shader_scalar(source, name),
                    value,
                    "{file}'s {name} is not this module's"
                );
            }
            assert!(
                source.contains("float volumetric_phase(float g, float cos_theta)"),
                "{file} does not declare this module's phase function"
            );
            // The lobe itself: the numerator, the denominator, and the
            // three-halves power written as a multiply by a square root rather
            // than as `pow` — which is the shading rule, not a style choice.
            assert!(
                source.contains("float d = 1.0 + a * a - 2.0 * a * clamp(cos_theta, -1.0, 1.0);"),
                "{file}'s phase denominator is not this module's"
            );
            assert!(
                source.contains("return VOLUMETRIC_INV_FOUR_PI * (1.0 - a * a) / (d * sqrt(d));"),
                "{file}'s phase lobe is not this module's, or reaches for `pow`"
            );
        }
    }

    /// The shadow constants `volumetric.slang` and its composite carry are this
    /// workspace's, and `volumetric.slang`'s cascade lookup is `mesh.slang`'s.
    ///
    /// The atlas is one image with one layout, read by a fragment stage through
    /// `mesh.slang` and by a compute stage through `volumetric.slang`, and there
    /// is no `#include` to share the walk. A copy that drifted would put a
    /// shaft's edge somewhere other than the shadow it belongs to — two pictures
    /// that each look plausible alone and disagree where they meet.
    ///
    /// The two files are allowed to differ in **one** thing: the name of the
    /// uniform block the atlas's texel size is read from, which is `frame` in
    /// one and `params` in the other. That substitution is made on both sides
    /// before the comparison; everything else is compared letter for letter,
    /// with runs of whitespace collapsed so a reformat is not a failure.
    #[test]
    fn both_shaders_spell_the_same_atlas_walk() {
        const MESH: &str = include_str!("../shaders/mesh.slang");
        const VOLUMETRIC: &str = include_str!("../shaders/volumetric.slang");

        // The cascade count is a *layout* number — it sizes the matrix array in
        // the shared block — so both files carry it whether they walk a cascade
        // or not. The grid is only the walk's, and only one file walks.
        let cascades = u32::try_from(crate::mesh::SHADOW_CASCADES).expect("a handful of cascades");
        for (file, source) in SHADERS {
            let spelling = format!("static const uint SHADOW_CASCADES = {cascades};");
            assert!(
                source.contains(&spelling),
                "{file} does not declare `{spelling}`, so the block it binds is a \
                 different size from the one the other shader binds"
            );
        }
        for (value, name) in [
            (crate::mesh::SHADOW_ATLAS_COLUMNS, "SHADOW_ATLAS_COLUMNS"),
            (crate::mesh::SHADOW_ATLAS_ROWS, "SHADOW_ATLAS_ROWS"),
        ] {
            let spelling = format!("static const uint {name} = {value};");
            assert!(
                VOLUMETRIC.contains(&spelling),
                "volumetric.slang does not declare `{spelling}`, so its shadow \
                 atlas is not the one `crcbl_render::shadow` fills"
            );
        }

        // The filter's shape, which `tile_pcf` reads and neither file computes:
        // a tap count, a reach and two tables. Compared as text, because what
        // has to agree is every digit — a shaft of light filtered on a different
        // disc from the surface behind it has two penumbrae where the scene has
        // one, and no assertion about the *walk* would see it.
        for (name, terminator) in [
            ("uint SHADOW_TAPS", ";"),
            ("float SHADOW_FILTER_TEXELS", ";"),
            ("float2 SHADOW_DISC", "};"),
            ("float2 SHADOW_ROTATIONS", "};"),
            ("uint SHADOW_DITHER", "};"),
        ] {
            assert_eq!(
                one_declaration(MESH, name, terminator),
                one_declaration(VOLUMETRIC, name, terminator),
                "`{name}` has drifted between mesh.slang and volumetric.slang"
            );
        }

        for signature in [
            "float2 atlas_uv(uint tile, float2 tile_uv)",
            "float tile_pcf(uint tile, float2 tile_uv, float reference, float2 pixel)",
        ] {
            assert_eq!(
                one_function(MESH, signature, "frame."),
                one_function(VOLUMETRIC, signature, "params."),
                "`{signature}` has drifted between mesh.slang and volumetric.slang"
            );
        }
    }

    /// The text of one `static const` declaration, from its name to
    /// `terminator`, with runs of whitespace collapsed to one space.
    ///
    /// Unlike `one_function` this keeps everything it finds: a table has no
    /// comments inside it, and the whole point of comparing two copies of one is
    /// that every literal in them is the same literal.
    fn one_declaration(source: &str, name: &str, terminator: &str) -> String {
        let at = source
            .find(&format!("static const {name}"))
            .unwrap_or_else(|| panic!("no `static const {name}` in this shader"));
        let tail = &source[at..];
        let end = tail
            .find(terminator)
            .unwrap_or_else(|| panic!("`{name}` is never terminated by `{terminator}`"));
        tail[..end + terminator.len()]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The body of the function `signature` opens: its code, with `//` comments
    /// dropped and runs of whitespace collapsed to one space.
    ///
    /// **Comments are dropped deliberately.** What has to agree between two
    /// copies of a function is the arithmetic; a copy that explains itself in
    /// its own terms — one file saying why the grid scales an atlas texel, the
    /// other saying which file it came from — is a copy doing its job, and a
    /// guard that failed on it would be a guard authors route around.
    ///
    /// `block` is the name the file gives its uniform block, with the dot —
    /// `frame.` in one shader and `params.` in the other — and it is replaced
    /// wherever a token *starts* with it. Starts, rather than anywhere: the
    /// field these two read is `shadow_params`, so a plain substring
    /// replacement turns `params.shadow_params.xy` into nonsense that then
    /// compares unequal for a reason that is not drift.
    ///
    /// Braces are counted rather than matched against a grammar, which is
    /// enough for these two: neither has a brace inside a string, and a copy
    /// that grew one would fail the comparison above rather than pass it
    /// silently.
    fn one_function(source: &str, signature: &str, block: &str) -> String {
        let at = source
            .find(signature)
            .unwrap_or_else(|| panic!("no `{signature}` in this shader"));
        let body = &source[at + signature.len()..];
        let open = body.find('{').expect("a function has a body");
        let mut depth = 0i32;
        let mut end = None;
        for (offset, character) in body[open..].char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.expect("a function body closes");
        body[open..end]
            .lines()
            .map(|line| line.split("//").next().unwrap_or(line))
            .flat_map(str::split_whitespace)
            .map(|token| match token.strip_prefix(block) {
                Some(rest) => format!("BLOCK.{rest}"),
                None => token.to_owned(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The composite scatters its partial slice through the froxel's own
    /// visibility rather than through a constant.
    ///
    /// **A text guard, and it is here because no rendered frame catches this.**
    /// Replacing `visibilities[froxel]` with `1.0` leaves every GPU test in
    /// `crcbl`'s `mesh_e2e` green to the digit: those tests measure background
    /// texels, whose partial slice is the tail of a column whose transmittance
    /// has already gone to nothing, and the texels where the partial slice
    /// *does* carry the frame are the ones a surface was drawn into — where
    /// turning the cascades on or off moves the surface's own shading too, so a
    /// two-frame comparison cannot attribute the difference to the medium.
    ///
    /// What would catch it is the per-froxel readback `docs/backlog.md` carries
    /// as a gap. Until that exists this is what stands between the composite and
    /// a seam at every slice boundary, and it is worth exactly what it says: the
    /// read is written down, not that it is right.
    #[test]
    fn the_composite_scatters_its_partial_slice_through_the_froxel_s_visibility() {
        const COMPOSITE: &str = include_str!("../shaders/volumetric_composite.slang");
        assert!(
            COMPOSITE.contains("volumetric_source(view_direction, visibilities[froxel])"),
            "volumetric_composite.slang no longer sources its partial slice from the \
             froxel's own visibility, so the shadow ends at the slice boundary"
        );
    }

    /// Every shader that spells the phase function is in [`SHADERS`].
    ///
    /// [`the_two_shaders_declare_one_block`] and the test above both walk a
    /// hand-written list, and a hand-written list is exactly what a third copy
    /// would not be added to. This reads the shader directory instead, so a new
    /// file carrying the copy fails here rather than drifting unguarded —
    /// `crate::fog`'s `every_shader_that_spells_the_exponential_is_guarded` is
    /// the same guard over the same hazard.
    #[test]
    fn every_shader_that_spells_the_phase_function_is_guarded() {
        let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders");
        let mut found: Vec<String> = std::fs::read_dir(directory)
            .expect("the shader directory is beside this crate")
            .map(|entry| entry.expect("a readable directory entry").path())
            .filter(|path| path.extension().is_some_and(|kind| kind == "slang"))
            .filter(|path| {
                std::fs::read_to_string(path)
                    .expect("a readable shader")
                    .contains("float volumetric_phase(float g, float cos_theta)")
            })
            .map(|path| {
                path.file_name()
                    .expect("a shader has a file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        found.sort();
        let mut guarded: Vec<String> = SHADERS.iter().map(|(name, _)| (*name).to_owned()).collect();
        guarded.sort();
        assert_eq!(
            found, guarded,
            "the shaders carrying the phase function are not the ones this module guards"
        );
    }
}
