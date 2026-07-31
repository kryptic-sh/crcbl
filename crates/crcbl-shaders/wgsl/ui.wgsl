struct Vertex_std430_0
{
    @align(16) position_0 : vec2<f32>,
    @align(8) uv_0 : vec2<f32>,
    @align(16) color_0 : vec4<f32>,
};

@binding(2) @group(0) var<storage, read> vertices_0 : array<Vertex_std430_0>;

struct UiConstants_std430_0
{
    @align(8) viewport_0 : vec2<f32>,
};

var<uniform> constants_0 : UiConstants_std430_0;
@binding(0) @group(0) var glyphAtlas_0 : texture_2d<f32>;

@binding(1) @group(0) var glyphSampler_0 : sampler;

struct UiOutput_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) uv_1 : vec2<f32>,
    @location(1) color_1 : vec4<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> UiOutput_0
{
    var v_0 : Vertex_std430_0 = vertices_0[index_0];
    var ndc_0 : vec2<f32>;
    ndc_0[i32(0)] = v_0.position_0.x / constants_0.viewport_0.x * 2.0f - 1.0f;
    ndc_0[i32(1)] = 1.0f - v_0.position_0.y / constants_0.viewport_0.y * 2.0f;
    var output_0 : UiOutput_0;
    output_0.position_1 = vec4<f32>(ndc_0, 0.0f, 1.0f);
    output_0.uv_1 = v_0.uv_0;
    output_0.color_1 = v_0.color_0;
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_2 : vec2<f32>,
    @location(1) color_2 : vec4<f32>,
};

@fragment
fn fragmentMain( _S1 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var color_3 : vec4<f32> = _S1.color_2;
    var _S2 : bool;
    if((_S1.uv_2.x) > 0.0f)
    {
        _S2 = true;
    }
    else
    {
        _S2 = (_S1.uv_2.y) > 0.0f;
    }
    if(_S2)
    {
        color_3[i32(3)] = color_3[i32(3)] * (textureSample((glyphAtlas_0), (glyphSampler_0), (_S1.uv_2)).x);
    }
    var _S3 : pixelOutput_0 = pixelOutput_0( color_3 );
    return _S3;
}

