struct SmaaParams_std140_0
{
    @align(16) inv_source_0 : vec2<f32>,
    @align(8) source_size_0 : vec2<f32>,
};

@binding(3) @group(0) var<uniform> params_0 : SmaaParams_std140_0;
@binding(1) @group(0) var blend_0 : texture_2d<f32>;

@binding(2) @group(0) var sourceSampler_0 : sampler;

@binding(0) @group(0) var source_0 : texture_2d<f32>;

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
    var texel_0 : vec2<f32> = params_0.inv_source_0;
    var _S3 : vec4<f32> = vec4<f32>(_S2.uv_1, _S2.uv_1);
    var offset_0 : vec4<f32> = vec4<f32>(1.0f, 0.0f, 0.0f, 1.0f) * vec4<f32>(params_0.inv_source_0, params_0.inv_source_0) + _S3;
    var a_0 : vec4<f32>;
    a_0[i32(0)] = (textureSampleLevel((blend_0), (sourceSampler_0), (offset_0.xy), (0.0f))).w;
    a_0[i32(1)] = (textureSampleLevel((blend_0), (sourceSampler_0), (offset_0.zw), (0.0f))).y;
    var own_0 : vec4<f32> = (textureSampleLevel((blend_0), (sourceSampler_0), (_S2.uv_1), (0.0f)));
    a_0[i32(3)] = own_0.x;
    a_0[i32(2)] = own_0.z;
    if((dot(a_0, vec4<f32>(1.0f, 1.0f, 1.0f, 1.0f))) < 0.00000999999974738f)
    {
        var _S4 : pixelOutput_0 = pixelOutput_0( vec4<f32>((textureSampleLevel((source_0), (sourceSampler_0), (_S2.uv_1), (0.0f))).xyz, 1.0f) );
        return _S4;
    }
    var _S5 : vec4<f32> = vec4<f32>(0.0f, a_0.y, 0.0f, a_0.w);
    var _S6 : vec2<f32> = vec2<f32>(a_0.y, a_0.w);
    var blending_offset_0 : vec4<f32>;
    var blending_weight_0 : vec2<f32>;
    if((max(a_0.x, a_0.z)) > (max(a_0.y, a_0.w)))
    {
        var _S7 : vec2<f32> = vec2<f32>(a_0.x, a_0.z);
        blending_offset_0 = vec4<f32>(a_0.x, 0.0f, a_0.z, 0.0f);
        blending_weight_0 = _S7;
    }
    else
    {
        blending_offset_0 = _S5;
        blending_weight_0 = _S6;
    }
    var blending_weight_1 : vec2<f32> = blending_weight_0 / vec2<f32>(dot(blending_weight_0, vec2<f32>(1.0f, 1.0f)));
    var blending_coord_0 : vec4<f32> = blending_offset_0 * vec4<f32>(texel_0, (vec2<f32>(0) - texel_0)) + _S3;
    var _S8 : pixelOutput_0 = pixelOutput_0( vec4<f32>(vec3<f32>(blending_weight_1.x) * (textureSampleLevel((source_0), (sourceSampler_0), (blending_coord_0.xy), (0.0f))).xyz + vec3<f32>(blending_weight_1.y) * (textureSampleLevel((source_0), (sourceSampler_0), (blending_coord_0.zw), (0.0f))).xyz, 1.0f) );
    return _S8;
}

