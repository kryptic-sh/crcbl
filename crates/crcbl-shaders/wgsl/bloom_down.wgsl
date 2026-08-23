struct BloomParams_std140_0
{
    @align(16) inv_source_0 : vec2<f32>,
    @align(8) karis_0 : f32,
    @align(4) strength_0 : f32,
};

@binding(2) @group(0) var<uniform> params_0 : BloomParams_std140_0;
@binding(0) @group(0) var source_0 : texture_2d<f32>;

@binding(1) @group(0) var sourceSampler_0 : sampler;

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

fn tap_0( uv_1 : vec2<f32>,  offset_0 : vec2<f32>) -> vec3<f32>
{
    return (textureSample((source_0), (sourceSampler_0), (uv_1 + offset_0 * params_0.inv_source_0))).xyz;
}

fn luma_0( color_0 : vec3<f32>) -> f32
{
    return dot(color_0, vec3<f32>(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f));
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_2 : vec2<f32>,
};

@fragment
fn fragmentMain( _S2 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var b_0 : vec3<f32> = tap_0(_S2.uv_2, vec2<f32>(0.0f, 2.0f));
    var d_0 : vec3<f32> = tap_0(_S2.uv_2, vec2<f32>(-2.0f, 0.0f));
    var e_0 : vec3<f32> = tap_0(_S2.uv_2, vec2<f32>(0.0f, 0.0f));
    var f_0 : vec3<f32> = tap_0(_S2.uv_2, vec2<f32>(2.0f, 0.0f));
    var h_0 : vec3<f32> = tap_0(_S2.uv_2, vec2<f32>(0.0f, -2.0f));
    var _S3 : vec3<f32> = vec3<f32>(0.25f);
    var g0_0 : vec3<f32> = (tap_0(_S2.uv_2, vec2<f32>(-2.0f, 2.0f)) + b_0 + d_0 + e_0) * _S3;
    var g1_0 : vec3<f32> = (b_0 + tap_0(_S2.uv_2, vec2<f32>(2.0f, 2.0f)) + e_0 + f_0) * _S3;
    var g2_0 : vec3<f32> = (d_0 + e_0 + tap_0(_S2.uv_2, vec2<f32>(-2.0f, -2.0f)) + h_0) * _S3;
    var g3_0 : vec3<f32> = (e_0 + f_0 + h_0 + tap_0(_S2.uv_2, vec2<f32>(2.0f, -2.0f))) * _S3;
    var g4_0 : vec3<f32> = (tap_0(_S2.uv_2, vec2<f32>(-1.0f, 1.0f)) + tap_0(_S2.uv_2, vec2<f32>(1.0f, 1.0f)) + tap_0(_S2.uv_2, vec2<f32>(-1.0f, -1.0f)) + tap_0(_S2.uv_2, vec2<f32>(1.0f, -1.0f))) * _S3;
    var w0_0 : f32 = 0.125f / (1.0f + params_0.karis_0 * luma_0(g0_0));
    var w1_0 : f32 = 0.125f / (1.0f + params_0.karis_0 * luma_0(g1_0));
    var w2_0 : f32 = 0.125f / (1.0f + params_0.karis_0 * luma_0(g2_0));
    var w3_0 : f32 = 0.125f / (1.0f + params_0.karis_0 * luma_0(g3_0));
    var w4_0 : f32 = 0.5f / (1.0f + params_0.karis_0 * luma_0(g4_0));
    var _S4 : pixelOutput_0 = pixelOutput_0( vec4<f32>((g0_0 * vec3<f32>(w0_0) + g1_0 * vec3<f32>(w1_0) + g2_0 * vec3<f32>(w2_0) + g3_0 * vec3<f32>(w3_0) + g4_0 * vec3<f32>(w4_0)) / vec3<f32>((w0_0 + w1_0 + w2_0 + w3_0 + w4_0)), 1.0f) );
    return _S4;
}

