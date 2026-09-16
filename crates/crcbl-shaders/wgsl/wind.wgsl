struct WindProbeParams_std140_0
{
    @align(16) count_0 : u32,
};

@binding(0) @group(1) var<uniform> probe_0 : WindProbeParams_std140_0;
@binding(2) @group(1) var<storage, read_write> velocities_0 : array<vec4<f32>>;

@binding(1) @group(1) var<storage, read> points_0 : array<vec4<f32>>;

struct WindParams_std140_0
{
    @align(16) baseDirection_0 : vec2<f32>,
    @align(8) baseSpeed_0 : f32,
    @align(4) gustAmplitude_0 : f32,
    @align(16) gustPhase_0 : f32,
    @align(4) invGustWavelength_0 : f32,
    @align(8) pad0_0 : vec2<f32>,
    @align(16) directionUv_0 : vec2<f32>,
    @align(8) directionUvPerMetre_0 : vec2<f32>,
    @align(16) intensityUv_0 : vec2<f32>,
    @align(8) intensityUvPerMetre_0 : vec2<f32>,
};

@binding(0) @group(0) var<uniform> wind_0 : WindParams_std140_0;
@binding(1) @group(0) var windDirectionLayer_0 : texture_2d<f32>;

@binding(3) @group(0) var windSampler_0 : sampler;

@binding(2) @group(0) var windIntensityLayer_0 : texture_2d<f32>;

fn windSmoothTriangle_0( u_0 : f32) -> f32
{
    var s_0 : f32 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}

fn windSample_0( posRel_0 : vec3<f32>) -> vec3<f32>
{
    var _S1 : vec2<f32> = posRel_0.xz;
    var deflection_0 : vec2<f32> = (textureSampleLevel((windDirectionLayer_0), (windSampler_0), (wind_0.directionUv_0 + _S1 * wind_0.directionUvPerMetre_0), (0.0f))).xy * vec2<f32>(2.0f) - vec2<f32>(1.0f);
    var intensity_0 : f32 = (textureSampleLevel((windIntensityLayer_0), (windSampler_0), (wind_0.intensityUv_0 + _S1 * wind_0.intensityUvPerMetre_0), (0.0f))).x;
    var base_0 : vec2<f32> = wind_0.baseDirection_0;
    var _S2 : f32 = wind_0.baseDirection_0.x;
    var _S3 : f32 = deflection_0.x;
    var _S4 : f32 = wind_0.baseDirection_0.y;
    var _S5 : f32 = deflection_0.y;
    var turned_0 : vec2<f32> = vec2<f32>(_S2 * _S3 - _S4 * _S5, _S2 * _S5 + _S4 * _S3);
    var lengthSquared_0 : f32 = dot(turned_0, turned_0);
    var direction_0 : vec2<f32>;
    if(lengthSquared_0 > 9.999999960041972e-13f)
    {
        direction_0 = turned_0 / vec2<f32>(sqrt(lengthSquared_0));
    }
    else
    {
        direction_0 = base_0;
    }
    var speed_0 : f32 = intensity_0 * wind_0.baseSpeed_0 * (1.0f + wind_0.gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S1, base_0) * wind_0.invGustWavelength_0 + wind_0.gustPhase_0) - 1.0f));
    return vec3<f32>(direction_0.x * speed_0, 0.0f, direction_0.y * speed_0);
}

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 >= (probe_0.count_0))
    {
        return;
    }
    velocities_0[index_0] = vec4<f32>(windSample_0(points_0[index_0].xyz), 0.0f);
    return;
}

