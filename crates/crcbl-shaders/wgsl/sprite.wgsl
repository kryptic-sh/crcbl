struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct SpriteConstants_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) viewport_0 : vec2<f32>,
    @align(8) base_0 : u32,
    @align(4) pad_0 : u32,
};

@binding(0) @group(0) var<uniform> constants_0 : SpriteConstants_std140_0;
struct SpriteInstance_std430_0
{
    @align(16) rect_0 : vec4<f32>,
    @align(16) uv_0 : vec4<f32>,
    @align(16) tint_0 : vec4<f32>,
    @align(16) sheet_0 : vec4<f32>,
};

@binding(1) @group(0) var<storage, read> sprites_0 : array<SpriteInstance_std430_0>;

@binding(0) @group(1) var sheet_1 : texture_2d<f32>;

@binding(1) @group(1) var sheetSampler_0 : sampler;

var<private> CORNERS_0 : array<vec2<f32>, i32(6)> = array<vec2<f32>, i32(6)>( vec2<f32>(0.0f, 0.0f), vec2<f32>(1.0f, 0.0f), vec2<f32>(0.0f, 1.0f), vec2<f32>(0.0f, 1.0f), vec2<f32>(1.0f, 0.0f), vec2<f32>(1.0f, 1.0f) );
struct SpriteVarying_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) uv_1 : vec2<f32>,
    @location(2) tint_1 : vec4<f32>,
    @interpolate(flat) @location(1) sheet_2 : vec3<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) vertex_0 : u32, @builtin(instance_index) instance_0 : u32) -> SpriteVarying_0
{
    var s_0 : SpriteInstance_std430_0 = sprites_0[instance_0 + constants_0.base_0];
    var uv_2 : vec2<f32> = vec2<f32>(mix(s_0.uv_0.x, s_0.uv_0.z, CORNERS_0[vertex_0].x), mix(s_0.uv_0.w, s_0.uv_0.y, CORNERS_0[vertex_0].y));
    var _S1 : vec4<f32> = (((vec4<f32>(s_0.rect_0.xy + CORNERS_0[vertex_0] * s_0.rect_0.zw, 0.0f, 1.0f)) * (mat4x4<f32>(constants_0.view_proj_0.data_0[i32(0)][i32(0)], constants_0.view_proj_0.data_0[i32(1)][i32(0)], constants_0.view_proj_0.data_0[i32(2)][i32(0)], constants_0.view_proj_0.data_0[i32(3)][i32(0)], constants_0.view_proj_0.data_0[i32(0)][i32(1)], constants_0.view_proj_0.data_0[i32(1)][i32(1)], constants_0.view_proj_0.data_0[i32(2)][i32(1)], constants_0.view_proj_0.data_0[i32(3)][i32(1)], constants_0.view_proj_0.data_0[i32(0)][i32(2)], constants_0.view_proj_0.data_0[i32(1)][i32(2)], constants_0.view_proj_0.data_0[i32(2)][i32(2)], constants_0.view_proj_0.data_0[i32(3)][i32(2)], constants_0.view_proj_0.data_0[i32(0)][i32(3)], constants_0.view_proj_0.data_0[i32(1)][i32(3)], constants_0.view_proj_0.data_0[i32(2)][i32(3)], constants_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    var clip_0 : vec4<f32> = _S1;
    var pixelMode_0 : f32 = s_0.sheet_0.z;
    var viewport_1 : vec2<f32> = max(constants_0.viewport_0, vec2<f32>(1.0f, 1.0f));
    if((_S1.w) > 0.0f)
    {
        var _S2 : vec2<f32> = vec2<f32>(0.5f);
        var pixels_0 : vec2<f32> = (clip_0.xy / vec2<f32>(clip_0.w) * _S2 + _S2) * viewport_1;
        var _S3 : vec2<f32> = (mix(pixels_0, round(pixels_0), vec2<f32>(pixelMode_0)) / viewport_1 * vec2<f32>(2.0f) - vec2<f32>(1.0f)) * vec2<f32>(clip_0.w);
        clip_0.x = _S3.x;
        clip_0.y = _S3.y;
    }
    var output_0 : SpriteVarying_0;
    output_0.position_0 = clip_0;
    output_0.uv_1 = uv_2;
    output_0.tint_1 = s_0.tint_0;
    output_0.sheet_2 = s_0.sheet_0.xyz;
    return output_0;
}

fn sharpen_0( uv_3 : vec2<f32>,  size_0 : vec2<f32>) -> vec2<f32>
{
    var _S4 : vec2<f32> = vec2<f32>(0.5f);
    var s_1 : vec2<f32> = uv_3 * size_0 - _S4;
    var whole_0 : vec2<f32> = floor(s_1);
    return (whole_0 + saturate((s_1 - whole_0 - _S4) / max((fwidth((s_1))), vec2<f32>(9.99999997475242708e-07f, 9.99999997475242708e-07f)) + _S4) + _S4) / size_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_4 : vec2<f32>,
    @location(2) tint_2 : vec4<f32>,
    @interpolate(flat) @location(1) sheet_3 : vec3<f32>,
};

@fragment
fn fragmentMain( _S5 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S6 : pixelOutput_0 = pixelOutput_0( (textureSample((sheet_1), (sheetSampler_0), (mix(_S5.uv_4, sharpen_0(_S5.uv_4, max(_S5.sheet_3.xy, vec2<f32>(1.0f, 1.0f))), vec2<f32>(_S5.sheet_3.z))))) * _S5.tint_2 );
    return _S6;
}

