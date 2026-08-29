struct SmaaParams_std140_0
{
    @align(16) inv_source_0 : vec2<f32>,
    @align(8) source_size_0 : vec2<f32>,
};

@binding(2) @group(0) var<uniform> params_0 : SmaaParams_std140_0;
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

fn luma_of_0( color_0 : vec3<f32>) -> f32
{
    return sqrt(dot(color_0, vec3<f32>(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f)));
}

fn luma_at_0( uv_1 : vec2<f32>,  offset_0 : vec2<f32>) -> f32
{
    return luma_of_0((textureSampleLevel((source_0), (sourceSampler_0), (uv_1 + offset_0 * params_0.inv_source_0), (0.0f))).xyz);
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
    var _S3 : vec2<f32> = vec2<f32>(luma_at_0(_S2.uv_2, vec2<f32>(-1.0f, 0.0f)), luma_at_0(_S2.uv_2, vec2<f32>(0.0f, -1.0f)));
    var _S4 : vec2<f32> = vec2<f32>(luma_at_0(_S2.uv_2, vec2<f32>(0.0f, 0.0f)));
    var delta_0 : vec2<f32> = abs(_S4 - _S3);
    var edges_0 : vec2<f32> = step(vec2<f32>(0.10000000149011612f, 0.10000000149011612f), delta_0);
    if((edges_0.x + edges_0.y) == 0.0f)
    {
        var _S5 : pixelOutput_0 = pixelOutput_0( vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f) );
        return _S5;
    }
    var max_delta_0 : vec2<f32> = max(max(delta_0, abs(_S4 - vec2<f32>(luma_at_0(_S2.uv_2, vec2<f32>(1.0f, 0.0f)), luma_at_0(_S2.uv_2, vec2<f32>(0.0f, 1.0f))))), abs(_S3 - vec2<f32>(luma_at_0(_S2.uv_2, vec2<f32>(-2.0f, 0.0f)), luma_at_0(_S2.uv_2, vec2<f32>(0.0f, -2.0f)))));
    var _S6 : f32 = max(max_delta_0.x, max_delta_0.y);
    var _S7 : pixelOutput_0 = pixelOutput_0( vec4<f32>(edges_0 * step(vec2<f32>(_S6, _S6), vec2<f32>(2.0f) * delta_0), 0.0f, 1.0f) );
    return _S7;
}

