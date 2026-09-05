//! Hillaire's sky and atmosphere, with every transcendental spent at cook time
//! or built out of [`crate::fog`]'s construction.
//!
//! "A Scalable and Production Ready Sky and Atmosphere Rendering Technique"
//! (Sébastien Hillaire, EGSR 2020) replaces a gradient with the light a real
//! planet's air scatters: Rayleigh scattering off molecules, Mie scattering off
//! aerosols, and ozone absorption in a band around 25 km. The medium's
//! coefficients are Bruneton and Neyret's, restated by Hillaire, and every one
//! of them is a named constant below with the paper it comes from.
//!
//! `docs/plan/43-render-standards.md` §8 decided the technique and the shape it
//! takes here, which is not quite the paper's:
//!
//! * **The transmittance and multiple-scattering LUTs are cooked and
//!   committed**, in `tables/atmosphere.bin`, exactly as
//!   [`crate::dfg`]'s split-sum table is. Neither depends on the sun — only on
//!   the planet — so neither has to be rebuilt at run time at all, and a
//!   committed artifact is bytes four backends read rather than four
//!   integrators disagreeing in a last place.
//! * **The sky-view LUT is built on the host**, by [`SkyView::build`], out of
//!   multiplies, adds, divisions, square roots and [`crate::fog::exp_neg`]. It
//!   depends on the sun, so it is rebuilt when the sun moves and uploaded; the
//!   arithmetic is restricted so that the *same* bytes reach every backend.
//! * **The device does one fetch.** `shaders/sky.slang` samples the uploaded
//!   sky-view LUT for the background and `mesh.slang` reads
//!   [`SkyView::irradiance`]'s L1 projection for the ambient term, which is the
//!   record `crate::sky::SkyGradient::irradiance` already fills.
//!
//! # Why the parameterisation is not the paper's
//!
//! Hillaire's sky-view LUT is indexed by a view zenith **angle** and an azimuth
//! **angle** relative to the sun, so both building it and reading it take a
//! `sin`, a `cos` or an `acos`. `docs/plan/44-lighting.md`'s rule forbids a
//! transcendental that reaches a colour, and a sky is nothing but colour — so
//! this module indexes the same field by two algebraic coordinates instead:
//!
//! * [`sky_view_up_of`] maps the vertical axis to the direction's `y` through
//!   `sign(s)·s²`, which concentrates texels at the horizon the way the
//!   paper's `sqrt` of the angle does, and whose inverse is a `sqrt`.
//! * [`sky_view_cosine_of`] maps the horizontal axis to the **cosine** of the
//!   azimuth away from the sun through `1 − 2u²`, which is uniform in the angle
//!   near the sun — where the aureole is — and whose inverse is a `sqrt`.
//!
//! Both directions of both maps are multiplies, adds and square roots, so the
//! shader reads the LUT with a dot product and a `sqrt` and the host builds it
//! with the same. The field being stored is the paper's; only the grid it is
//! stored on is this one's.
//!
//! # What this module does not do
//!
//! **No sun disc.** The LUT holds the scattered sky alone, as the paper's does;
//! the sun itself is a directional light the forward pass already shades with.
//!
//! **No aerial perspective.** The paper's third LUT — the froxel volume that
//! puts the air *in front of* a surface — is not built here.
//! `crate::volumetric` owns the froxel column this engine has.
//!
//! **The ground is black below the horizon.** A view ray that meets the planet
//! stops there and contributes only the air in front of it, so
//! [`SkyView::irradiance`]'s lower hemisphere is the air's own glow and not a
//! bounce. What bounces off the scene's floor is
//! `docs/plan/50-irradiance-probes.md`'s volume, and adding an idealised
//! sphere's albedo here as well would count it twice.
//!
//! Regenerate or verify the committed tables with the tool that owns them:
//!
//! ```text
//! cargo run -p crcbl-shaders --example cook-atmosphere            # regenerate
//! cargo run -p crcbl-shaders --example cook-atmosphere -- --check # verify only
//! ```

use crate::fog::{exp_neg, one_minus_exp_over};
use crate::probe::GpuProbe;

/// The planet's radius, in kilometres.
///
/// Bruneton and Neyret's Earth, as Hillaire 2020 restates it: a ground radius
/// of 6360 km and an atmosphere 100 km deep. Every length in this module is in
/// kilometres, which is the unit those coefficients are quoted in.
pub const GROUND_RADIUS_KM: f32 = 6360.0;

/// The top of the atmosphere, in kilometres from the planet's centre.
///
/// See [`GROUND_RADIUS_KM`]. Above this the medium is taken to be empty, which
/// is where a view ray's march stops.
pub const TOP_RADIUS_KM: f32 = 6460.0;

/// Rayleigh scattering at sea level, per kilometre, for the engine's three
/// channels.
///
/// Bruneton's spectral coefficients integrated against the sRGB primaries:
/// `5.802e-6`, `13.558e-6` and `33.1e-6` per metre, restated per kilometre.
/// The blue channel is six times the red, which is the whole reason the sky is
/// blue and the setting sun is red.
///
/// Rayleigh scattering has no absorption: this is the extinction as well.
pub const RAYLEIGH_SCATTERING_PER_KM: [f32; 3] = [0.005_802, 0.013_558, 0.033_1];

/// The height over which Rayleigh density falls by a factor of `e`, in
/// kilometres.
pub const RAYLEIGH_SCALE_HEIGHT_KM: f32 = 8.0;

/// Mie scattering at sea level, per kilometre.
///
/// Bruneton's `3.996e-6` per metre. Grey rather than per channel, which is what
/// makes an aerosol haze white where the molecular sky is blue.
pub const MIE_SCATTERING_PER_KM: f32 = 0.003_996;

/// Mie extinction at sea level, per kilometre — scattering plus absorption.
///
/// Bruneton's `4.44e-6` per metre, so the aerosol's single-scattering albedo is
/// [`MIE_SCATTERING_PER_KM`] over this, a shade under 0.9.
pub const MIE_EXTINCTION_PER_KM: f32 = 0.004_44;

/// The height over which Mie density falls by a factor of `e`, in kilometres.
pub const MIE_SCALE_HEIGHT_KM: f32 = 1.2;

/// The Henyey-Greenstein asymmetry of the aerosol phase function.
///
/// Hillaire's 0.8: strongly forward-scattering, which is the bright halo around
/// a low sun.
pub const MIE_PHASE_G: f32 = 0.8;

/// Ozone absorption at the layer's peak, per kilometre, per channel.
///
/// Bruneton's `0.650e-6`, `1.881e-6` and `0.085e-6` per metre. Ozone scatters
/// nothing; it only absorbs, and it absorbs in the green far more than in the
/// red or the blue, which is what keeps a twilight sky from turning brown.
pub const OZONE_ABSORPTION_PER_KM: [f32; 3] = [0.000_650, 0.001_881, 0.000_085];

/// The height the ozone layer peaks at, in kilometres.
///
/// Bruneton's tent profile, which is 1 at this height and falls linearly to
/// zero [`OZONE_HALF_WIDTH_KM`] either side of it — see [`ozone_density`].
pub const OZONE_PEAK_HEIGHT_KM: f32 = 25.0;

/// How far either side of [`OZONE_PEAK_HEIGHT_KM`] the ozone profile reaches,
/// in kilometres.
pub const OZONE_HALF_WIDTH_KM: f32 = 15.0;

/// The idealised planet's own albedo, used by the multiple-scattering cook and
/// by nothing else.
///
/// Hillaire's sample scene's 0.3. It is what the second and later scattering
/// orders bounce off, and it is deliberately *not* applied to a view ray that
/// meets the ground — the module header says why.
pub const GROUND_ALBEDO: f32 = 0.3;

/// Texels across the transmittance LUT — the `cos` of the view zenith angle,
/// through Bruneton's mapping.
///
/// Hillaire's own 256×64. The table is host-only: it is read while
/// [`SkyView::build`] marches and while the multiple-scattering LUT is cooked,
/// and it is never uploaded, so its size costs a device nothing.
pub const TRANSMITTANCE_WIDTH: usize = 256;

/// Texels down the transmittance LUT — the altitude. See
/// [`TRANSMITTANCE_WIDTH`].
pub const TRANSMITTANCE_HEIGHT: usize = 64;

/// Texels along each axis of the multiple-scattering LUT: the `cos` of the sun
/// zenith angle across, altitude down.
///
/// Hillaire's own 32×32. The function is very smooth in both arguments — it is
/// an already-integrated quantity — so this is generous rather than tight.
pub const MULTISCATTER_SIZE: usize = 32;

/// Bytes one entry of either committed LUT occupies: three little-endian
/// `f32`s, one per channel.
///
/// `f32` rather than the half pair a GPU would sample, for
/// [`crate::dfg::DFG_ENTRY_BYTES`]' reason: neither table is uploaded, so there
/// is nothing to be gained by rounding them, and the artifact is compared as
/// numbers rather than as texels.
pub const ENTRY_BYTES: usize = 12;

/// Where the transmittance LUT ends and the multiple-scattering LUT begins in
/// `tables/atmosphere.bin`.
pub const MULTISCATTER_OFFSET: usize = TRANSMITTANCE_WIDTH * TRANSMITTANCE_HEIGHT * ENTRY_BYTES;

/// The committed artifact's exact length: the transmittance LUT followed by the
/// multiple-scattering LUT.
///
/// **One file for two tables**, where [`crate::dfg`] and
/// [`crate::sky_prefilter`] have one each, because these two are one cook: the
/// multiple-scattering integrator reads the transmittance LUT it was just
/// handed, so a tree where one is regenerated and the other is not is a tree
/// with no meaning. One artifact makes that state unrepresentable.
pub const TABLE_BYTES: usize =
    MULTISCATTER_OFFSET + MULTISCATTER_SIZE * MULTISCATTER_SIZE * ENTRY_BYTES;

/// The committed tables, `tables/atmosphere.bin`.
///
/// In the binary, exactly as the compiled shaders are, so there is no file for
/// a deployment to lose.
const TABLE: &[u8; TABLE_BYTES] = include_bytes!("../tables/atmosphere.bin");

/// The committed bytes, for a tool holding them to their integrator.
#[must_use]
pub const fn bytes() -> &'static [u8; TABLE_BYTES] {
    TABLE
}

/// Where along an axis of `texels` texels the texel at `index` sits — at its
/// centre.
///
/// [`crate::dfg::axis_value`]'s convention, restated for two tables that do not
/// share a size: a texel stands for the middle of the interval it covers, which
/// is what a clamped bilinear read assumes.
#[must_use]
pub fn axis_value(index: usize, texels: usize) -> f32 {
    (index as f32 + 0.5) / texels as f32
}

/// The three `f32`s at `entry` of the committed artifact, counted from
/// `offset`.
fn table_entry(offset: usize, entry: usize) -> [f32; 3] {
    let at = offset + entry * ENTRY_BYTES;
    let mut out = [0.0f32; 3];
    for (channel, slot) in out.iter_mut().enumerate() {
        let base = at + channel * 4;
        *slot = f32::from_le_bytes([
            TABLE[base],
            TABLE[base + 1],
            TABLE[base + 2],
            TABLE[base + 3],
        ]);
    }
    out
}

/// The committed transmittance entry at `(zenith_index, altitude_index)`.
///
/// # Panics
///
/// If either index is outside the table.
#[must_use]
pub fn transmittance_entry(zenith_index: usize, altitude_index: usize) -> [f32; 3] {
    assert!(
        zenith_index < TRANSMITTANCE_WIDTH && altitude_index < TRANSMITTANCE_HEIGHT,
        "({zenith_index}, {altitude_index}) is outside a \
         {TRANSMITTANCE_WIDTH}x{TRANSMITTANCE_HEIGHT} table"
    );
    table_entry(0, altitude_index * TRANSMITTANCE_WIDTH + zenith_index)
}

/// The committed multiple-scattering entry at `(sun_index, altitude_index)`.
///
/// # Panics
///
/// If either index is at or past [`MULTISCATTER_SIZE`].
#[must_use]
pub fn multiscatter_entry(sun_index: usize, altitude_index: usize) -> [f32; 3] {
    assert!(
        sun_index < MULTISCATTER_SIZE && altitude_index < MULTISCATTER_SIZE,
        "({sun_index}, {altitude_index}) is outside a {MULTISCATTER_SIZE}-square table"
    );
    table_entry(
        MULTISCATTER_OFFSET,
        altitude_index * MULTISCATTER_SIZE + sun_index,
    )
}

// ---------------------------------------------------------------------------
// RUNTIME PATH BEGINS
//
// Everything between this marker and the one that closes it is what
// `SkyView::build` reaches, and therefore what reaches a colour. It may use
// multiplies, adds, divisions, square roots, comparisons and
// `crate::fog::exp_neg` — every one of which IEEE-754 pins down — and nothing
// else. `the_runtime_path_calls_no_transcendental` reads this file between the
// two markers and fails on any other one.
// ---------------------------------------------------------------------------

/// The Rayleigh density profile at `height_km` above the ground, relative to
/// its sea-level value.
///
/// `exp(−h/H)` through [`crate::fog::exp_neg`] rather than through a `libm`, so
/// the value is the same on every machine that builds a sky-view LUT.
#[must_use]
pub fn rayleigh_density(height_km: f32) -> f32 {
    exp_neg(height_km / RAYLEIGH_SCALE_HEIGHT_KM)
}

/// The Mie density profile at `height_km`, relative to its sea-level value.
#[must_use]
pub fn mie_density(height_km: f32) -> f32 {
    exp_neg(height_km / MIE_SCALE_HEIGHT_KM)
}

/// The ozone density profile at `height_km`, relative to its peak.
///
/// Bruneton's tent: 1 at [`OZONE_PEAK_HEIGHT_KM`], linearly down to zero
/// [`OZONE_HALF_WIDTH_KM`] either side, and clamped to zero beyond. It has no
/// exponential in it at all, so it is the same function in both precisions.
#[must_use]
pub fn ozone_density(height_km: f32) -> f32 {
    (1.0 - (height_km - OZONE_PEAK_HEIGHT_KM).abs() / OZONE_HALF_WIDTH_KM).clamp(0.0, 1.0)
}

/// Total extinction at `height_km`, per kilometre, per channel.
#[must_use]
pub fn extinction(height_km: f32) -> [f32; 3] {
    let rayleigh = rayleigh_density(height_km);
    let mie = mie_density(height_km);
    let ozone = ozone_density(height_km);
    let mut out = [0.0f32; 3];
    for (channel, slot) in out.iter_mut().enumerate() {
        *slot = RAYLEIGH_SCATTERING_PER_KM[channel] * rayleigh
            + MIE_EXTINCTION_PER_KM * mie
            + OZONE_ABSORPTION_PER_KM[channel] * ozone;
    }
    out
}

/// The Rayleigh phase function at a scattering cosine of `cos_theta`.
///
/// `3(1 + cos²θ) / 16π`, which is the exact dipole result rather than a fit,
/// and is multiplies and adds.
#[must_use]
pub fn rayleigh_phase(cos_theta: f32) -> f32 {
    3.0 / (16.0 * core::f32::consts::PI) * (1.0 + cos_theta * cos_theta)
}

/// The Henyey-Greenstein phase function at a scattering cosine of `cos_theta`,
/// with asymmetry [`MIE_PHASE_G`].
///
/// `(1 − g²) / (4π (1 + g² − 2g cosθ)^{3/2})`. The three-halves power is
/// `d · sqrt(d)`, so this needs no `pow`: the one place an atmosphere would
/// normally reach for one is the one place it does not have to.
///
/// `cos_theta` is the cosine between the view direction and the direction
/// **towards** the sun, which is the cosine of the deflection a photon
/// travelling from the sun into the eye underwent — so a forward-scattering
/// `g` peaks when the eye looks at the sun.
#[must_use]
pub fn mie_phase(cos_theta: f32) -> f32 {
    let g = MIE_PHASE_G;
    let numerator = 1.0 - g * g;
    let denominator = (1.0 + g * g - 2.0 * g * cos_theta).max(1.0e-6);
    numerator / (4.0 * core::f32::consts::PI * denominator * denominator.sqrt())
}

/// The distance from a point `radius_km` from the planet's centre, along a
/// direction whose cosine with the local up is `cos_zenith`, to where it leaves
/// a sphere of `sphere_radius_km`.
///
/// The far root of the quadratic, and negative discriminants clamp to zero
/// rather than producing a `NaN`. A caller inside the sphere always gets a
/// non-negative answer.
#[must_use]
pub fn distance_to_sphere(radius_km: f32, cos_zenith: f32, sphere_radius_km: f32) -> f32 {
    let discriminant = radius_km * radius_km * (cos_zenith * cos_zenith - 1.0)
        + sphere_radius_km * sphere_radius_km;
    (-radius_km * cos_zenith + discriminant.max(0.0).sqrt()).max(0.0)
}

/// Whether a ray leaving `radius_km` at `cos_zenith` meets the planet before it
/// leaves the atmosphere.
///
/// The near root has to exist *and* be in front of the ray, which is what the
/// two conditions are: a downward direction, and a discriminant that reaches
/// the ground.
#[must_use]
pub fn meets_the_ground(radius_km: f32, cos_zenith: f32) -> bool {
    let discriminant = radius_km * radius_km * (cos_zenith * cos_zenith - 1.0)
        + GROUND_RADIUS_KM * GROUND_RADIUS_KM;
    cos_zenith < 0.0 && discriminant >= 0.0
}

/// The distance from `radius_km` at `cos_zenith` to where the view ray ends —
/// the ground if it meets it, the top of the atmosphere otherwise.
#[must_use]
pub fn distance_to_end(radius_km: f32, cos_zenith: f32) -> f32 {
    let top = distance_to_sphere(radius_km, cos_zenith, TOP_RADIUS_KM);
    if !meets_the_ground(radius_km, cos_zenith) {
        return top;
    }
    let discriminant = radius_km * radius_km * (cos_zenith * cos_zenith - 1.0)
        + GROUND_RADIUS_KM * GROUND_RADIUS_KM;
    let near = -radius_km * cos_zenith - discriminant.max(0.0).sqrt();
    // `>=`, not `>`: a viewpoint exactly on the surface looking down has a near
    // root of exactly zero, and reading that as "no hit" marches the ray
    // through the planet and hands the lower hemisphere a full atmosphere's
    // glow. `the_ground_is_black_from_the_surface` is what holds that shut.
    if near >= 0.0 { near.min(top) } else { top }
}

/// The `(u, v)` the transmittance LUT holds `(radius_km, cos_zenith)` at.
///
/// **Bruneton's mapping**, which is the one Hillaire's listing uses: `v` is the
/// altitude as a share of the horizontal distance to the atmosphere's edge, and
/// `u` is the distance the ray travels to that edge as a share of the range
/// that distance can take at this altitude. It is square roots and divisions,
/// and it is what puts most of the table's resolution near the horizon, where
/// transmittance changes fastest.
#[must_use]
pub fn transmittance_uv(radius_km: f32, cos_zenith: f32) -> [f32; 2] {
    let horizon = (TOP_RADIUS_KM * TOP_RADIUS_KM - GROUND_RADIUS_KM * GROUND_RADIUS_KM).sqrt();
    let rho = (radius_km * radius_km - GROUND_RADIUS_KM * GROUND_RADIUS_KM)
        .max(0.0)
        .sqrt();
    let distance = distance_to_sphere(radius_km, cos_zenith, TOP_RADIUS_KM);
    let shortest = (TOP_RADIUS_KM - radius_km).max(0.0);
    let longest = rho + horizon;
    let span = (longest - shortest).max(1.0e-6);
    [
        ((distance - shortest) / span).clamp(0.0, 1.0),
        rho / horizon,
    ]
}

/// [`transmittance_uv`]'s inverse: the `(radius_km, cos_zenith)` a texel at
/// `(u, v)` stands for.
///
/// The cook is what calls this, and it is here rather than in the tool because
/// a mapping written in two places is a mapping that drifts —
/// `the_transmittance_mapping_round_trips` holds the pair together.
#[must_use]
pub fn transmittance_params(u: f32, v: f32) -> (f32, f32) {
    let horizon = (TOP_RADIUS_KM * TOP_RADIUS_KM - GROUND_RADIUS_KM * GROUND_RADIUS_KM).sqrt();
    let rho = horizon * v.clamp(0.0, 1.0);
    let radius = (rho * rho + GROUND_RADIUS_KM * GROUND_RADIUS_KM).sqrt();
    let shortest = TOP_RADIUS_KM - radius;
    let longest = rho + horizon;
    let distance = shortest + u.clamp(0.0, 1.0) * (longest - shortest);
    let cosine = if distance <= 0.0 {
        1.0
    } else {
        ((horizon * horizon - rho * rho - distance * distance) / (2.0 * radius * distance))
            .clamp(-1.0, 1.0)
    };
    (radius, cosine)
}

/// One axis of a clamped bilinear read: the two texels and the weight between
/// them, for `value` in `[0, 1]` over `texels` texels sampled at their centres.
///
/// [`crate::dfg::sample`]'s addressing exactly, lifted out because two tables
/// of different sizes want it.
fn axis_taps(value: f32, texels: usize) -> (usize, usize, f32) {
    let scaled = value.clamp(0.0, 1.0) * texels as f32 - 0.5;
    let low = scaled.floor().clamp(0.0, (texels - 1) as f32);
    let high = (low + 1.0).min((texels - 1) as f32);
    (low as usize, high as usize, (scaled - low).clamp(0.0, 1.0))
}

/// The transmittance from `radius_km` along `cos_zenith` out to space, read
/// bilinearly from the committed table.
#[must_use]
pub fn sample_transmittance(radius_km: f32, cos_zenith: f32) -> [f32; 3] {
    let [u, v] = transmittance_uv(radius_km, cos_zenith);
    let (x0, x1, fx) = axis_taps(u, TRANSMITTANCE_WIDTH);
    let (y0, y1, fy) = axis_taps(v, TRANSMITTANCE_HEIGHT);
    let mut out = [0.0f32; 3];
    for (channel, slot) in out.iter_mut().enumerate() {
        let top = transmittance_entry(x0, y0)[channel] * (1.0 - fx)
            + transmittance_entry(x1, y0)[channel] * fx;
        let bottom = transmittance_entry(x0, y1)[channel] * (1.0 - fx)
            + transmittance_entry(x1, y1)[channel] * fx;
        *slot = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// The `(u, v)` the multiple-scattering LUT holds `(radius_km, cos_sun_zenith)`
/// at.
///
/// Both axes are linear, where the transmittance LUT's are not: the quantity
/// stored here is already an integral over the whole sphere, so it has no
/// horizon feature to spend resolution on.
#[must_use]
pub fn multiscatter_uv(radius_km: f32, cos_sun_zenith: f32) -> [f32; 2] {
    let altitude = (radius_km - GROUND_RADIUS_KM) / (TOP_RADIUS_KM - GROUND_RADIUS_KM);
    [
        (cos_sun_zenith * 0.5 + 0.5).clamp(0.0, 1.0),
        altitude.clamp(0.0, 1.0),
    ]
}

/// The second-and-later scattering orders reaching a point at `radius_km` under
/// a sun at `cos_sun_zenith`, read bilinearly from the committed table.
///
/// Isotropic, which is Hillaire's §4 approximation: past the first bounce the
/// light has forgotten which way it came, so it needs no phase function and no
/// direction — one value per channel per point.
#[must_use]
pub fn sample_multiscatter(radius_km: f32, cos_sun_zenith: f32) -> [f32; 3] {
    let [u, v] = multiscatter_uv(radius_km, cos_sun_zenith);
    let (x0, x1, fx) = axis_taps(u, MULTISCATTER_SIZE);
    let (y0, y1, fy) = axis_taps(v, MULTISCATTER_SIZE);
    let mut out = [0.0f32; 3];
    for (channel, slot) in out.iter_mut().enumerate() {
        let top = multiscatter_entry(x0, y0)[channel] * (1.0 - fx)
            + multiscatter_entry(x1, y0)[channel] * fx;
        let bottom = multiscatter_entry(x0, y1)[channel] * (1.0 - fx)
            + multiscatter_entry(x1, y1)[channel] * fx;
        *slot = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// Texels across the sky-view LUT — the azimuth away from the sun, through
/// [`sky_view_cosine_of`].
///
/// Hillaire's is 192×108 and is built by the GPU every frame; this one is built
/// by the CPU when the sun moves, so it is smaller. It covers **half** the
/// azimuth range his does, because the field is symmetric about the sun's
/// meridian and this parameterisation exploits that — so the angular resolution
/// near the sun is finer than the texel count alone suggests.
/// `SkyView::build`'s doc carries the measured build cost at this size.
pub const SKY_VIEW_WIDTH: usize = 96;

/// Texels down the sky-view LUT — the direction's `y`, through
/// [`sky_view_up_of`]. See [`SKY_VIEW_WIDTH`].
pub const SKY_VIEW_HEIGHT: usize = 64;

/// Where in a slice its one sample is taken, as a share of the slice.
///
/// **The midpoint, and not Hillaire's 0.3.** His listing offsets the sample
/// towards the front of the slice. Swept here at four segment values against a
/// 768-step reference, along the ray this march converges slowest on — straight
/// up from the ground under an overhead sun — the offset costs accuracy rather
/// than buying it, and costs more of it the coarser the march: the midpoint
/// rule is second order over a slice whose transmittance is already integrated
/// in closed form, and an offset one is not. At the shipped step count the 0.3
/// sample read several per cent above the reference where the midpoint reads
/// under one per cent below it, which is the figure
/// `the_march_has_converged_at_the_shipped_step_count` prints and asserts.
pub const SAMPLE_SEGMENT: f32 = 0.5;

/// Steps [`SkyView::build`] marches along each view ray.
///
/// Hillaire's fast path takes 30 and his reference 64. **Swept rather than
/// copied**: `the_march_has_converged_at_the_shipped_step_count` runs five step
/// counts against a far finer march over four rays chosen to bracket this
/// integrand's difficulty — straight up under an overhead sun, ten degrees up
/// under the same, and the horizon towards and away from a sun ten degrees up —
/// prints the whole row and asserts on this one. The spread is almost entirely
/// the zenith ray, where a hundred kilometres of path sits over an
/// eight-kilometre scale height; this is the first count whose worst channel is
/// inside a per cent, and doubling it doubles a build that already costs tens
/// of milliseconds.
pub const SKY_VIEW_STEPS: usize = 32;

/// The direction's `y` a sky-view LUT row at `v` stands for.
///
/// `sign(s)·s²` with `s = 2v − 1`: the horizon sits at `v = ½` and the map is
/// flat there, so texel rows crowd towards the horizon — which is where a sky
/// changes fastest — and spread out towards the poles. Multiplies and an
/// absolute value.
#[must_use]
pub fn sky_view_up_of(v: f32) -> f32 {
    let signed = 2.0 * v.clamp(0.0, 1.0) - 1.0;
    signed * signed.abs()
}

/// [`sky_view_up_of`]'s inverse: the row a direction's `y` is read from.
#[must_use]
pub fn sky_view_v_of(up: f32) -> f32 {
    let clamped = up.clamp(-1.0, 1.0);
    let root = clamped.abs().sqrt();
    0.5 + 0.5 * if clamped >= 0.0 { root } else { -root }
}

/// The cosine of the azimuth away from the sun that a sky-view LUT column at
/// `u` stands for.
///
/// `1 − 2u²`, so column 0 looks straight at the sun's azimuth and the last
/// column straight away from it. Near the sun `u` is proportional to the angle
/// itself — the aureole gets uniform angular resolution — and the spacing
/// coarsens towards the anti-solar side, where the field is smooth.
#[must_use]
pub fn sky_view_cosine_of(u: f32) -> f32 {
    let clamped = u.clamp(0.0, 1.0);
    1.0 - 2.0 * clamped * clamped
}

/// [`sky_view_cosine_of`]'s inverse: the column an azimuth cosine is read from.
#[must_use]
pub fn sky_view_u_of(cosine: f32) -> f32 {
    ((1.0 - cosine.clamp(-1.0, 1.0)) * 0.5).max(0.0).sqrt()
}

/// The sun and the viewpoint a sky-view LUT is built for.
///
/// Everything else about the sky is the planet, and the planet is the committed
/// tables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// The unit direction **towards** the sun, in the engine's world axes —
    /// `+Y` up.
    ///
    /// The same convention `crcbl_render::DirectionalLight` states its
    /// direction in, so a scene points one vector at the sun and both the
    /// shading and the sky follow it.
    pub sun_direction: [f32; 3],
    /// The sun's illuminance on a surface facing it, in linear RGB, above the
    /// atmosphere.
    ///
    /// Multiplied into the LUT as it is built rather than at sample time: the
    /// scattering integral is linear in it, the sun's colour changes far less
    /// often than this LUT is rebuilt anyway, and folding it in keeps the
    /// device's read to one fetch with no second uniform to agree about.
    pub sun_illuminance: [f32; 3],
    /// The viewpoint's height above the ground, in kilometres.
    ///
    /// A scene at sea level passes zero. The LUT is built for one height, which
    /// is Hillaire's own approximation: a frame's camera moves far less than
    /// the atmosphere's scale height, so one LUT serves the whole frame.
    pub altitude_km: f32,
}

impl Atmosphere {
    /// A midday sun overhead and a little south, at sea level, normalised so
    /// the sun's illuminance is one in every channel.
    ///
    /// The fixture the tests and the render harness build from. A caller who
    /// wants a physical exposure scales [`Self::sun_illuminance`]; the sky's
    /// radiance is linear in it.
    pub const NOON: Self = Self {
        sun_direction: [0.0, 1.0, 0.0],
        sun_illuminance: [1.0, 1.0, 1.0],
        altitude_km: 0.0,
    };

    /// This atmosphere's viewpoint, in kilometres from the planet's centre.
    #[must_use]
    pub fn view_radius_km(&self) -> f32 {
        (GROUND_RADIUS_KM + self.altitude_km).clamp(GROUND_RADIUS_KM, TOP_RADIUS_KM)
    }
}

/// The sky-view LUT: the scattered radiance of the whole sky for one sun, in
/// linear RGB.
///
/// Built by [`Self::build`] on the host, uploaded by `crcbl_render::sky_pass`
/// as the `float4` storage buffer [`Self::rows`] lays out, projected to an L1
/// probe by [`Self::irradiance`], and read by `shaders/sky.slang` with one
/// bilinear tap.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyView {
    /// `[row * SKY_VIEW_WIDTH + column]`, row-major with the direction's `y`
    /// slow — the order [`Self::rows`] uploads and [`Self::entry`] indexes.
    radiance: Vec<[f32; 3]>,
    /// The sun this was built for, kept so [`Self::radiance`] can turn a world
    /// direction into the LUT's sun-relative coordinates without the caller
    /// having to hold the pair together.
    sun_direction: [f32; 3],
}

impl SkyView {
    /// Marches the atmosphere once per texel and stores what each view ray
    /// sees.
    ///
    /// Hillaire's sky-view pass, on the host and on this module's grid. Each
    /// texel is a direction; the march accumulates single scattering weighted
    /// by the two phase functions and the sun's transmittance, plus the
    /// isotropic multiple-scattering term, integrating each step with the
    /// closed form [`crate::fog::one_minus_exp_over`] carries rather than a
    /// midpoint sample.
    ///
    /// **The arithmetic is restricted to what IEEE-754 pins down** — the module
    /// header says why, and `the_runtime_path_calls_no_transcendental` is what
    /// enforces it. Two builds from the same [`Atmosphere`] produce the same
    /// bytes on every machine, which
    /// `the_lut_is_bit_identical_across_two_builds` asserts.
    ///
    /// **The whole LUT in one call**, which is what a caller that has no frame
    /// to fit it into wants. A caller that does — the renderer — steps a
    /// [`SkyViewBuild`] instead and gets the same bytes out of it.
    #[must_use]
    pub fn build(atmosphere: &Atmosphere) -> Self {
        let mut build = SkyViewBuild::start(atmosphere);
        build.step(SKY_VIEW_HEIGHT);
        build.finish()
    }

    /// The stored radiance at `(column, row)`.
    ///
    /// # Panics
    ///
    /// If either index is outside the LUT.
    #[must_use]
    pub fn entry(&self, column: usize, row: usize) -> [f32; 3] {
        assert!(
            column < SKY_VIEW_WIDTH && row < SKY_VIEW_HEIGHT,
            "({column}, {row}) is outside a {SKY_VIEW_WIDTH}x{SKY_VIEW_HEIGHT} LUT"
        );
        self.radiance[row * SKY_VIEW_WIDTH + column]
    }

    /// The sun this LUT was built for.
    #[must_use]
    pub fn sun_direction(&self) -> [f32; 3] {
        self.sun_direction
    }

    /// The LUT read bilinearly at an azimuth cosine and a direction's `y`, the
    /// way the shader's linear filter reads it, clamped at every edge.
    #[must_use]
    pub fn sample(&self, up: f32, azimuth_cosine: f32) -> [f32; 3] {
        let (x0, x1, fx) = axis_taps(sky_view_u_of(azimuth_cosine), SKY_VIEW_WIDTH);
        let (y0, y1, fy) = axis_taps(sky_view_v_of(up), SKY_VIEW_HEIGHT);
        let mut out = [0.0f32; 3];
        for (channel, slot) in out.iter_mut().enumerate() {
            let top = self.entry(x0, y0)[channel] * (1.0 - fx) + self.entry(x1, y0)[channel] * fx;
            let bottom =
                self.entry(x0, y1)[channel] * (1.0 - fx) + self.entry(x1, y1)[channel] * fx;
            *slot = top * (1.0 - fy) + bottom * fy;
        }
        out
    }

    /// The radiance a ray leaving the scene along `direction` sees, in linear
    /// RGB.
    ///
    /// `direction` should be unit length. This is [`crate::sky::SkyGradient::radiance`]'s
    /// place in the atmosphere arm, and the arithmetic `sky.slang`'s
    /// `atmosphere_radiance` spells: the direction's `y` picks the row, and the
    /// cosine between the two horizontal projections — the view's and the
    /// sun's — picks the column. A sun straight overhead leaves that cosine
    /// undefined, and the sky is azimuthally symmetric exactly then, so the
    /// column falls back to the one facing the sun's azimuth.
    #[must_use]
    pub fn radiance(&self, direction: [f32; 3]) -> [f32; 3] {
        let view_flat = (direction[0] * direction[0] + direction[2] * direction[2]).sqrt();
        let sun_flat = (self.sun_direction[0] * self.sun_direction[0]
            + self.sun_direction[2] * self.sun_direction[2])
            .sqrt();
        let cosine = if view_flat > 0.0 && sun_flat > 0.0 {
            (direction[0] * self.sun_direction[0] + direction[2] * self.sun_direction[2])
                / (view_flat * sun_flat)
        } else {
            1.0
        };
        self.sample(direction[1], cosine)
    }

    /// This sky projected onto the L1 irradiance basis, ready to be added to
    /// whatever a probe volume contributes.
    ///
    /// [`crate::sky::SkyGradient::irradiance`]'s place in the atmosphere arm,
    /// and it fills the same [`GpuProbe`] record `mesh.slang` already unpacks —
    /// so a scene that swaps a gradient for an atmosphere changes the numbers
    /// in `frame.sky_sh_*` and nothing about how they are read.
    ///
    /// A gradient's projection is closed form because a gradient is a cubic in
    /// one variable. This field is a marched integral, so this is a
    /// **quadrature** — but a quadrature with no transcendental in it:
    ///
    /// * the vertical axis is [`IRRADIANCE_BANDS`] uniform bands of the
    ///   direction's `y`, whose element of solid angle is exactly `dφ dy`;
    /// * the azimuth is [`IRRADIANCE_AZIMUTHS`] nodes of the tangent
    ///   half-angle, `cos φ = ((1−q)² − q²) / ((1−q)² + q²)` and
    ///   `sin φ = 2q(1−q) / ((1−q)² + q²)`, whose `dφ` is
    ///   `2 dq / ((1−q)² + q²)` — a rational parameterisation of the half
    ///   circle, so the nodes and their weights are divisions and nothing more.
    ///
    /// Each node is emitted twice, mirrored across the sun's meridian, which is
    /// where the LUT's own symmetry is spent: the two samples' components
    /// perpendicular to that plane cancel in the linear band exactly rather
    /// than approximately.
    #[must_use]
    pub fn irradiance(&self) -> GpuProbe {
        // The sun's azimuth as a horizontal unit vector, and the horizontal
        // perpendicular to it. A sun straight overhead leaves the first
        // undefined and the sky azimuthally symmetric, so any axis will do.
        let flat = (self.sun_direction[0] * self.sun_direction[0]
            + self.sun_direction[2] * self.sun_direction[2])
            .sqrt();
        let towards = if flat > 0.0 {
            [self.sun_direction[0] / flat, self.sun_direction[2] / flat]
        } else {
            [1.0, 0.0]
        };
        let across = [towards[1], -towards[0]];

        let band_step = 2.0 / IRRADIANCE_BANDS as f32;
        let node_step = 1.0 / IRRADIANCE_AZIMUTHS as f32;
        let mut probe = GpuProbe::ZERO;
        for band in 0..IRRADIANCE_BANDS {
            let up = -1.0 + (band as f32 + 0.5) * band_step;
            let side = (1.0 - up * up).max(0.0).sqrt();
            for node in 0..IRRADIANCE_AZIMUTHS {
                let q = (node as f32 + 0.5) * node_step;
                let one_minus = 1.0 - q;
                let sum = one_minus * one_minus + q * q;
                let cosine = (one_minus * one_minus - q * q) / sum;
                let sine = 2.0 * q * one_minus / sum;
                // `dφ` for this node, from the same rational parameterisation.
                let azimuth_step = 2.0 * node_step / sum;
                let solid_angle = band_step * azimuth_step;
                let radiance = self.sample(up, cosine);
                let along = side * cosine;
                let off = side * sine;
                for sign in [1.0f32, -1.0] {
                    let direction = [
                        along * towards[0] + sign * off * across[0],
                        up,
                        along * towards[1] + sign * off * across[1],
                    ];
                    probe.accumulate(direction, radiance, solid_angle);
                }
            }
        }
        probe
    }

    /// The LUT as the bytes `shaders/sky.slang` reads it out of: one `float4`
    /// per texel, row-major with the direction's `y` slow — [`Self::entry`]'s
    /// order.
    ///
    /// **A storage buffer rather than a sampled image**, which is what lets a
    /// sun that moved reach the device through a mapped write instead of an
    /// image upload and the device idle an image upload takes. It also settles
    /// the filter the way every other table in this crate settles it: the
    /// shader spells the bilinear blend out over four loads, because a
    /// hardware filter's weights are fixed-function arithmetic four rasterisers
    /// compute independently and these goldens are compared across all four.
    ///
    /// `f32` and not the half floats [`crate::ltc::texels`] writes, for the
    /// same reason those are half: the constraint there was an image format,
    /// and a buffer has none. The fourth lane is one and no shader reads it —
    /// it is the `float4` a `StructuredBuffer` wants rather than a value.
    #[must_use]
    pub fn rows(&self) -> Vec<u8> {
        let mut rows = Vec::with_capacity(SKY_VIEW_BUFFER_BYTES);
        for entry in &self.radiance {
            for channel in entry {
                rows.extend_from_slice(&channel.to_le_bytes());
            }
            rows.extend_from_slice(&1.0f32.to_le_bytes());
        }
        rows
    }

    /// The three bands of the [`crate::sky::SkyGradient`] that stands in for
    /// this sky where a consumer takes a gradient and cannot take the LUT.
    ///
    /// **`ssr.slang` is that consumer**, and this is the least that keeps the
    /// reflection pass and the drawn sky agreeing — `docs/plan/43-render-standards.md`
    /// §8 states the decision and `docs/backlog.md` carries what it leaves. The
    /// march's own poles are the two polar bands, and the horizon band is the
    /// LUT's azimuthal mean at `y = 0`, so a mirror pointed at the zenith, the
    /// horizon or straight down reflects what the background shows there and a
    /// rougher lobe blends between the three the way
    /// [`crate::sky_prefilter`] already does.
    ///
    /// What it cannot carry is the aureole: a reflection of the sky *beside*
    /// the sun reads the azimuthal mean rather than the bright band, because a
    /// gradient has no azimuth in it at all.
    #[must_use]
    pub fn gradient_fit(&self) -> crate::sky::SkyGradient {
        // The poles are one direction each — `side` is zero there, so every
        // column of the top and bottom rows marched the same ray.
        let zenith = self.sample(1.0, 1.0);
        let ground = self.sample(-1.0, 1.0);
        let mut horizon = [0.0f32; 3];
        for column in 0..SKY_VIEW_WIDTH {
            let entry = self.sample(0.0, sky_view_cosine_of(axis_value(column, SKY_VIEW_WIDTH)));
            for (channel, slot) in horizon.iter_mut().enumerate() {
                *slot += entry[channel];
            }
        }
        for slot in &mut horizon {
            *slot /= SKY_VIEW_WIDTH as f32;
        }
        crate::sky::SkyGradient {
            zenith,
            horizon,
            ground,
        }
    }
}

/// Rows [`SkyViewBuild::step`] marches per call.
///
/// **The renderer's frame budget is what picks it**, so it is a fraction of
/// [`SKY_VIEW_HEIGHT`] rather than a round number: one step costs that share of
/// [`SkyView::build`]'s whole march, and `crcbl_render::forward` takes one step
/// per frame while the sun is moving. It divides [`SKY_VIEW_HEIGHT`] — so a
/// build is a whole number of steps and no step is a short one — which the
/// assertion below holds it to, and
/// `the_amortised_step_is_a_fraction_of_the_whole_build` prints what the two
/// actually cost.
pub const SKY_VIEW_BUILD_ROWS: usize = 4;

/// [`SKY_VIEW_BUILD_ROWS`] is a proper divisor of [`SKY_VIEW_HEIGHT`].
///
/// Its doc says a build is a whole number of steps and that one step is a
/// fraction of the whole march. A stripe that divided nothing would make the
/// first claim false; one as tall as the LUT amortises nothing and would make
/// the second false. Neither is a runtime condition, so neither is a test.
const _: () = assert!(
    SKY_VIEW_BUILD_ROWS > 0
        && SKY_VIEW_BUILD_ROWS < SKY_VIEW_HEIGHT
        && SKY_VIEW_HEIGHT.is_multiple_of(SKY_VIEW_BUILD_ROWS),
    "SKY_VIEW_BUILD_ROWS must be a proper divisor of SKY_VIEW_HEIGHT"
);

/// A sky-view LUT part way through its march.
///
/// [`SkyView::build`] is this stepped straight to the end. A caller with a
/// frame to fit the march into steps it [`SKY_VIEW_BUILD_ROWS`] rows at a time
/// instead and keeps showing the LUT it last finished, which is what
/// `crcbl_render::forward` does with a sun that moves.
///
/// **Rows are independent**: the loop body reads the sun, the viewpoint and the
/// committed tables, and nothing another row wrote. That is what makes a march
/// stopped between two rows and resumed produce the same bytes as one that ran
/// straight through — `a_striped_build_is_the_one_shot_build` asserts it at
/// three stripe widths, one of them not a divisor.
#[derive(Clone, Debug)]
pub struct SkyViewBuild {
    /// The sun and viewpoint this march was started from.
    ///
    /// Kept so a caller stepping one across frames can tell a build of the sun
    /// it wants now from a build of a sun that has since moved.
    atmosphere: Atmosphere,
    /// The sun's horizontal component in the LUT's own frame, from
    /// [`Self::start`].
    ///
    /// **Derived once for the whole march** rather than per step. It is the
    /// same for every row, so deriving it again where a stripe resumes would be
    /// a second copy of [`Self::start`]'s arithmetic that nothing holds to
    /// agreeing with the first — which is exactly the shape of bug a march that
    /// stops and resumes is exposed to.
    sun_side: f32,
    /// The sun's vertical component, on [`Self::sun_side`]'s terms.
    sun_up: f32,
    /// The viewpoint's radius from the planet's centre, on [`Self::sun_side`]'s
    /// terms.
    view_radius: f32,
    /// The rows marched so far, in [`SkyView::radiance`]'s order.
    ///
    /// Its length is [`Self::rows_done`] times [`SKY_VIEW_WIDTH`], which is why
    /// there is no second counter to disagree with it.
    radiance: Vec<[f32; 3]>,
}

impl SkyViewBuild {
    /// Starts a march of `atmosphere`'s sky with no row marched yet.
    #[must_use]
    pub fn start(atmosphere: &Atmosphere) -> Self {
        let sun_up = atmosphere.sun_direction[1].clamp(-1.0, 1.0);
        // The sun in the LUT's own frame, where the sun's azimuth is the `+x`
        // axis: horizontal component first, then up. The frame is only ever
        // used through the two cosines the march takes, so it needs no basis
        // vectors.
        let sun_side = (1.0 - sun_up * sun_up).max(0.0).sqrt();
        Self {
            atmosphere: *atmosphere,
            sun_side,
            sun_up,
            view_radius: atmosphere.view_radius_km(),
            radiance: Vec::with_capacity(SKY_VIEW_WIDTH * SKY_VIEW_HEIGHT),
        }
    }

    /// The sun and viewpoint this march was started from.
    #[must_use]
    pub const fn atmosphere(&self) -> Atmosphere {
        self.atmosphere
    }

    /// How many rows of the LUT are marched.
    #[must_use]
    pub fn rows_done(&self) -> usize {
        self.radiance.len() / SKY_VIEW_WIDTH
    }

    /// Whether every row is marched, so [`Self::finish`] will not panic.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.rows_done() == SKY_VIEW_HEIGHT
    }

    /// Marches the next `rows` rows — or what is left of the LUT, if that is
    /// fewer — and returns [`Self::is_complete`].
    ///
    /// A `rows` past the end is clamped rather than refused, so a caller may
    /// step a build it does not know the progress of and a last short stripe
    /// needs no arithmetic at the call site.
    pub fn step(&mut self, rows: usize) -> bool {
        let from = self.rows_done();
        let until = from.saturating_add(rows).min(SKY_VIEW_HEIGHT);
        for row in from..until {
            let up = sky_view_up_of(axis_value(row, SKY_VIEW_HEIGHT));
            let side = (1.0 - up * up).max(0.0).sqrt();
            for column in 0..SKY_VIEW_WIDTH {
                let cosine = sky_view_cosine_of(axis_value(column, SKY_VIEW_WIDTH));
                let across = side * (1.0 - cosine * cosine).max(0.0).sqrt();
                let direction = [side * cosine, up, across];
                // The scattering cosine: the view direction against the sun,
                // both in the LUT's frame where the sun has no `z`.
                let sun_cosine = direction[0] * self.sun_side + up * self.sun_up;
                let scattered = march(
                    self.view_radius,
                    direction,
                    self.sun_side,
                    self.sun_up,
                    sun_cosine,
                    SKY_VIEW_STEPS,
                );
                let mut lit = [0.0f32; 3];
                for (channel, slot) in lit.iter_mut().enumerate() {
                    *slot = scattered[channel] * self.atmosphere.sun_illuminance[channel];
                }
                self.radiance.push(lit);
            }
        }
        self.is_complete()
    }

    /// The finished LUT.
    ///
    /// # Panics
    ///
    /// If any row is still unmarched. A LUT short of rows is not a stale sky
    /// but a broken one — [`SkyView::rows`] would encode fewer bytes than
    /// [`SKY_VIEW_BUFFER_BYTES`] and the buffer's tail would keep whatever was
    /// there — so this is loud rather than padded: a caller steps until
    /// [`Self::step`] says the march is done.
    #[must_use]
    pub fn finish(self) -> SkyView {
        assert!(
            self.is_complete(),
            "{} of {SKY_VIEW_HEIGHT} rows are marched, so this LUT is not a sky yet",
            self.rows_done()
        );
        SkyView {
            radiance: self.radiance,
            sun_direction: self.atmosphere.sun_direction,
        }
    }
}

/// Bands of the direction's `y` [`SkyView::irradiance`] integrates over.
///
/// Twice the LUT's own row count, so no band straddles more than half a texel.
/// `a_uniform_lut_projects_to_pi_times_its_radiance` bounds what this pair
/// costs on a field with no feature in it, and
/// `the_l1_projection_matches_a_brute_force_integral` bounds it on the real
/// one; the whole projection is `IRRADIANCE_BANDS × IRRADIANCE_AZIMUTHS`
/// bilinear reads, which is thousandths of the march that filled the LUT.
pub const IRRADIANCE_BANDS: usize = 128;

/// Azimuth nodes per band [`SkyView::irradiance`] integrates over, covering
/// half the circle; each is emitted mirrored, so the whole circle is 2× this.
///
/// Two thirds of the LUT's column count is enough because the columns crowd
/// towards the sun and these nodes crowd the same way — the tangent half-angle
/// spends its density where `sky_view_cosine_of` spends its texels.
pub const IRRADIANCE_AZIMUTHS: usize = 64;

/// Bytes one texel of [`SkyView::rows`] occupies: a `float4`.
pub const SKY_VIEW_ROW_BYTES: usize = 16;

/// The length of [`SkyView::rows`], and the size of the buffer
/// `crcbl_render::sky_pass` binds.
pub const SKY_VIEW_BUFFER_BYTES: usize = SKY_VIEW_WIDTH * SKY_VIEW_HEIGHT * SKY_VIEW_ROW_BYTES;

/// One view ray's scattered radiance, for a sun of unit illuminance.
///
/// `radius_km` is where the ray starts, `direction` is where it points in the
/// LUT's frame — the sun's azimuth along `+x`, up along `+y` — and `sun_side`
/// and `sun_up` are that same sun's horizontal and vertical components.
/// `sun_cosine` is the scattering cosine between the two, which the caller has
/// already formed, and `steps` is how finely the ray is cut — [`SKY_VIEW_STEPS`]
/// everywhere but in the test that measures what a finer march would move.
fn march(
    radius_km: f32,
    direction: [f32; 3],
    sun_side: f32,
    sun_up: f32,
    sun_cosine: f32,
    steps: usize,
) -> [f32; 3] {
    let up = direction[1];
    let end = distance_to_end(radius_km, up);
    let rayleigh = rayleigh_phase(sun_cosine);
    let mie = mie_phase(sun_cosine);

    let mut transmittance = [1.0f32; 3];
    let mut radiance = [0.0f32; 3];
    for slice in 0..steps {
        // **Slices grow quadratically along the ray**, which is Hillaire's own
        // distribution and not a refinement of it. The medium's density falls
        // off over `RAYLEIGH_SCALE_HEIGHT_KM` while a ray to the zenith is a
        // hundred kilometres long, so a uniform march spends most of its
        // samples where there is nothing to sample: at this step count it
        // reported a third less light overhead than the converged value, which
        // is what `the_march_has_converged_at_the_shipped_step_count`
        // measures now that the slices are placed this way.
        let near = slice as f32 / steps as f32;
        let far = (slice + 1) as f32 / steps as f32;
        let from = near * near * end;
        let step = far * far * end - from;
        let distance = from + step * SAMPLE_SEGMENT;
        // The sample's distance from the planet's centre, by the cosine rule.
        let sample_radius =
            (radius_km * radius_km + distance * distance + 2.0 * radius_km * distance * up)
                .max(GROUND_RADIUS_KM * GROUND_RADIUS_KM)
                .sqrt();
        let height = sample_radius - GROUND_RADIUS_KM;

        let rayleigh_density = rayleigh_density(height);
        let mie_density = mie_density(height);
        let ozone_density = ozone_density(height);
        let mie_scattering = MIE_SCATTERING_PER_KM * mie_density;

        // The sun's zenith cosine where this sample sits: the local up is the
        // sample's own position, and the sun has no `z` in this frame.
        let sun_zenith = (distance * direction[0] * sun_side
            + (radius_km + distance * up) * sun_up)
            / sample_radius;
        let sun_transmittance = if meets_the_ground(sample_radius, sun_zenith) {
            [0.0f32; 3]
        } else {
            sample_transmittance(sample_radius, sun_zenith)
        };
        let multiscatter = sample_multiscatter(sample_radius, sun_zenith);

        for channel in 0..3 {
            let rayleigh_scattering = RAYLEIGH_SCATTERING_PER_KM[channel] * rayleigh_density;
            let extinction = rayleigh_scattering
                + MIE_EXTINCTION_PER_KM * mie_density
                + OZONE_ABSORPTION_PER_KM[channel] * ozone_density;
            let scattered = (rayleigh_scattering * rayleigh + mie_scattering * mie)
                * sun_transmittance[channel]
                + (rayleigh_scattering + mie_scattering) * multiscatter[channel];
            // The slice's integral in closed form: `∫₀^step e^{-σt} dt` is
            // `step · (1 − e^{-σ step}) / (σ step)`, which is what
            // `one_minus_exp_over` is and what keeps a thin or empty slice
            // from dividing by nothing.
            let optical_depth = extinction * step;
            radiance[channel] +=
                transmittance[channel] * scattered * step * one_minus_exp_over(optical_depth);
            transmittance[channel] *= exp_neg(optical_depth);
        }
    }
    radiance
}

// ---------------------------------------------------------------------------
// RUNTIME PATH ENDS
//
// Below this marker is the cook, which may use whatever arithmetic it likes:
// its output is a committed artifact compared as bytes, not a colour computed
// four times. `crate::dfg`'s header carries the general form of that argument.
// ---------------------------------------------------------------------------

/// Steps [`transmittance_at`] marches.
///
/// **Bruneton's order of magnitude rather than Hillaire's.** Hillaire's
/// real-time listing takes 40 because it rebuilds this table on the device;
/// Bruneton's offline reference takes 500. This is a cook whose output is
/// committed, so the only thing a fine march costs is seconds on one machine
/// once — and every consumer of this table, the sky-view march and the
/// multiple-scattering cook alike, inherits whatever error it has.
/// `the_transmittance_integrator_has_converged` measures what eight times this
/// count would move.
pub const TRANSMITTANCE_STEPS: usize = 200;

/// Directions per texel the multiple-scattering cook integrates over.
///
/// Hillaire's 64, as an 8×8 stratification of the unit sphere.
pub const MULTISCATTER_DIRECTIONS: usize = 64;

/// Steps each of those directions is marched for.
///
/// Hillaire's listing takes 20 for this pass, half the transmittance LUT's,
/// because it is integrating an already-smooth quantity over 64 directions.
pub const MULTISCATTER_STEPS: usize = 20;

/// The density profiles in `f64`, for the cook.
///
/// The runtime pair above is `f32` and goes through
/// [`crate::fog::exp_neg`] because it reaches a colour. These do not: the cook
/// sums thousands of terms into one accumulator and rounds once, so it wants
/// the wider type and the platform's own `exp`.
/// `the_two_precisions_agree_about_the_densities` holds them together.
fn densities(height_km: f64) -> (f64, f64, f64) {
    let rayleigh = (-height_km / f64::from(RAYLEIGH_SCALE_HEIGHT_KM)).exp();
    let mie = (-height_km / f64::from(MIE_SCALE_HEIGHT_KM)).exp();
    let ozone = (1.0
        - (height_km - f64::from(OZONE_PEAK_HEIGHT_KM)).abs() / f64::from(OZONE_HALF_WIDTH_KM))
    .clamp(0.0, 1.0);
    (rayleigh, mie, ozone)
}

/// Extinction and the two scattering coefficients at `height_km`, in `f64`.
fn medium(height_km: f64) -> ([f64; 3], [f64; 3], f64) {
    let (rayleigh, mie, ozone) = densities(height_km);
    let mut extinction = [0.0f64; 3];
    let mut rayleigh_scattering = [0.0f64; 3];
    for channel in 0..3 {
        rayleigh_scattering[channel] = f64::from(RAYLEIGH_SCATTERING_PER_KM[channel]) * rayleigh;
        extinction[channel] = rayleigh_scattering[channel]
            + f64::from(MIE_EXTINCTION_PER_KM) * mie
            + f64::from(OZONE_ABSORPTION_PER_KM[channel]) * ozone;
    }
    (
        extinction,
        rayleigh_scattering,
        f64::from(MIE_SCATTERING_PER_KM) * mie,
    )
}

/// The distance from `radius_km` at `cos_zenith` to the top of the atmosphere,
/// in `f64`.
fn distance_to_top(radius_km: f64, cos_zenith: f64) -> f64 {
    let top = f64::from(TOP_RADIUS_KM);
    let discriminant = radius_km * radius_km * (cos_zenith * cos_zenith - 1.0) + top * top;
    (-radius_km * cos_zenith + discriminant.max(0.0).sqrt()).max(0.0)
}

/// Whether a ray leaving `radius_km` at `cos_zenith` meets the planet, in
/// `f64`.
fn hits_the_ground(radius_km: f64, cos_zenith: f64) -> bool {
    let ground = f64::from(GROUND_RADIUS_KM);
    let discriminant = radius_km * radius_km * (cos_zenith * cos_zenith - 1.0) + ground * ground;
    cos_zenith < 0.0 && discriminant >= 0.0
}

/// The transmittance from `radius_km` along `cos_zenith` out to space, marched
/// rather than read from the table.
///
/// **This is the definition the committed table is an interpolation of**, and
/// it is public because that is what makes the table checkable: a test compares
/// this against the closed form a vertical ray has, and `cook-atmosphere
/// --check` compares the table against this.
///
/// A ray that meets the planet transmits nothing, which is the convention
/// Bruneton's mapping assumes — the table is only parameterised over rays that
/// leave.
#[must_use]
pub fn transmittance_at(radius_km: f64, cos_zenith: f64) -> [f64; 3] {
    if hits_the_ground(radius_km, cos_zenith) {
        return [0.0; 3];
    }
    let end = distance_to_top(radius_km, cos_zenith);
    let step = end / TRANSMITTANCE_STEPS as f64;
    let ground = f64::from(GROUND_RADIUS_KM);
    let mut depth = [0.0f64; 3];
    for slice in 0..TRANSMITTANCE_STEPS {
        let distance = (slice as f64 + 0.5) * step;
        let sample_radius =
            (radius_km * radius_km + distance * distance + 2.0 * radius_km * distance * cos_zenith)
                .max(ground * ground)
                .sqrt();
        let (extinction, _, _) = medium(sample_radius - ground);
        for channel in 0..3 {
            depth[channel] += extinction[channel] * step;
        }
    }
    [(-depth[0]).exp(), (-depth[1]).exp(), (-depth[2]).exp()]
}

/// The transmittance LUT, row-major with altitude slow.
#[must_use]
pub fn bake_transmittance() -> Vec<[f32; 3]> {
    let mut table = Vec::with_capacity(TRANSMITTANCE_WIDTH * TRANSMITTANCE_HEIGHT);
    for altitude in 0..TRANSMITTANCE_HEIGHT {
        let v = axis_value(altitude, TRANSMITTANCE_HEIGHT);
        for zenith in 0..TRANSMITTANCE_WIDTH {
            let u = axis_value(zenith, TRANSMITTANCE_WIDTH);
            let (radius, cosine) = transmittance_params(u, v);
            let value = transmittance_at(f64::from(radius), f64::from(cosine));
            table.push([value[0] as f32, value[1] as f32, value[2] as f32]);
        }
    }
    table
}

/// The multiple-scattering LUT, row-major with altitude slow, integrated
/// against `transmittance`.
///
/// Hillaire's §4: at each `(altitude, sun)` pair, march
/// [`MULTISCATTER_DIRECTIONS`] directions with an **isotropic** phase function
/// and accumulate two quantities — the second-order scattered radiance `L₂` and
/// the fraction `f_ms` of light a point scatters back to itself. The infinite
/// series of further orders is then `L₂ / (1 − f_ms)`, which is the paper's
/// closed form for an infinitely scattering medium and the whole reason one
/// small table stands in for every order past the first.
///
/// `transmittance` is the freshly baked table rather than the committed one, so
/// the two halves of `tables/atmosphere.bin` are always each other's.
#[must_use]
pub fn bake_multiscatter(transmittance: &[[f32; 3]]) -> Vec<[f32; 3]> {
    assert_eq!(
        transmittance.len(),
        TRANSMITTANCE_WIDTH * TRANSMITTANCE_HEIGHT,
        "the multiple-scattering cook was handed a transmittance table of the wrong size"
    );
    let ground = f64::from(GROUND_RADIUS_KM);
    let top = f64::from(TOP_RADIUS_KM);
    let isotropic = 1.0 / (4.0 * std::f64::consts::PI);
    // The sphere's solid angle divided among the directions, then multiplied by
    // the isotropic phase again — Hillaire applies that second factor after the
    // reduction, and the two cancel into a plain mean over directions. Writing
    // it as the mean rather than as `4π/n · 1/4π` is what keeps a reader from
    // wondering which of the two `1/4π`s went missing.
    let per_direction = 1.0 / MULTISCATTER_DIRECTIONS as f64;
    let stratum = (MULTISCATTER_DIRECTIONS as f64).sqrt() as usize;

    let mut table = Vec::with_capacity(MULTISCATTER_SIZE * MULTISCATTER_SIZE);
    for altitude in 0..MULTISCATTER_SIZE {
        let radius = ground + f64::from(axis_value(altitude, MULTISCATTER_SIZE)) * (top - ground);
        for sun in 0..MULTISCATTER_SIZE {
            let sun_zenith = f64::from(axis_value(sun, MULTISCATTER_SIZE)) * 2.0 - 1.0;
            let sun_side = (1.0 - sun_zenith * sun_zenith).max(0.0).sqrt();

            let mut second_order = [0.0f64; 3];
            let mut transfer = [0.0f64; 3];
            for i in 0..stratum {
                for j in 0..stratum {
                    // A uniform stratification of the sphere: azimuth linear,
                    // polar cosine linear.
                    let azimuth = std::f64::consts::TAU * (i as f64 + 0.5) / stratum as f64;
                    let cos_polar = 1.0 - 2.0 * (j as f64 + 0.5) / stratum as f64;
                    let sin_polar = (1.0 - cos_polar * cos_polar).max(0.0).sqrt();
                    let direction = [
                        sin_polar * azimuth.cos(),
                        cos_polar,
                        sin_polar * azimuth.sin(),
                    ];
                    let (order, share) = multiscatter_ray(
                        transmittance,
                        radius,
                        direction,
                        sun_side,
                        sun_zenith,
                        isotropic,
                    );
                    for channel in 0..3 {
                        second_order[channel] += order[channel] * per_direction;
                        transfer[channel] += share[channel] * per_direction;
                    }
                }
            }

            let mut entry = [0.0f32; 3];
            for channel in 0..3 {
                // The geometric series of every further order. `f_ms` is a
                // fraction of an already-scattered quantity and stays well
                // under one for this medium; the clamp is what stops a
                // hypothetical medium that did not from dividing by zero.
                let remaining = (1.0 - transfer[channel]).max(1.0e-4);
                entry[channel] = (second_order[channel] / remaining) as f32;
            }
            table.push(entry);
        }
    }
    table
}

/// One direction of [`bake_multiscatter`]'s sphere: the second-order radiance
/// it brings back and the share of the point's own scattering it returns.
fn multiscatter_ray(
    transmittance: &[[f32; 3]],
    radius_km: f64,
    direction: [f64; 3],
    sun_side: f64,
    sun_up: f64,
    isotropic: f64,
) -> ([f64; 3], [f64; 3]) {
    let ground = f64::from(GROUND_RADIUS_KM);
    let up = direction[1];
    let end = if hits_the_ground(radius_km, up) {
        let discriminant = radius_km * radius_km * (up * up - 1.0) + ground * ground;
        (-radius_km * up - discriminant.max(0.0).sqrt()).max(0.0)
    } else {
        distance_to_top(radius_km, up)
    };
    let step = end / MULTISCATTER_STEPS as f64;

    let mut path = [1.0f64; 3];
    let mut order = [0.0f64; 3];
    let mut share = [0.0f64; 3];
    for slice in 0..MULTISCATTER_STEPS {
        let distance = (slice as f64 + 0.5) * step;
        let sample_radius =
            (radius_km * radius_km + distance * distance + 2.0 * radius_km * distance * up)
                .max(ground * ground)
                .sqrt();
        let (extinction, rayleigh, mie) = medium(sample_radius - ground);
        let sun_zenith = (distance * direction[0] * sun_side
            + (radius_km + distance * up) * sun_up)
            / sample_radius;
        let sun_transmittance = if hits_the_ground(sample_radius, sun_zenith) {
            [0.0f64; 3]
        } else {
            sampled_transmittance(transmittance, sample_radius, sun_zenith)
        };

        for channel in 0..3 {
            let scattering = rayleigh[channel] + mie;
            let depth = extinction[channel] * step;
            let slice_transmittance = (-depth).exp();
            // The same closed-form slice integral the runtime march uses,
            // written directly because this side may divide by an exponential.
            let integral = if extinction[channel] > 0.0 {
                (1.0 - slice_transmittance) / extinction[channel]
            } else {
                step
            };
            order[channel] +=
                path[channel] * scattering * isotropic * sun_transmittance[channel] * integral;
            share[channel] += path[channel] * scattering * integral;
            path[channel] *= slice_transmittance;
        }
    }

    if hits_the_ground(radius_km, up) {
        // The planet's own Lambertian bounce, which is what the higher orders
        // pick up off the ground. `end` put the ray on the surface, so the
        // local up there is the ray's own end point normalised.
        let sun_at_ground = (end * direction[0] * sun_side + (radius_km + end * up) * sun_up)
            / f64::from(GROUND_RADIUS_KM);
        if sun_at_ground > 0.0 {
            let ground_transmittance =
                sampled_transmittance(transmittance, f64::from(GROUND_RADIUS_KM), sun_at_ground);
            let albedo = f64::from(GROUND_ALBEDO) / std::f64::consts::PI;
            for channel in 0..3 {
                order[channel] +=
                    path[channel] * ground_transmittance[channel] * sun_at_ground * albedo;
            }
        }
    }
    (order, share)
}

/// A freshly baked transmittance table read bilinearly, in `f64`.
///
/// [`sample_transmittance`]'s arithmetic against a `&[[f32; 3]]` rather than
/// against the committed bytes, because the cook has to read the table it is
/// producing rather than the one on disk.
fn sampled_transmittance(table: &[[f32; 3]], radius_km: f64, cos_zenith: f64) -> [f64; 3] {
    let [u, v] = transmittance_uv(radius_km as f32, cos_zenith as f32);
    let (x0, x1, fx) = axis_taps(u, TRANSMITTANCE_WIDTH);
    let (y0, y1, fy) = axis_taps(v, TRANSMITTANCE_HEIGHT);
    let at =
        |x: usize, y: usize, channel: usize| f64::from(table[y * TRANSMITTANCE_WIDTH + x][channel]);
    let (fx, fy) = (f64::from(fx), f64::from(fy));
    let mut out = [0.0f64; 3];
    for (channel, slot) in out.iter_mut().enumerate() {
        let top = at(x0, y0, channel) * (1.0 - fx) + at(x1, y0, channel) * fx;
        let bottom = at(x0, y1, channel) * (1.0 - fx) + at(x1, y1, channel) * fx;
        *slot = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// Both tables as the bytes `tables/atmosphere.bin` holds: the transmittance
/// LUT, then the multiple-scattering LUT that was integrated against it.
#[must_use]
pub fn bake_bytes() -> Vec<u8> {
    let transmittance = bake_transmittance();
    let multiscatter = bake_multiscatter(&transmittance);
    let mut bytes = Vec::with_capacity(TABLE_BYTES);
    for entry in transmittance.iter().chain(&multiscatter) {
        for channel in entry {
            bytes.extend_from_slice(&channel.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sun ten degrees above the horizon, which is where an atmosphere stops
    /// looking like a gradient: the direction is exact rather than rounded from
    /// an angle, so no test here depends on a `sin` of its own.
    const LOW_SUN: Atmosphere = Atmosphere {
        sun_direction: [0.984_807_7, 0.173_648_18, 0.0],
        sun_illuminance: [1.0, 1.0, 1.0],
        altitude_km: 0.0,
    };

    /// The optical depth of a **vertical** ray from the ground, in closed form.
    ///
    /// The one geometry the spherical integral collapses for: the ray is
    /// radial, so height and path length are the same variable and each species
    /// integrates exactly — `σ·H·(1 − e^{−100/H})` for the two exponentials and
    /// `σ·OZONE_HALF_WIDTH_KM` for the tent, whose area is its half-width times
    /// its unit peak. This is the independent oracle
    /// [`transmittance_at`] is checked against; nothing in the module computes
    /// it.
    fn vertical_optical_depth(channel: usize) -> f64 {
        let top = f64::from(TOP_RADIUS_KM - GROUND_RADIUS_KM);
        let column = |scale: f64| scale * (1.0 - (-top / scale).exp());
        f64::from(RAYLEIGH_SCATTERING_PER_KM[channel]) * column(f64::from(RAYLEIGH_SCALE_HEIGHT_KM))
            + f64::from(MIE_EXTINCTION_PER_KM) * column(f64::from(MIE_SCALE_HEIGHT_KM))
            + f64::from(OZONE_ABSORPTION_PER_KM[channel]) * f64::from(OZONE_HALF_WIDTH_KM)
    }

    /// The same for a vertical ray starting `height_km` up: each exponential
    /// column is scaled by its own density there and the tent keeps whatever of
    /// its area is above the start.
    fn vertical_optical_depth_from(channel: usize, height_km: f64) -> f64 {
        let top = f64::from(TOP_RADIUS_KM - GROUND_RADIUS_KM);
        let column = |scale: f64| scale * ((-height_km / scale).exp() - (-top / scale).exp());
        let peak = f64::from(OZONE_PEAK_HEIGHT_KM);
        let half = f64::from(OZONE_HALF_WIDTH_KM);
        // The tent's area above `height_km`: whole below its foot, the two
        // triangles' remainder inside it, nothing above its far foot.
        let tail = |from: f64| {
            if from <= peak - half {
                half
            } else if from <= peak {
                // The whole falling side, plus what is left of the rising one
                // above `from`.
                let rising = peak - from;
                half * 0.5 + rising - rising * rising / (2.0 * half)
            } else if from <= peak + half {
                let falling = peak + half - from;
                falling * falling / (2.0 * half)
            } else {
                0.0
            }
        };
        f64::from(RAYLEIGH_SCATTERING_PER_KM[channel]) * column(f64::from(RAYLEIGH_SCALE_HEIGHT_KM))
            + f64::from(MIE_EXTINCTION_PER_KM) * column(f64::from(MIE_SCALE_HEIGHT_KM))
            + f64::from(OZONE_ABSORPTION_PER_KM[channel]) * tail(height_km)
    }

    /// The share of `reference` that `value` misses it by.
    fn relative(value: f64, reference: f64) -> f64 {
        (value - reference).abs() / reference.abs().max(f64::MIN_POSITIVE)
    }

    /// The integrator against the closed form a vertical ray has, at the ground
    /// and at four heights up the ozone layer.
    ///
    /// **This is the module's only claim about the physics that does not come
    /// out of the module**, so it is the one that would catch a mistyped
    /// coefficient, a scale height swapped between species, or an ozone tent
    /// with the wrong area. The tolerance is the march's own quadrature error
    /// at [`TRANSMITTANCE_STEPS`] and is measured rather than chosen — the test
    /// prints what it actually reaches.
    ///
    /// It has already caught one thing, in the oracle rather than in the
    /// module: an ozone tail that integrated the tent's rising side the wrong
    /// way round read 0.32 % out here, which is what a real coefficient slip
    /// would look like.
    #[test]
    fn the_vertical_transmittance_matches_its_closed_form() {
        let mut worst = 0.0f64;
        for height in [0.0f64, 5.0, 12.0, 25.0, 40.0] {
            let marched = transmittance_at(f64::from(GROUND_RADIUS_KM) + height, 1.0);
            for (channel, value) in marched.into_iter().enumerate() {
                let expected = (-vertical_optical_depth_from(channel, height)).exp();
                let miss = relative(value, expected);
                worst = worst.max(miss);
                assert!(
                    miss <= MAX_VERTICAL_ERROR,
                    "at {height} km the integrator transmits {value} in channel {channel} where \
                     the closed form gives {expected}, a miss of {miss}"
                );
            }
        }
        // The ground row, spelled against the simpler closed form as well, so
        // the height-dependent one above cannot be wrong in the same way.
        for channel in 0..3 {
            let expected = (-vertical_optical_depth(channel)).exp();
            let marched = transmittance_at(f64::from(GROUND_RADIUS_KM), 1.0)[channel];
            assert!(relative(marched, expected) <= MAX_VERTICAL_ERROR);
        }
        eprintln!(
            "crcbl-shaders atmosphere: the vertical march tracks its closed form to {:.4}% at \
             worst over five heights and three channels",
            worst * 100.0
        );
    }

    /// The largest share of the closed form the marched vertical transmittance
    /// may miss it by.
    ///
    /// Twice what `the_vertical_transmittance_matches_its_closed_form` prints,
    /// which is the march's quadrature error at [`TRANSMITTANCE_STEPS`] and
    /// nothing else — the closed form is exact.
    const MAX_VERTICAL_ERROR: f64 = 2.0e-4;

    /// Doubling [`TRANSMITTANCE_STEPS`] moves the table by less than the
    /// tolerance `cook-atmosphere --check` compares within.
    ///
    /// The claim the step count rests on: a finer march would not produce a
    /// different table, so the committed one is the converged one.
    #[test]
    fn the_transmittance_integrator_has_converged() {
        let mut worst = 0.0f64;
        for (radius, cosine) in [
            (f64::from(GROUND_RADIUS_KM), 1.0),
            (f64::from(GROUND_RADIUS_KM), 0.1),
            (f64::from(GROUND_RADIUS_KM), 0.0),
            (f64::from(GROUND_RADIUS_KM) + 20.0, 0.3),
            (f64::from(GROUND_RADIUS_KM) + 60.0, -0.1),
        ] {
            let coarse = transmittance_at(radius, cosine);
            let fine = fine_transmittance_at(radius, cosine, TRANSMITTANCE_STEPS * 8);
            for channel in 0..3 {
                worst = worst.max((coarse[channel] - fine[channel]).abs());
            }
        }
        assert!(
            worst <= 5.0e-4,
            "eight times the steps moves the transmittance by {worst}, so \
             TRANSMITTANCE_STEPS is not where this integrator has converged"
        );
        eprintln!(
            "crcbl-shaders atmosphere: eight times the transmittance steps moves a value by \
             {worst:.2e} at worst"
        );
    }

    /// [`transmittance_at`] at an arbitrary step count, for the convergence
    /// test alone — the shipped function pins the count so the table has one
    /// definition.
    fn fine_transmittance_at(radius_km: f64, cos_zenith: f64, steps: usize) -> [f64; 3] {
        if hits_the_ground(radius_km, cos_zenith) {
            return [0.0; 3];
        }
        let end = distance_to_top(radius_km, cos_zenith);
        let step = end / steps as f64;
        let ground = f64::from(GROUND_RADIUS_KM);
        let mut depth = [0.0f64; 3];
        for slice in 0..steps {
            let distance = (slice as f64 + 0.5) * step;
            let sample_radius = (radius_km * radius_km
                + distance * distance
                + 2.0 * radius_km * distance * cos_zenith)
                .max(ground * ground)
                .sqrt();
            let (extinction, _, _) = medium(sample_radius - ground);
            for channel in 0..3 {
                depth[channel] += extinction[channel] * step;
            }
        }
        [(-depth[0]).exp(), (-depth[1]).exp(), (-depth[2]).exp()]
    }

    /// The committed table is what [`transmittance_at`] produces, spot-checked
    /// on the four corners and the middle.
    ///
    /// The whole table is `cook-atmosphere --check`'s job; this is what fails
    /// in `cargo test` when somebody edits a coefficient and does not re-cook.
    #[test]
    fn the_committed_transmittance_is_what_the_integrator_produces() {
        for (zenith, altitude) in [
            (0usize, 0usize),
            (TRANSMITTANCE_WIDTH - 1, 0),
            (0, TRANSMITTANCE_HEIGHT - 1),
            (TRANSMITTANCE_WIDTH - 1, TRANSMITTANCE_HEIGHT - 1),
            (TRANSMITTANCE_WIDTH / 2, TRANSMITTANCE_HEIGHT / 2),
        ] {
            let (radius, cosine) = transmittance_params(
                axis_value(zenith, TRANSMITTANCE_WIDTH),
                axis_value(altitude, TRANSMITTANCE_HEIGHT),
            );
            let fresh = transmittance_at(f64::from(radius), f64::from(cosine));
            let committed = transmittance_entry(zenith, altitude);
            for channel in 0..3 {
                assert!(
                    (f64::from(committed[channel]) - fresh[channel]).abs() <= 1.0e-6,
                    "tables/atmosphere.bin holds {} at ({zenith}, {altitude}) channel {channel} \
                     where the integrator produces {} — regenerate with `cargo run -p \
                     crcbl-shaders --example cook-atmosphere`",
                    committed[channel],
                    fresh[channel]
                );
            }
        }
    }

    /// [`transmittance_uv`] and [`transmittance_params`] are each other's
    /// inverse.
    ///
    /// The mapping is Bruneton's and it is written twice — once forward for the
    /// read, once back for the cook — so a table baked on one and read on the
    /// other would be smoothly and invisibly wrong.
    #[test]
    fn the_transmittance_mapping_round_trips() {
        let mut worst = 0.0f32;
        for altitude in 0..TRANSMITTANCE_HEIGHT {
            let v = axis_value(altitude, TRANSMITTANCE_HEIGHT);
            for zenith in 0..TRANSMITTANCE_WIDTH {
                let u = axis_value(zenith, TRANSMITTANCE_WIDTH);
                let (radius, cosine) = transmittance_params(u, v);
                let [back_u, back_v] = transmittance_uv(radius, cosine);
                worst = worst.max((back_u - u).abs()).max((back_v - v).abs());
            }
        }
        assert!(
            worst <= 1.0e-4,
            "the transmittance mapping loses {worst} of a texel round-tripping"
        );
    }

    /// The multiple-scattering table is a small, positive fraction of the sun's
    /// own illuminance, and it grows with the sun's elevation.
    ///
    /// The structural claims a mis-normalised cook fails: an earlier draft
    /// summed the sphere without the isotropic phase and produced entries in
    /// the thousands, which is what this floor and ceiling are shaped against.
    #[test]
    fn the_multiscatter_table_is_a_small_share_of_the_sun() {
        let ground = 0;
        let overhead = multiscatter_entry(MULTISCATTER_SIZE - 1, ground);
        let below = multiscatter_entry(0, ground);
        for channel in 0..3 {
            assert!(
                overhead[channel] > 0.0 && overhead[channel] < 1.0,
                "the multiple-scattering term under an overhead sun is {} in channel {channel}, \
                 which is not a share of the sun's illuminance",
                overhead[channel]
            );
            assert!(
                below[channel] < overhead[channel],
                "channel {channel} scatters {} with the sun below the horizon and {} with it \
                 overhead",
                below[channel],
                overhead[channel]
            );
        }
        // Blue over red, for the reason the sky is blue at all.
        assert!(overhead[2] > overhead[0]);
    }

    /// The `f64` cook and the `f32` runtime agree about the medium.
    ///
    /// Two spellings of one profile, which is what
    /// [`crate::sky_prefilter`]'s `blend` is to [`crate::sky::smoothstep`]: the
    /// wider one because the cook sums thousands of terms, the narrower one
    /// because it reaches a colour.
    #[test]
    fn the_two_precisions_agree_about_the_densities() {
        let mut worst = 0.0f64;
        for step in 0..=200u32 {
            let height = f64::from(step) * 0.5;
            let (rayleigh, mie, ozone) = densities(height);
            let narrow = (
                f64::from(rayleigh_density(height as f32)),
                f64::from(mie_density(height as f32)),
                f64::from(ozone_density(height as f32)),
            );
            for (wide, narrow) in [(rayleigh, narrow.0), (mie, narrow.1), (ozone, narrow.2)] {
                worst = worst.max((wide - narrow).abs());
            }
        }
        assert!(
            worst <= 1.0e-6,
            "the two precisions disagree about a density by {worst}"
        );
    }

    /// The sky-view LUT's two coordinate maps are each other's inverse, and
    /// each is monotone.
    #[test]
    fn the_sky_view_maps_round_trip_and_stay_monotone() {
        let mut previous_up = f32::NEG_INFINITY;
        let mut previous_cosine = f32::INFINITY;
        for step in 0..=1000u32 {
            let t = f32::from(u16::try_from(step).expect("a thousand")) / 1000.0;
            let up = sky_view_up_of(t);
            assert!(up >= previous_up, "the row map fell at {t}");
            previous_up = up;
            assert!(
                (sky_view_v_of(up) - t).abs() <= 1.0e-4,
                "the row map at {t}"
            );

            let cosine = sky_view_cosine_of(t);
            assert!(cosine <= previous_cosine, "the column map rose at {t}");
            previous_cosine = cosine;
            assert!(
                (sky_view_u_of(cosine) - t).abs() <= 1.0e-4,
                "the column map at {t}"
            );
        }
        // The two ends are exact, so the poles and the sun's own azimuth are
        // read from the texels that stand for them rather than beside them.
        assert_eq!(sky_view_up_of(0.0), -1.0);
        assert_eq!(sky_view_up_of(0.5), 0.0);
        assert_eq!(sky_view_up_of(1.0), 1.0);
        assert_eq!(sky_view_cosine_of(0.0), 1.0);
        assert_eq!(sky_view_cosine_of(1.0), -1.0);
    }

    /// Two builds from one [`Atmosphere`] produce the same bytes.
    ///
    /// The property the whole restriction on this module's arithmetic exists
    /// for: the LUT is uploaded rather than computed on the device, so a
    /// machine that produced different bytes would put a different sky on that
    /// backend and no golden could hold both.
    #[test]
    fn the_lut_is_bit_identical_across_two_builds() {
        let first = SkyView::build(&LOW_SUN);
        let second = SkyView::build(&LOW_SUN);
        assert_eq!(
            first.rows(),
            second.rows(),
            "two builds of one atmosphere disagree, so something in the march is not a function \
             of its inputs alone"
        );
        // And the two builds are not both empty, which is the way a comparison
        // like this passes for the wrong reason.
        assert!(
            first.rows().iter().any(|byte| *byte != 0),
            "the LUT is all zeroes, so the equality above says nothing"
        );
    }

    /// A build marched in stripes is the build marched in one call.
    ///
    /// **What the amortisation rests on.** `crcbl_render::forward` shows the
    /// LUT it last finished while a [`SkyViewBuild`] catches up with a sun that
    /// moved, and that is only a stale sky rather than a wrong one if stopping
    /// between two rows changes nothing about the rows either side. Three
    /// stripe widths: one row at a time, seven — which divides neither
    /// [`SKY_VIEW_HEIGHT`] nor [`SKY_VIEW_BUILD_ROWS`], so the last stripe is
    /// short and every stripe after the first starts at an odd row — and the
    /// whole LUT in one step.
    #[test]
    fn a_striped_build_is_the_one_shot_build() {
        let whole = SkyView::build(&LOW_SUN).rows();
        assert!(
            whole.iter().any(|byte| *byte != 0),
            "the reference LUT is all zeroes, so the equalities below say nothing"
        );
        for stripe in [1usize, 7, SKY_VIEW_HEIGHT] {
            let mut build = SkyViewBuild::start(&LOW_SUN);
            let mut steps = 0usize;
            while !build.step(stripe) {
                steps += 1;
                assert_eq!(
                    build.rows_done(),
                    (steps * stripe).min(SKY_VIEW_HEIGHT),
                    "a {stripe}-row step left the march somewhere other than where it says"
                );
            }
            assert_eq!(
                build.rows_done(),
                SKY_VIEW_HEIGHT,
                "a completed {stripe}-row build has not marched the whole LUT"
            );
            assert_eq!(
                build.finish().rows(),
                whole,
                "a build marched {stripe} rows at a time is not the build marched in one go"
            );
        }
    }

    /// Prints what a whole [`SkyView::build`] costs and what one
    /// [`SkyViewBuild::step`] of [`SKY_VIEW_BUILD_ROWS`] costs beside it:
    ///
    /// ```text
    /// cargo test -p crcbl-shaders --release --lib -- --ignored --nocapture \
    ///     --exact atmosphere::tests::the_amortised_step_is_a_fraction_of_the_whole_build
    /// ```
    ///
    /// `#[ignore]` because it is a measurement and not a check: a wall clock on
    /// a shared machine is not something to fail a build on. It is what the
    /// numbers in `docs/plan/43-render-standards.md` §8 and the `CHANGELOG`
    /// entry come from, so it is here rather than in a scratch file. Medians of
    /// three, and `--release` because a debug march is not the one that ships.
    #[test]
    #[ignore = "measures wall time rather than checking anything"]
    fn the_amortised_step_is_a_fraction_of_the_whole_build() {
        use std::time::Instant;

        let median = |mut runs: Vec<f64>| -> f64 {
            runs.sort_by(f64::total_cmp);
            runs[runs.len() / 2]
        };
        let runs = 3;
        let whole = median(
            (0..runs)
                .map(|_| {
                    let at = Instant::now();
                    let built = SkyView::build(&LOW_SUN);
                    let elapsed = at.elapsed().as_secs_f64() * 1.0e3;
                    // Read the LUT back so no optimiser can decide the march
                    // was not worth doing.
                    assert!(built.entry(0, SKY_VIEW_HEIGHT - 1)[0] >= 0.0);
                    elapsed
                })
                .collect(),
        );
        let stripe = median(
            (0..runs)
                .map(|_| {
                    let mut build = SkyViewBuild::start(&LOW_SUN);
                    let at = Instant::now();
                    build.step(SKY_VIEW_BUILD_ROWS);
                    let elapsed = at.elapsed().as_secs_f64() * 1.0e3;
                    assert_eq!(build.rows_done(), SKY_VIEW_BUILD_ROWS);
                    elapsed
                })
                .collect(),
        );
        // What a frame under a *static* sun used to pay: both projections were
        // taken from the presented LUT every frame until they were cached
        // beside it.
        let built = SkyView::build(&LOW_SUN);
        let projections = median(
            (0..runs)
                .map(|_| {
                    let at = Instant::now();
                    let gradient = built.gradient_fit();
                    let probe = built.irradiance();
                    let elapsed = at.elapsed().as_secs_f64() * 1.0e3;
                    assert!(gradient.horizon[0] >= 0.0 && probe.sh_r[3] >= 0.0);
                    elapsed
                })
                .collect(),
        );
        // The one host cost left in a frame whose sun has not moved: the LUT
        // still has to reach that frame's own ring slot.
        let encode = median(
            (0..runs)
                .map(|_| {
                    let at = Instant::now();
                    let rows = built.rows();
                    let elapsed = at.elapsed().as_secs_f64() * 1.0e3;
                    assert_eq!(rows.len(), SKY_VIEW_BUFFER_BYTES);
                    elapsed
                })
                .collect(),
        );
        println!(
            "SkyView::build {whole:.2} ms, one {SKY_VIEW_BUILD_ROWS}-row step {stripe:.3} ms, \
             gradient_fit + irradiance {projections:.3} ms, rows {encode:.3} ms"
        );
    }

    /// Nothing between the runtime path's two markers calls a transcendental.
    ///
    /// **The guard the module's whole shape rests on.** A `sin`, an `exp` or a
    /// `powf` added to [`SkyView::build`]'s reach would compile, would look
    /// right, and would put a different sky on macOS than on Linux — which no
    /// test comparing one machine against itself can see. So this reads the
    /// source between the markers instead.
    #[test]
    fn the_runtime_path_calls_no_transcendental() {
        let source = include_str!("atmosphere.rs");
        let body = source
            .split_once("// RUNTIME PATH BEGINS")
            .expect("this file marks where the runtime path starts")
            .1
            .split_once("// RUNTIME PATH ENDS")
            .expect("this file marks where the runtime path ends")
            .0;
        // Every `f32`/`f64` method IEEE-754 does not pin down, plus the free
        // functions a caller might reach for instead.
        for banned in [
            ".exp(",
            ".exp2(",
            ".exp_m1(",
            ".ln(",
            ".log(",
            ".log2(",
            ".log10(",
            ".powf(",
            ".powi(",
            ".sin(",
            ".cos(",
            ".tan(",
            ".asin(",
            ".acos(",
            ".atan(",
            ".atan2(",
            ".sinh(",
            ".cosh(",
            ".tanh(",
            ".cbrt(",
            ".hypot(",
            ".to_radians(",
            ".to_degrees(",
        ] {
            assert!(
                !body.contains(banned),
                "the runtime path calls `{banned}`, which no two platforms compute the same way \
                 — build it out of `crate::fog`'s construction instead"
            );
        }
        // And the guard is not vacuous: the region really is the one that
        // marches, and it really does reach the exponential it is allowed.
        assert!(
            body.contains("fn march("),
            "the markers do not enclose the march"
        );
        assert!(
            body.contains("exp_neg(optical_depth)"),
            "the march no longer takes the exponential this module built for it"
        );
    }

    /// A LUT holding one radiance everywhere projects to `π` times it, with no
    /// directional band.
    ///
    /// The check on [`SkyView::irradiance`]'s **quadrature weights** alone,
    /// separated from the sky: a constant environment of radiance `L` reaches a
    /// surface as `πL` whichever way it faces, so this fails if the tangent
    /// half-angle nodes, their `dφ`, or the band's `dy` are wrong — and it
    /// cannot be rescued by the field being smooth.
    #[test]
    fn a_uniform_lut_projects_to_pi_times_its_radiance() {
        let uniform = SkyView {
            radiance: vec![[0.25, 0.5, 1.0]; SKY_VIEW_WIDTH * SKY_VIEW_HEIGHT],
            sun_direction: LOW_SUN.sun_direction,
        };
        let probe = uniform.irradiance();
        let mut worst = 0.0f32;
        for (band, radiance) in [(probe.sh_r, 0.25f32), (probe.sh_g, 0.5), (probe.sh_b, 1.0)] {
            let expected = core::f32::consts::PI * radiance;
            worst = worst.max((band[3] - expected).abs() / expected);
            for linear in &band[..3] {
                worst = worst.max(linear.abs() / expected);
            }
            assert!(
                (band[3] - expected).abs() <= expected * MAX_UNIFORM_ERROR,
                "the constant band is {} where a uniform sky of {radiance} gives {expected}",
                band[3]
            );
            for (axis, linear) in band[..3].iter().enumerate() {
                assert!(
                    linear.abs() <= expected * MAX_UNIFORM_ERROR,
                    "a uniform sky left a linear band of {linear} on axis {axis}"
                );
            }
        }
        eprintln!(
            "crcbl-shaders atmosphere: a uniform LUT projects to within {:.2e} of πL over every \
             coefficient",
            worst
        );
    }

    /// The share of `πL` a uniform LUT's projection may miss it by.
    ///
    /// The quadrature's own error and nothing else — the field is constant, so
    /// there is nothing for it to resolve, and the test prints what it reaches.
    const MAX_UNIFORM_ERROR: f32 = 1.5e-4;

    /// The L1 projection against a brute-force integral of the same LUT.
    ///
    /// The oracle is written a different way on purpose: a uniform grid in the
    /// polar and azimuthal **angles**, with the `sin` and `cos` a test is
    /// allowed and the shipped projection is not, in `f64`, at far more samples
    /// than the shipped one takes. So the two share the LUT and nothing else —
    /// not the nodes, not the weights, not the arithmetic.
    ///
    /// The tolerance is a share of the constant band, and it is the quadrature
    /// gap between the two rather than anything about the sky — the test prints
    /// what it reaches over both fixtures and all twelve coefficients.
    #[test]
    fn the_l1_projection_matches_a_brute_force_integral() {
        let mut worst = 0.0f64;
        for atmosphere in [Atmosphere::NOON, LOW_SUN] {
            let sky = SkyView::build(&atmosphere);
            let projected = sky.irradiance();
            let reference = brute_force_irradiance(&sky, 512, 1024);
            let scale = f64::from(projected.sh_b[3]).abs();
            assert!(scale > 0.0, "the fixture's sky is black");
            for (band, oracle) in [
                (projected.sh_r, reference[0]),
                (projected.sh_g, reference[1]),
                (projected.sh_b, reference[2]),
            ] {
                for coefficient in 0..4 {
                    let miss = (f64::from(band[coefficient]) - oracle[coefficient]).abs() / scale;
                    worst = worst.max(miss);
                    assert!(
                        miss <= MAX_PROJECTION_ERROR,
                        "coefficient {coefficient} projects to {} where the brute-force integral \
                         gives {}",
                        band[coefficient],
                        oracle[coefficient]
                    );
                }
            }
        }
        eprintln!(
            "crcbl-shaders atmosphere: the L1 projection tracks a brute-force integral to \
             {:.3}% of the constant band at worst",
            worst * 100.0
        );
    }

    /// The largest share of the constant band [`SkyView::irradiance`] may miss
    /// a brute-force integral of the same LUT by.
    ///
    /// Four times what `the_l1_projection_matches_a_brute_force_integral`
    /// prints, which is the gap between two quadratures of one field and not an
    /// error in either.
    const MAX_PROJECTION_ERROR: f64 = 3.0e-3;

    /// The same integral as [`SkyView::irradiance`], in angles and in `f64`.
    ///
    /// Returns `[channel][coefficient]` in [`GpuProbe`]'s packing — the linear
    /// band in `0..3` and the constant band in `3` — with the same per-sample
    /// weights [`GpuProbe::accumulate`] applies.
    fn brute_force_irradiance(sky: &SkyView, polar: usize, azimuth: usize) -> [[f64; 4]; 3] {
        let project_l0 = f64::from(crate::probe::TRANSFER_L0) / (4.0 * std::f64::consts::PI);
        let project_l1 = 3.0 * f64::from(crate::probe::TRANSFER_L1) / (4.0 * std::f64::consts::PI);
        let flat = (f64::from(sky.sun_direction[0]) * f64::from(sky.sun_direction[0])
            + f64::from(sky.sun_direction[2]) * f64::from(sky.sun_direction[2]))
        .sqrt();
        let towards = if flat > 0.0 {
            [
                f64::from(sky.sun_direction[0]) / flat,
                f64::from(sky.sun_direction[2]) / flat,
            ]
        } else {
            [1.0, 0.0]
        };
        let across = [towards[1], -towards[0]];

        let mut out = [[0.0f64; 4]; 3];
        let band = 2.0 / polar as f64;
        let turn = std::f64::consts::TAU / azimuth as f64;
        for row in 0..polar {
            let up = -1.0 + (row as f64 + 0.5) * band;
            let side = (1.0 - up * up).max(0.0).sqrt();
            for column in 0..azimuth {
                let angle = (column as f64 + 0.5) * turn;
                let (sine, cosine) = angle.sin_cos();
                let direction = [
                    side * (cosine * towards[0] + sine * across[0]),
                    up,
                    side * (cosine * towards[1] + sine * across[1]),
                ];
                let radiance = sky.sample(up as f32, cosine as f32);
                let solid_angle = band * turn;
                for channel in 0..3 {
                    let value = f64::from(radiance[channel]);
                    for axis in 0..3 {
                        out[channel][axis] += solid_angle * project_l1 * value * direction[axis];
                    }
                    out[channel][3] += solid_angle * project_l0 * value;
                }
            }
        }
        out
    }

    /// Doubling and quadrupling [`SKY_VIEW_STEPS`] does not move the sky by
    /// more than the constant's own doc claims.
    ///
    /// The measurement the shipped step count was chosen from, kept as a test
    /// so the number in that doc cannot quietly stop being true.
    #[test]
    fn the_march_has_converged_at_the_shipped_step_count() {
        let rays: [([f32; 3], f32, f32, f32); 4] = [
            ([0.0, 1.0, 0.0], 0.0, 1.0, 1.0),
            ([0.984_807_7, 0.173_648_18, 0.0], 0.0, 1.0, 0.173_648_18),
            ([1.0, 0.0, 0.0], 0.984_807_7, 0.173_648_18, 0.984_807_7),
            ([-1.0, 0.0, 0.0], 0.984_807_7, 0.173_648_18, -0.984_807_7),
        ];
        let sweep = [16usize, 24, 32, 48, 64];
        let mut row = String::new();
        let mut shipped_worst = 0.0f32;
        for steps in sweep {
            let mut worst = 0.0f32;
            for (direction, sun_side, sun_up, sun_cosine) in rays {
                let coarse = march(
                    GROUND_RADIUS_KM,
                    direction,
                    sun_side,
                    sun_up,
                    sun_cosine,
                    steps,
                );
                let reference = march(
                    GROUND_RADIUS_KM,
                    direction,
                    sun_side,
                    sun_up,
                    sun_cosine,
                    SKY_VIEW_STEPS * 32,
                );
                for channel in 0..3 {
                    worst = worst.max(
                        (coarse[channel] - reference[channel]).abs()
                            / reference[channel].max(1.0e-9),
                    );
                }
            }
            row.push_str(&format!(" {steps}:{:.2}%", worst * 100.0));
            if steps == SKY_VIEW_STEPS {
                shipped_worst = worst;
            }
        }
        assert!(
            sweep.contains(&SKY_VIEW_STEPS),
            "the sweep no longer covers the shipped step count, so it measures nothing about it"
        );
        assert!(
            shipped_worst > 0.0 && shipped_worst <= 0.01,
            "the shipped march misses a far finer one by {shipped_worst}, which is past the \
             per cent SKY_VIEW_STEPS' doc rests on"
        );
        eprintln!(
            "crcbl-shaders atmosphere: the march's worst channel against a far finer one, over \
             four rays —{row}"
        );
    }

    /// A view ray leaving the surface downwards ends where it starts, so the
    /// lower hemisphere is black.
    ///
    /// The module header calls this a decision; this is what makes it one
    /// rather than an accident. It caught the version that read a near root of
    /// exactly zero as "no hit" and marched the ray through the planet, which
    /// lit the ground brighter than the horizon.
    #[test]
    fn the_ground_is_black_from_the_surface() {
        let sky = SkyView::build(&Atmosphere::NOON);
        assert_eq!(sky.radiance([0.0, -1.0, 0.0]), [0.0; 3]);
        assert_eq!(distance_to_end(GROUND_RADIUS_KM, -1.0), 0.0);
        assert_eq!(distance_to_end(GROUND_RADIUS_KM, -0.5), 0.0);
        // And the sky above it is not, which is what stops this passing on a
        // LUT that is black everywhere.
        assert!(sky.radiance([0.0, 1.0, 0.0])[2] > 0.0);
    }

    /// The sky is blue overhead and the low sun's own horizon is red.
    ///
    /// The observable Rayleigh's `λ⁻⁴` exists for, stated as an ordering rather
    /// than as numbers: overhead the blue channel leads, and along the horizon
    /// towards a sun ten degrees up the red channel leads, because the long
    /// slant path has already taken the blue out.
    #[test]
    fn the_sky_is_blue_overhead_and_red_towards_a_low_sun() {
        let noon = SkyView::build(&Atmosphere::NOON);
        let overhead = noon.radiance([0.0, 1.0, 0.0]);
        assert!(
            overhead[2] > overhead[1] && overhead[1] > overhead[0],
            "the midday zenith reads {overhead:?}, which is not blue"
        );

        let dusk = SkyView::build(&LOW_SUN);
        let towards = dusk.radiance([1.0, 0.0, 0.0]);
        assert!(
            towards[0] > towards[1] && towards[1] > towards[2],
            "the horizon under the sun reads {towards:?}, which is not red"
        );
        let away = dusk.radiance([-1.0, 0.0, 0.0]);
        assert!(
            towards[0] > 3.0 * away[0],
            "the sun's own side of the horizon reads {} against {} away from it, which is not a \
             sunset at all",
            towards[0],
            away[0]
        );
    }

    /// Doubling the sun's illuminance doubles every radiance and every
    /// coefficient.
    ///
    /// What makes a scene's exposure a scale rather than a rebuild, and what
    /// [`Atmosphere::sun_illuminance`]'s doc claims when it says the integral
    /// is linear in it.
    #[test]
    fn the_sky_is_linear_in_the_suns_illuminance() {
        let dim = SkyView::build(&LOW_SUN);
        let bright = SkyView::build(&Atmosphere {
            sun_illuminance: [2.0, 2.0, 2.0],
            ..LOW_SUN
        });
        for row in 0..SKY_VIEW_HEIGHT {
            for column in 0..SKY_VIEW_WIDTH {
                let one = dim.entry(column, row);
                let two = bright.entry(column, row);
                for channel in 0..3 {
                    assert_eq!(
                        two[channel],
                        one[channel] * 2.0,
                        "({column}, {row}) channel {channel} is not linear in the sun"
                    );
                }
            }
        }
    }

    /// The uploaded rows are the LUT, exactly, in the order the shader indexes
    /// them.
    ///
    /// Exact rather than within anything: the buffer holds the same `f32`s the
    /// march produced, so a tolerance here would only hide a row written to the
    /// wrong offset.
    #[test]
    fn the_rows_carry_the_lut_they_encode() {
        let sky = SkyView::build(&LOW_SUN);
        let rows = sky.rows();
        assert_eq!(rows.len(), SKY_VIEW_BUFFER_BYTES);
        let lane = |at: usize| f32::from_le_bytes(rows[at..at + 4].try_into().expect("four"));
        for row in 0..SKY_VIEW_HEIGHT {
            for column in 0..SKY_VIEW_WIDTH {
                let entry = sky.entry(column, row);
                let at = (row * SKY_VIEW_WIDTH + column) * SKY_VIEW_ROW_BYTES;
                for (channel, value) in entry.into_iter().enumerate() {
                    assert_eq!(
                        lane(at + channel * 4),
                        value,
                        "({column}, {row}) channel {channel}"
                    );
                }
                assert_eq!(lane(at + 12), 1.0, "the fourth lane is the padding one");
            }
        }
        // And the rows are not all one number, which is how an offset mistake
        // would pass a comparison this exact.
        assert!(
            rows.chunks_exact(4)
                .map(lane_of)
                .collect::<std::collections::BTreeSet<u32>>()
                .len()
                > 100,
            "the LUT holds too few distinct values for this comparison to mean anything"
        );
    }

    /// One row lane's bits, for the distinctness count above.
    fn lane_of(bytes: &[u8]) -> u32 {
        u32::from_le_bytes(bytes.try_into().expect("four"))
    }

    /// `sky.slang` and `ssr.slang` read the LUT the way [`SkyView::sample`] and
    /// [`SkyView::radiance`] do.
    ///
    /// Slang has no `#include`, so **each** shader spells the two coordinate
    /// maps and the clamped bilinear again — the background pass because it
    /// draws the sky, the reflection pass because a mirror reflects it. The
    /// **dimensions** are checked numerically — parsed out of each shader and
    /// compared to this module's constants — because a LUT read at the wrong
    /// width is a sky that is smoothly and plausibly wrong rather than an
    /// error; the rest are the lines whose absence would change what the shader
    /// computes.
    #[test]
    fn the_shader_reads_the_lut_the_way_the_host_does() {
        for (file, source) in [
            ("sky.slang", include_str!("../shaders/sky.slang")),
            ("ssr.slang", include_str!("../shaders/ssr.slang")),
        ] {
            let declared = |name: &str| {
                source
                    .split_once(&format!("static const uint {name} = "))
                    .unwrap_or_else(|| panic!("{file} declares `{name}`"))
                    .1
                    .split_once(';')
                    .expect("the constant ends")
                    .0
                    .trim()
                    .parse::<usize>()
                    .expect("the constant is a literal")
            };
            assert_eq!(declared("SKY_VIEW_WIDTH"), SKY_VIEW_WIDTH);
            assert_eq!(declared("SKY_VIEW_HEIGHT"), SKY_VIEW_HEIGHT);

            let body = source
                .split_once("float3 sky_view_at(float up, float azimuth_cosine)\n{")
                .unwrap_or_else(|| panic!("{file} declares `sky_view_at`"))
                .1
                .split_once("\n}")
                .expect("the function has a body")
                .0;
            for line in [
                // `sky_view_u_of`: the column map's inverse.
                "float u = sqrt(max(0.0, (1.0 - clamp(azimuth_cosine, -1.0, 1.0)) * 0.5));",
                // `sky_view_v_of`: the row map's, sign and all.
                "float v = 0.5 + 0.5 * (clamped >= 0.0 ? root : -root);",
                // `axis_taps`, both axes: centres, floor, and both ends clamped.
                "float across = clamp(u, 0.0, 1.0) * float(SKY_VIEW_WIDTH) - 0.5;",
                "float down = clamp(v, 0.0, 1.0) * float(SKY_VIEW_HEIGHT) - 0.5;",
                // The blend, in the two-ended form `sample` uses — `lerp` is the
                // other one, and the two do not agree in floating point.
                "return top * (1.0 - fy) + bottom * fy;",
            ] {
                assert!(
                    body.contains(line),
                    "{file}'s `sky_view_at` no longer contains `{line}`, so it and \
                     `SkyView::sample` are reading different LUTs"
                );
            }

            let radiance = source
                .split_once("float3 atmosphere_radiance(float3 direction)\n{")
                .unwrap_or_else(|| panic!("{file} declares `atmosphere_radiance`"))
                .1
                .split_once("\n}")
                .expect("the function has a body")
                .0;
            assert!(
                radiance.contains(
                    "(direction.x * sun.x + direction.z * sun.z) / (view_flat * sun_flat)"
                ),
                "{file} no longer takes the azimuth cosine between the two horizontal \
                 projections, so its column and `SkyView::radiance`'s are different columns"
            );
            assert!(
                radiance.contains("return sky_view_at(direction.y, cosine);"),
                "{file} no longer reads the row from the direction's own `y`"
            );
        }
    }

    /// `ssr.slang` mixes the LUT into the three bands the way a host predicting
    /// a reflected pixel has to.
    ///
    /// The bullet this rung closed said a mirror must read the LUT and a rough
    /// lobe must keep the gradient, and the *ramp between them* is the whole of
    /// what a test can hold: `render_e2e`'s `an_atmosphere_mirror_reflects_the_
    /// luts_limb` predicts a floor band by evaluating this same blend on the
    /// host, so a shader that weighted the two differently would move every
    /// prediction at once. The share is `sharpness_of`'s ramp, which is why the
    /// argument is passed in rather than derived here.
    #[test]
    fn the_reflection_pass_blends_the_lut_over_the_bands() {
        let source = include_str!("../shaders/ssr.slang");
        let body = source
            .split_once("float3 sky_environment(float3 direction, float roughness, float share)\n{")
            .expect("ssr.slang declares `sky_environment`")
            .1
            .split_once("\n}\n")
            .expect("the function has a body")
            .0;
        for line in [
            // The bands, at the surface's own roughness.
            "float3 bands = sky_prefiltered(direction, roughness);",
            // A frame with no atmosphere returns them untouched, which is what
            // keeps every golden blessed before this existed byte-identical.
            "if (camera.atmosphere.w <= 0.0)",
            "return bands;",
            // And the two-ended blend, in the form the host mirror is written
            // in — `lerp` is the other one and the two differ in floating point.
            "return bands * (1.0 - share) + atmosphere_radiance(direction) * share;",
        ] {
            assert!(
                body.contains(line),
                "ssr.slang's `sky_environment` no longer contains `{line}`, so a host \
                 predicting a reflected pixel is predicting a different blend"
            );
        }
        assert!(
            source.contains("+ sky_environment(reflection_direction, surface.a, sharpness);"),
            "ssr.slang no longer hands `sky_environment` the march's own sharpness ramp, so \
             the lobe that reads one LUT tap and the lobe that trusts one screen-space ray \
             are no longer the same lobe"
        );
    }

    /// The gradient the reflection pass is handed reads the same three
    /// directions the background does.
    #[test]
    fn the_gradient_fit_takes_the_luts_own_bands() {
        let sky = SkyView::build(&LOW_SUN);
        let fit = sky.gradient_fit();
        assert_eq!(fit.zenith, sky.radiance([0.0, 1.0, 0.0]));
        assert_eq!(fit.ground, sky.radiance([0.0, -1.0, 0.0]));
        // The horizon band is the azimuthal mean, so it sits between the
        // brightest and the dimmest horizon the LUT holds.
        let towards = sky.radiance([1.0, 0.0, 0.0]);
        let away = sky.radiance([-1.0, 0.0, 0.0]);
        for channel in 0..3 {
            assert!(
                fit.horizon[channel] < towards[channel] && fit.horizon[channel] > away[channel],
                "the horizon band {} in channel {channel} is not between {} and {}",
                fit.horizon[channel],
                away[channel],
                towards[channel]
            );
        }
    }
}
