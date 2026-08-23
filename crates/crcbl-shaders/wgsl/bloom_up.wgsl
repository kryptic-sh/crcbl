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

fn tent_0( uv_2 : vec2<f32>) -> vec3<f32>
{
    var _S2 : vec3<f32> = vec3<f32>(2.0f);
    return (tap_0(uv_2, vec2<f32>(-1.0f, 1.0f)) + tap_0(uv_2, vec2<f32>(0.0f, 1.0f)) * _S2 + tap_0(uv_2, vec2<f32>(1.0f, 1.0f)) + tap_0(uv_2, vec2<f32>(-1.0f, 0.0f)) * _S2 + tap_0(uv_2, vec2<f32>(0.0f, 0.0f)) * vec3<f32>(4.0f) + tap_0(uv_2, vec2<f32>(1.0f, 0.0f)) * _S2 + tap_0(uv_2, vec2<f32>(-1.0f, -1.0f)) + tap_0(uv_2, vec2<f32>(0.0f, -1.0f)) * _S2 + tap_0(uv_2, vec2<f32>(1.0f, -1.0f))) * vec3<f32>(0.0625f);
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_3 : vec2<f32>,
};

@fragment
fn fragmentMain( _S3 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S4 : pixelOutput_0 = pixelOutput_0( vec4<f32>(tent_0(_S3.uv_3), 0.0f) );
    return _S4;
}

