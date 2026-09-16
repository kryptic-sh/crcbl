// crcbl-targets: spirv, wgsl, msl, dxil

// The GPU copy of the wind field — `docs/plan/56-wind.md`'s decision 6.
//
// The CPU copy in `crcbl-wind` is authoritative and is what physics, audio and
// CPU particles read; this is the same formula for the things that are only
// drawn. It is not bit-identical to the CPU's and is not meant to be: a shader
// compiler may contract a multiply and an add into an FMA, and an eight-bit
// texture's bilinear filter has hardware-defined sub-texel precision. What is
// held is agreement within `crcbl_shaders::wind::MAX_CPU_GPU_ERROR`, measured
// by `crates/crcbl/tests/render_e2e/wind.rs` on every backend.
//
// # One bind group, and every texture sampled
//
// Set 0 is the wind: the uniform block, the two authored layers and one
// sampler. Nothing here is a storage texture, so WebGPU's four storage textures
// per stage are untouched — decision 6's constraint, and the reason the layers
// are read through `SampleLevel` rather than `Load`ed and filtered by hand.
//
// Set 1 belongs to whoever is asking. At this rung the only caller is the
// agreement probe, which hands over a list of camera-relative positions and
// takes back a velocity each.
//
// # No transcendental, and nothing world-scale
//
// Decision 9 bans the platform's `sin`, `cos` and `exp` from anything that
// reaches a pixel. Nothing here needs one: the gust front is decision 3's
// Crysis construction, a smoothstep over a triangle wave, and the direction is
// a vector composed by a complex product rather than an angle turned by
// `atan2`. The one square root is decision 1's renormalise, and it is a real
// `sqrt` rather than `rsqrt` because `rsqrt` is an approximation on several
// targets and the tolerance above is not there to pay for one.
//
// Decision 3's other half is the offset: positions arrive relative to the
// camera and the gust phase arrives already reduced into `0..1`, so no `float`
// in this file ever holds a world-scale coordinate.

/// The wind's uniform block, matching `crcbl_shaders::wind::WindParams`.
struct WindParams
{
    /// Unit horizontal direction the wind blows towards, on the XZ plane.
    float2 baseDirection;
    /// Base speed in m/s — `S` in decision 6's formula.
    float baseSpeed;
    /// Peak fractional swing of the gust about one. Zero is no gusting.
    float gustAmplitude;
    /// `dot(camera - o, baseDirection) / wavelength`, whole turns dropped.
    float gustPhase;
    /// One over the gust wavelength, in reciprocal metres.
    float invGustWavelength;
    /// Written as zero and unread; the row `std140` leaves.
    float2 pad0;
    /// The camera's texture coordinate in the direction layer, wrapped.
    float2 directionUv;
    /// Direction-layer texture coordinates per metre of world.
    float2 directionUvPerMetre;
    /// The camera's texture coordinate in the intensity layer, wrapped.
    float2 intensityUv;
    /// Intensity-layer texture coordinates per metre of world.
    float2 intensityUvPerMetre;
};

[[vk::binding(0, 0)]]
ConstantBuffer<WindParams> wind;

/// The coarse direction layer: a deflection from `baseDirection` per texel, in
/// red and green over `-1..1`. Blue and alpha are unread.
[[vk::binding(1, 0)]]
Texture2D<float4> windDirectionLayer;

/// The fine intensity layer: a speed multiplier per texel, in red. Zero is
/// calm, and calm stays calm however hard the gust blows, because the gust is a
/// factor on this and not a term beside it.
[[vk::binding(2, 0)]]
Texture2D<float4> windIntensityLayer;

/// Linear filtering, `repeat` on both axes — the addressing the CPU copy's
/// wrapped texel indices are the statement of.
[[vk::binding(3, 0)]]
SamplerState windSampler;

/// What the agreement probe needs that is not buffer contents, matching
/// `crcbl_shaders::wind::ProbeParams`.
struct WindProbeParams
{
    /// Positions in `points`, and velocities in `velocities`. A dispatch is a
    /// whole number of workgroups, so the last one runs past the end whenever
    /// this is not a multiple of the workgroup size.
    uint count;
};

[[vk::binding(0, 1)]]
ConstantBuffer<WindProbeParams> probe;

/// Camera-relative sample positions in `xyz`; `w` is unread.
[[vk::binding(1, 1)]]
StructuredBuffer<float4> points;

/// One wind velocity per point in `xyz`, in m/s; `w` is written as zero.
[[vk::binding(2, 1)]]
RWStructuredBuffer<float4> velocities;

/// Below this squared length a composed direction has no direction left, and
/// the base direction answers instead. Matches
/// `crcbl_shaders::wind::MIN_DIRECTION_LENGTH_SQUARED`.
static const float WIND_MIN_DIRECTION_LENGTH_SQUARED = 1e-12;

/// Crysis's smoothed triangle wave: period one, range `0..1`, `C¹` throughout.
///
/// `docs/plan/56-wind.md`'s decision 3 states it as "`x·x·(3 − 2x)` over
/// `abs(frac(x + 0.5)·2 − 1)`". `frac` here is `x - floor(x)` on every target
/// this compiles to, which is the same non-negative convention
/// `crcbl_wind::layers::fract` is written to hold.
float windSmoothTriangle(float u)
{
    float s = abs(frac(u + 0.5) * 2.0 - 1.0);
    return s * s * (3.0 - 2.0 * s);
}

/// The wind velocity at a camera-relative position, in m/s.
///
/// Decision 6's first term, `I(x) · S · D(x) · (1 + gust(x − o))`. The motor
/// list (rung W3) and the wake grid (rung W5) are the two terms beside it, and
/// neither is here: a term that is always zero reads as a term that works.
float3 windSample(float3 posRel)
{
    float2 deflection =
        windDirectionLayer
            .SampleLevel(windSampler, wind.directionUv + posRel.xz * wind.directionUvPerMetre, 0.0)
            .rg
            * 2.0
        - 1.0;
    float intensity =
        windIntensityLayer
            .SampleLevel(windSampler, wind.intensityUv + posRel.xz * wind.intensityUvPerMetre, 0.0)
            .r;

    // `D(x)`: the prevailing direction turned by the layer's deflection, as the
    // product of the complex numbers the two stand for, then renormalised.
    float2 base = wind.baseDirection;
    float2 turned = float2(base.x * deflection.x - base.y * deflection.y,
                           base.x * deflection.y + base.y * deflection.x);
    float lengthSquared = dot(turned, turned);
    float2 direction = lengthSquared > WIND_MIN_DIRECTION_LENGTH_SQUARED
                           ? turned / sqrt(lengthSquared)
                           : base;

    float along = dot(posRel.xz, base) * wind.invGustWavelength + wind.gustPhase;
    float gust = 1.0 + wind.gustAmplitude * (2.0 * windSmoothTriangle(along) - 1.0);
    float speed = intensity * wind.baseSpeed * gust;
    return float3(direction.x * speed, 0.0, direction.y * speed);
}

/// One invocation per point. `crcbl_shaders::wind::WORKGROUP_SIZE` carries the
/// same number, and that module's test reads this file to check the two have
/// not drifted.
[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint index = thread.x;
    if (index >= probe.count)
    {
        return;
    }

    velocities[index] = float4(windSample(points[index].xyz), 0.0);
}
