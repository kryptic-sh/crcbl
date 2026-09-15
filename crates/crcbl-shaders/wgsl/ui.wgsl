struct Vertex_std430_0
{
    @align(16) position_0 : vec2<f32>,
    @align(8) uv_0 : vec2<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) clip_0 : vec4<f32>,
    @align(16) shape_0 : vec4<f32>,
    @align(16) radii_0 : vec4<f32>,
    @align(16) border_0 : vec4<f32>,
};

@binding(2) @group(0) var<storage, read> vertices_0 : array<Vertex_std430_0>;

struct UiConstants_std140_0
{
    @align(16) viewport_0 : vec2<f32>,
};

@binding(3) @group(0) var<uniform> constants_0 : UiConstants_std140_0;
@binding(0) @group(0) var glyphAtlas_0 : texture_2d<f32>;

@binding(1) @group(0) var glyphSampler_0 : sampler;

@binding(4) @group(0) var imageAtlas_0 : texture_2d<f32>;

@binding(5) @group(0) var imageSampler_0 : sampler;

struct UiOutput_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) uv_1 : vec2<f32>,
    @location(6) color_1 : vec4<f32>,
    @location(1) screen_0 : vec2<f32>,
    @interpolate(flat) @location(2) clip_1 : vec4<f32>,
    @interpolate(flat) @location(3) shape_1 : vec4<f32>,
    @interpolate(flat) @location(4) radii_1 : vec4<f32>,
    @interpolate(flat) @location(5) border_1 : vec4<f32>,
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
    output_0.screen_0 = v_0.position_0;
    output_0.clip_1 = v_0.clip_0;
    output_0.shape_1 = v_0.shape_0;
    output_0.radii_1 = v_0.radii_0;
    output_0.border_1 = v_0.border_0;
    return output_0;
}

fn sharpen_0( uv_2 : vec2<f32>,  size_0 : vec2<f32>) -> vec2<f32>
{
    var _S1 : vec2<f32> = vec2<f32>(0.5f);
    var s_0 : vec2<f32> = uv_2 * size_0 - _S1;
    var whole_0 : vec2<f32> = floor(s_0);
    return (whole_0 + saturate((s_0 - whole_0 - _S1) / max((fwidth((s_0))), vec2<f32>(9.99999997475242708e-07f, 9.99999997475242708e-07f)) + _S1) + _S1) / size_0;
}

fn roundedBoxDistance_0( p_0 : vec2<f32>,  halfExtent_0 : vec2<f32>,  radii_2 : vec4<f32>) -> f32
{
    var _S2 : bool = (p_0.x) > 0.0f;
    var top_0 : f32;
    if(_S2)
    {
        top_0 = radii_2.y;
    }
    else
    {
        top_0 = radii_2.x;
    }
    var bottom_0 : f32;
    if(_S2)
    {
        bottom_0 = radii_2.z;
    }
    else
    {
        bottom_0 = radii_2.w;
    }
    var radius_0 : f32;
    if((p_0.y) > 0.0f)
    {
        radius_0 = bottom_0;
    }
    else
    {
        radius_0 = top_0;
    }
    var q_0 : vec2<f32> = abs(p_0) - halfExtent_0 + vec2<f32>(radius_0);
    return min(max(q_0.x, q_0.y), 0.0f) + length(max(q_0, vec2<f32>(0.0f, 0.0f))) - radius_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_3 : vec2<f32>,
    @location(6) color_2 : vec4<f32>,
    @location(1) screen_1 : vec2<f32>,
    @interpolate(flat) @location(2) clip_2 : vec4<f32>,
    @interpolate(flat) @location(3) shape_2 : vec4<f32>,
    @interpolate(flat) @location(4) radii_3 : vec4<f32>,
    @interpolate(flat) @location(5) border_2 : vec4<f32>,
};

@fragment
fn fragmentMain( _S3 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var glyph_0 : f32 = (textureSample((glyphAtlas_0), (glyphSampler_0), (_S3.uv_3)).x);
    var imageWidth_0 : u32;
    var imageHeight_0 : u32;
    {var dim = textureDimensions((imageAtlas_0));((imageWidth_0)) = dim.x;((imageHeight_0)) = dim.y;};
    var texel_0 : vec4<f32> = (textureSample((imageAtlas_0), (imageSampler_0), (sharpen_0(_S3.uv_3, vec2<f32>(f32(imageWidth_0), f32(imageHeight_0))))));
    var primitive_0 : f32 = _S3.shape_2.w;
    var color_3 : vec4<f32> = _S3.color_2;
    var _S4 : f32;
    if(primitive_0 == 1.0f)
    {
        _S4 = glyph_0;
    }
    else
    {
        _S4 = 1.0f;
    }
    color_3[i32(3)] = color_3[i32(3)] * _S4;
    var _S5 : vec4<f32>;
    if(primitive_0 == 2.0f)
    {
        _S5 = texel_0 * _S3.color_2;
    }
    else
    {
        _S5 = color_3;
    }
    color_3 = _S5;
    var distance_0 : f32 = roundedBoxDistance_0(_S3.uv_3, _S3.shape_2.xy, _S3.radii_3);
    var coverage_0 : f32 = saturate(0.5f - distance_0);
    var _S6 : f32 = _S3.shape_2.z;
    var inner_0 : f32 = saturate(0.5f - (distance_0 + _S6));
    var rounded_0 : vec4<f32>;
    if(_S6 > 0.0f)
    {
        _S4 = inner_0;
    }
    else
    {
        _S4 = 1.0f;
    }
    rounded_0 = mix(_S3.border_2, _S3.color_2, vec4<f32>(_S4));
    rounded_0[i32(3)] = rounded_0[i32(3)] * coverage_0;
    if(primitive_0 == 3.0f)
    {
        _S5 = rounded_0;
    }
    else
    {
        _S5 = color_3;
    }
    color_3 = _S5;
    var _S7 : f32 = _S3.screen_1.x;
    var _S8 : bool;
    if(_S7 < (_S3.clip_2.x))
    {
        _S8 = true;
    }
    else
    {
        _S8 = (_S3.screen_1.y) < (_S3.clip_2.y);
    }
    if(_S8)
    {
        _S8 = true;
    }
    else
    {
        _S8 = _S7 >= (_S3.clip_2.z);
    }
    if(_S8)
    {
        _S8 = true;
    }
    else
    {
        _S8 = (_S3.screen_1.y) >= (_S3.clip_2.w);
    }
    if(_S8)
    {
        discard;
    }
    var _S9 : pixelOutput_0 = pixelOutput_0( color_3 );
    return _S9;
}

