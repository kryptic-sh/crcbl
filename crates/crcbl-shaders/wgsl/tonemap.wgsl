@binding(0) @group(0) var scene_0 : texture_2d<f32>;

@binding(1) @group(0) var sceneSampler_0 : sampler;

struct TonemapParams_std140_0
{
    @align(16) exposure_0 : f32,
    @align(4) curve_0 : u32,
};

@binding(2) @group(0) var<uniform> params_0 : TonemapParams_std140_0;
struct FullscreenOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) uv_0 : vec2<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> FullscreenOutput_0
{
    var output_0 : FullscreenOutput_0;
    var _S1 : vec2<f32> = vec2<f32>(f32((((index_0 << (u32(1)))) & (u32(2)))), f32((index_0 & (u32(2)))));
    output_0.uv_0 = _S1;
    output_0.position_0 = vec4<f32>(_S1 * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f), 0.0f, 1.0f);
    return output_0;
}

fn rrt_and_odt_fit_0( v_0 : vec3<f32>) -> vec3<f32>
{
    return (v_0 * (v_0 + vec3<f32>(0.02457859925925732f)) - vec3<f32>(0.0000905370034161f)) / (v_0 * (vec3<f32>(0.98372900485992432f) * v_0 + vec3<f32>(0.43295100331306458f)) + vec3<f32>(0.23808099329471588f));
}

fn tonemap_0( color_0 : vec3<f32>,  exposure_1 : f32,  curve_1 : u32) -> vec3<f32>
{
    var exposed_0 : vec3<f32> = color_0 * vec3<f32>(exposure_1);
    if(curve_1 == u32(1))
    {
        return saturate((((rrt_and_odt_fit_0((((exposed_0) * (mat3x3<f32>(0.59719002246856689f, 0.35457998514175415f, 0.04822999984025955f, 0.07599999755620956f, 0.9083399772644043f, 0.01565999910235405f, 0.0284000001847744f, 0.1338299959897995f, 0.83776998519897461f)))))) * (mat3x3<f32>(1.60475003719329834f, -0.53108000755310059f, -0.07366999983787537f, -0.10208000242710114f, 1.10812997817993164f, -0.00604999996721745f, -0.00326999998651445f, -0.07276000082492828f, 1.0760200023651123f)))));
    }
    return saturate(exposed_0);
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_1 : vec2<f32>,
};

@fragment
fn fragmentMain( _S2 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S3 : pixelOutput_0 = pixelOutput_0( vec4<f32>(tonemap_0((textureSample((scene_0), (sceneSampler_0), (_S2.uv_1))).xyz, params_0.exposure_0, params_0.curve_0), 1.0f) );
    return _S3;
}

