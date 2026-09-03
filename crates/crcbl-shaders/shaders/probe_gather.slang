// crcbl-targets: spirv, wgsl, msl, dxil

// The second half of `docs/plan/50-irradiance-probes.md`'s raster updater: the
// reflective shadow map `mesh.slang`'s `rsmFragmentMain` drew, gathered into
// every probe row of the volume, each sample gated by that probe's own
// visibility map.
//
// ```text
//  rsm albedo ─┐
//  rsm normal ─┼─▶ one workgroup per probe, every thread striding every texel
//  rsm world ──┘        │
//                       ├─ probe_chebyshev ─▶ a texel the probe cannot see is
//                       │                     dropped outright
//                       ▼
//        groupshared tree reduction ──▶ lane 0 writes the probe's three float4s
// ```
//
// # One workgroup per probe, and every probe gathers every texel
//
// The dispatch is one group per row of the table, not `div_ceil` of anything:
// a scene of sixty probes would otherwise run on a single workgroup. Inside a
// group the threads stride the whole map, which is what makes the sample
// pattern *the image* — nothing here maps a probe to a subset of texels, so
// there is no mapping to transcribe, no hash, no stratum table and no temporal
// history. `docs/backlog.md`'s survey constraint C2 holds by construction and a
// frame is a function of its own inputs.
//
// # `probe_chebyshev` and not `probe_weight`
//
// `PROBE_OCCLUDED_WEIGHT` is a property of the trilinear blend's *divisor* — it
// is what keeps the sum of a fragment's eight corner weights off zero. This
// gather has no divisor: it sums thousands of texels into one row, so a floor
// under every wall-hidden texel would let each of them contribute a
// ten-thousandth of its flux, and thousands of those is a leak by another name.
// `mesh.slang` splits the bound out of the floor for exactly this call, and
// `crcbl_shaders::probe_visibility`'s
// `the_shaders_weigh_a_probe_the_way_this_module_does` holds this file's copy of
// the bound to that file's and to `ssr.slang`'s, character for character.
//
// # The albedo target carries no sun, and this is where the sun is applied
//
// `RsmOutput` in `mesh.slang` argues it: the pass is drawn through one
// directional light's cascade, so the sun's colour is one number for the whole
// image. `params.sun_color` is the colour `crcbl_render::light::sun_row` writes,
// handed in as data.
//
// **What one texel contributes, derived rather than assembled.** A texel is a
// fixed cross-section of the sun's own beam — the cascade is orthographic and
// its axes are perpendicular to the light — so the radiant flux passing through
// it is `E · texel_area`, with `E` the sun's colour, whatever the surface behind
// it is tilted at; the flux it reflects is that times the albedo. A Lambertian
// patch of flux `Φ` gives a probe at distance `r` the irradiance
// `Φ · facing / (π r²)`, where `facing` is the cosine between the patch's normal
// and the direction to the probe. So the whole contribution is
//
// ```text
//   albedo · E · texel_area · facing / (π r²)
// ```
//
// and the sun's own cosine `n·l` appears nowhere: a tilted surface spreads that
// flux over a patch `texel_area / (n·l)` in area, and the `n·l` its radiance
// loses is exactly the one its area gains. Both are left out rather than written
// and cancelled, because cancelling them means dividing by a cosine that goes to
// zero at a grazing sun. `accumulate` is linear in the product of its radiance
// and its weight, so what it is handed is a radiance and an area whose product
// is the line above.
//
// # The projection weights are declared, not derived
//
// `PROBE_PROJECT_L0` and `PROBE_PROJECT_L1` are `crcbl_shaders::probe`'s
// `PROJECT_L0` and `PROJECT_L1` — the two numbers `GpuProbe::accumulate`
// multiplies a sample by, and the ones that module's docs call the arithmetic a
// caller doing it by hand would get plausibly wrong. They are written out here
// and pinned to the module's values by
// `the_gather_projects_a_sample_the_way_this_module_does`, on
// `the_visibility_map_constants_match_the_ones_the_shaders_declare`'s terms:
// nothing else in the tree knows what the right answer was.

/// Texels along one side of a visibility map's interior — the part that holds
/// directions. `crcbl_shaders::probe_visibility::SIDE`, and
/// `the_visibility_map_constants_match_the_ones_the_shaders_declare` is what
/// holds this file, `mesh.slang`, `ssr.slang` and that module to one another.
static const float PROBE_VISIBILITY_SIDE = 16.0;

/// Texels of wrapped border on each side of that interior, which is what lets
/// the blend below read four ordinary neighbours everywhere.
/// `crcbl_shaders::probe_visibility::BORDER`.
static const float PROBE_VISIBILITY_BORDER = 1.0;

/// How far along its own normal a shaded point is moved before its distance to a
/// probe is measured, in world units.
/// `crcbl_shaders::probe_visibility::SURFACE_BIAS`, where the reason is argued:
/// without it a floor is exactly as far from the probe above it as the floor
/// that probe recorded, and shadows itself.
static const float PROBE_SURFACE_BIAS = 0.05;

/// The share of its trilinear weight a fully occluded corner keeps.
/// `crcbl_shaders::probe_visibility::OCCLUDED_WEIGHT`, where the reason is
/// argued. **Declared and deliberately unused here** — this file calls
/// `probe_chebyshev` and never `probe_weight`, and the header says why — but the
/// constant is pinned across every file that carries the bound, so the drift
/// check reads one list rather than one list per constant.
static const float PROBE_OCCLUDED_WEIGHT = 0.0001;

/// What one sample of the environment adds to a probe's constant band —
/// `crcbl_shaders::probe::PROJECT_L0`, which is `Â₀ · Y₀₀²`.
static const float PROBE_PROJECT_L0 = 0.25;

/// The same for the linear band — `crcbl_shaders::probe::PROJECT_L1`, which is
/// `Â₁ · (Y₁₁/x)²`.
static const float PROBE_PROJECT_L1 = 0.5;

/// A Lambertian surface's reciprocal normalisation: outgoing radiance is the
/// irradiance it received divided by `π`.
static const float PROBE_INV_PI = 0.3183098861837907;

/// The most one texel's `texel_area · facing / r²` may be credited with.
///
/// That quantity is the texel's solid angle at the probe scaled by the cosine
/// between its surface and the sun — see the header's derivation — so it is
/// bounded above by the hemisphere a patch can subtend, and this is that bound.
/// A bound rather than a fudge: the quotient grows without limit as `r` goes to
/// zero, and a probe standing on a surface would otherwise take the whole room's
/// colour from the one texel it is touching.
static const float PROBE_MAX_SOLID_ANGLE = 6.283185307179586;

/// Invocations per workgroup, one workgroup per probe.
/// `crcbl_shaders::probe_gather::GATHER_WORKGROUP_SIZE`.
static const uint PROBE_GATHER_THREADS = 64;

/// Everything the gather needs that is not a buffer or an image, matching
/// `crcbl_shaders::probe_gather::GatherParams`.
struct GatherParams
{
    /// The sun's colour premultiplied by its intensity in `rgb`, exactly as the
    /// light row carries it. `w` is unread padding.
    float4 sun_color;
    /// The world area one reflective-shadow-map texel covers, in square world
    /// units: the cascade's world footprint over `rsm_side`, squared. Evaluated
    /// on the host, because the cascade's extent is the host's number and this
    /// pass carries no projection to recover it from.
    float texel_area;
    /// Texels along one side of the reflective shadow map —
    /// `crcbl_shaders::probe_gather::RSM_SIDE`.
    uint rsm_side;
    /// Rows the dispatch covers, which is the volume's total probe count. A
    /// group past it writes nothing.
    uint probes;
    /// Padding to a `float4` boundary; unread.
    uint reserved;
};

[[vk::binding(0, 0)]]
ConstantBuffer<GatherParams> params;

/// Where each probe stands, in world space, `xyz`; `w` is unread padding.
///
/// **Evaluated on the host and handed in as data**, on `probe_octahedral.slang`'s
/// terms exactly: `crcbl_shaders::probe::ProbeVolume::position` owns the clipmap
/// arithmetic that turns a level and a cell into a place, and a second
/// transcription of it in Slang would be a second thing that can drift from the
/// volume it is supposed to agree with. One entry per row of the table, in the
/// table's own order.
[[vk::binding(1, 0)]]
StructuredBuffer<float4> probe_positions;

/// The captured per-probe visibility maps, one layer per row of the table —
/// `crcbl_render::probe_capture` writes them and `mesh.slang` reads them with
/// this same name. `Rg32Float`, read with `Load` and no sampler, which is what
/// an unfilterable format allows.
[[vk::binding(2, 0)]]
Texture2DArray<float4> probe_visibility;

/// The reflective shadow map's diffuse albedo — `RsmOutput::albedo`.
[[vk::binding(3, 0)]]
Texture2D<float4> rsm_albedo;

/// Its world normals, encoded `n * 0.5 + 0.5` — `RsmOutput::normal`.
[[vk::binding(4, 0)]]
Texture2D<float4> rsm_normal;

/// Its world positions in `xyz`, with the coverage flag in `w` —
/// `RsmOutput::world`.
[[vk::binding(5, 0)]]
Texture2D<float4> rsm_world;

/// The probe table, three `float4`s a row: `sh_r`, `sh_g`, `sh_b` in that order,
/// which is `crcbl_shaders::probe::GpuProbe`'s layout and its `to_bytes` order.
///
/// A `float4` array rather than a struct, so the write is three stores of a
/// natural vector on every backend and no `std430` struct stride has to agree
/// between four compilers and one Rust serialiser.
[[vk::binding(6, 0)]]
RWStructuredBuffer<float4> probes;

/// `+1` for a positive or zero value and `-1` for a negative one — the
/// octahedral mapping's `signNotZero`.
///
/// Not `sign`, which answers zero for zero: the fold below multiplies by this,
/// and a zero factor would collapse a direction lying in an axis plane onto the
/// square's centre.
float sign_not_zero(float value)
{
    return value >= 0.0 ? 1.0 : -1.0;
}

/// A unit direction as a point of the octahedral square, each component in
/// `-1..1` — `crcbl_shaders::probe_visibility::oct_encode`.
///
/// This body is character for character `mesh.slang`'s and `ssr.slang`'s; the
/// header says which test holds the three together.
float2 oct_encode(float3 direction)
{
    float norm = abs(direction.x) + abs(direction.y) + abs(direction.z);
    float2 p = direction.xz / max(norm, 1e-20);
    if (direction.y < 0.0)
    {
        p = float2((1.0 - abs(p.y)) * sign_not_zero(p.x),
                   (1.0 - abs(p.x)) * sign_not_zero(p.y));
    }
    return p;
}

/// The two depth moments probe `index`'s map holds along `direction`, blended
/// bilinearly — `crcbl_shaders::probe_visibility::ProbeVisibility::moments`.
float2 probe_moments(uint index, float3 direction)
{
    uint width;
    uint height;
    uint layers;
    probe_visibility.GetDimensions(width, height, layers);
    float2 extent = float2(float(width), float(height));
    float2 scaled = (oct_encode(direction) * 0.5 + 0.5) * PROBE_VISIBILITY_SIDE
                  + PROBE_VISIBILITY_BORDER - 0.5;
    float2 low = clamp(floor(scaled), float2(0.0, 0.0), extent - 1.0);
    float2 high = min(low + 1.0, extent - 1.0);
    float2 weight = clamp(scaled - low, 0.0, 1.0);
    int layer = int(min(index, max(layers, 1u) - 1u));

    float2 c00 = probe_visibility.Load(int4(int(low.x), int(low.y), layer, 0)).xy;
    float2 c10 = probe_visibility.Load(int4(int(high.x), int(low.y), layer, 0)).xy;
    float2 c01 = probe_visibility.Load(int4(int(low.x), int(high.y), layer, 0)).xy;
    float2 c11 = probe_visibility.Load(int4(int(high.x), int(high.y), layer, 0)).xy;
    return lerp(lerp(c00, c10, weight.x), lerp(c01, c11, weight.x), weight.y);
}

/// The Chebyshev bound on the probe at `probe_position`'s visibility of a
/// surface at `world_position` facing `normal` — `probe_weight` without its
/// floor.
///
/// **Without the floor is the whole reason this file calls it**, and the header
/// argues why: a gather with no divisor must not pay `PROBE_OCCLUDED_WEIGHT`.
float probe_chebyshev(uint index, float3 probe_position, float3 world_position, float3 normal)
{
    float3 to_probe = probe_position - (world_position + normal * PROBE_SURFACE_BIAS);
    float to_surface = length(to_probe);
    // The map is indexed by the direction the probe looks *out* along.
    float2 moments = probe_moments(index, -to_probe);
    // Clamped because the two moments are blended independently and a texel
    // between two disagreeing neighbours can land a hair under `mean²`, which
    // is a negative variance and a visibility above one.
    float variance = max(moments.y - moments.x * moments.x, 0.0);
    float behind = to_surface - moments.x;
    float bound = variance / (variance + behind * behind);
    return to_surface <= moments.x ? 1.0 : bound * bound * bound;
}

/// One thread's running sum of the three channels' four coefficients, in
/// `GpuProbe`'s own `(L1x, L1y, L1z, L0)` packing.
struct Bands
{
    float4 r;
    float4 g;
    float4 b;
};

/// Adds one sample of the environment to `bands` — `GpuProbe::accumulate`'s
/// body, written in the packing the row is stored in.
///
/// `direction` is a unit vector pointing from the probe *towards* where the
/// light comes from, `radiance` is the linear RGB radiance arriving along it,
/// and `solid_angle` is how much of the sphere the sample stands for.
void accumulate(inout Bands bands, float3 direction, float3 radiance, float solid_angle)
{
    float4 basis = float4(direction * (solid_angle * PROBE_PROJECT_L1),
                          solid_angle * PROBE_PROJECT_L0);
    bands.r += basis * radiance.r;
    bands.g += basis * radiance.g;
    bands.b += basis * radiance.b;
}

/// One slot per thread of the reduction below, three `float4`s each.
///
/// A tree reduction rather than an atomic add: there are no float atomics at
/// this seam, and the order of a tree over fixed lanes is the same order on
/// every run — which is what a golden of this pass rests on.
groupshared Bands tile[PROBE_GATHER_THREADS];

[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 group: SV_GroupID, uint3 thread: SV_GroupThreadID)
{
    // **One group is one probe**, not `div_ceil` of anything — the header says
    // why. The guard is not an early return: every lane below reaches the same
    // barriers, and a workgroup that leaves through one of them while its
    // neighbours wait is undefined on every backend here.
    uint probe = group.x;
    uint lane = thread.x;

    Bands bands;
    bands.r = float4(0.0, 0.0, 0.0, 0.0);
    bands.g = float4(0.0, 0.0, 0.0, 0.0);
    bands.b = float4(0.0, 0.0, 0.0, 0.0);

    if (probe < params.probes)
    {
        float3 probe_position = probe_positions[probe].xyz;
        uint side = max(params.rsm_side, 1u);
        uint texels = side * side;
        for (uint index = lane; index < texels; index += PROBE_GATHER_THREADS)
        {
            uint row = index / side;
            uint column = index - row * side;
            int3 at = int3(int(column), int(row), 0);

            float4 world = rsm_world.Load(at);
            // `w` is the coverage flag `RsmOutput::world` writes: the attachment
            // is cleared to zero, so a texel the pass drew nothing into is a
            // texel with no patch in it rather than one at the world origin.
            if (world.w <= 0.0)
            {
                continue;
            }
            float3 sample_position = world.xyz;
            // Normalised rather than trusted: the encode is `n * 0.5 + 0.5` into
            // an eight-bit target, so what comes back is the direction to within
            // a quantisation step and not a unit vector.
            float3 sample_normal = normalize(rsm_normal.Load(at).xyz * 2.0 - 1.0);

            float3 offset = sample_position - probe_position;
            float distance_squared = dot(offset, offset);
            // A probe standing exactly on a patch has no direction to that
            // patch. `PROBE_MAX_SOLID_ANGLE` bounds what a merely *close* one
            // may take; this is the degenerate case that has no direction at all.
            if (distance_squared <= 1e-12)
            {
                continue;
            }
            float distance = sqrt(distance_squared);
            float3 direction = offset / distance;

            // How much of itself the patch shows the probe. A patch turned away
            // reflects nothing towards it, and its solid angle there is zero
            // rather than negative.
            float facing = dot(sample_normal, -direction);
            if (facing <= 0.0)
            {
                continue;
            }

            // The whole point of the pass: a patch the probe's own visibility
            // map reports as behind a wall contributes nothing at all.
            float visibility = probe_chebyshev(probe, probe_position, sample_position,
                                               sample_normal);
            if (visibility <= 0.0)
            {
                continue;
            }

            // **The sun's own cosine is deliberately absent** — the header's
            // derivation is why, and it is the one thing here a reader will
            // expect to see and must not.
            float3 radiance = rsm_albedo.Load(at).rgb * params.sun_color.rgb * PROBE_INV_PI;
            float weight =
                min(params.texel_area * facing / distance_squared, PROBE_MAX_SOLID_ANGLE);
            accumulate(bands, direction, radiance * visibility, weight);
        }
    }

    tile[lane] = bands;
    GroupMemoryBarrierWithGroupSync();
    // The tree, halving each round. `PROBE_GATHER_THREADS` is a power of two, so
    // no round has an odd lane to carry.
    for (uint stride = PROBE_GATHER_THREADS / 2u; stride > 0u; stride >>= 1u)
    {
        if (lane < stride)
        {
            tile[lane].r += tile[lane + stride].r;
            tile[lane].g += tile[lane + stride].g;
            tile[lane].b += tile[lane + stride].b;
        }
        GroupMemoryBarrierWithGroupSync();
    }

    if (lane == 0u && probe < params.probes)
    {
        probes[probe * 3u + 0u] = tile[0].r;
        probes[probe * 3u + 1u] = tile[0].g;
        probes[probe * 3u + 2u] = tile[0].b;
    }
}
