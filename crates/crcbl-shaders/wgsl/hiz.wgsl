@binding(0) @group(0) var source_0 : texture_depth_2d;

struct FullscreenOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> FullscreenOutput_0
{
    var output_0 : FullscreenOutput_0;
    output_0.position_0 = vec4<f32>(vec2<f32>(f32((((index_0 << (u32(1)))) & (u32(2)))), f32((index_0 & (u32(2))))) * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f), 0.0f, 1.0f);
    return output_0;
}

struct HizOutput_0
{
    @builtin(frag_depth) depth_0 : f32,
};

@fragment
fn fragmentMain(@builtin(position) position_1 : vec4<f32>) -> HizOutput_0
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((source_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var _S1 : i32 = i32(width_0);
    var _S2 : i32 = i32(height_0);
    var base_0 : vec2<i32> = vec2<i32>(position_1.xy) * vec2<i32>(i32(2));
    const _S3 : vec2<i32> = vec2<i32>(i32(0), i32(0));
    const _S4 : vec2<i32> = vec2<i32>(i32(1), i32(1));
    var _S5 : vec2<i32> = vec2<i32>(_S1, _S2) - _S4;
    var _S6 : vec3<i32> = vec3<i32>(clamp(base_0, _S3, _S5), i32(0));
    var _S7 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(1), i32(0)), _S3, _S5), i32(0));
    var _S8 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(0), i32(1)), _S3, _S5), i32(0));
    var _S9 : vec3<i32> = vec3<i32>(clamp(base_0 + _S4, _S3, _S5), i32(0));
    var _S10 : f32 = max(max(max((textureLoad((source_0), ((_S6)).xy, ((_S6)).z)), (textureLoad((source_0), ((_S7)).xy, ((_S7)).z))), (textureLoad((source_0), ((_S8)).xy, ((_S8)).z))), (textureLoad((source_0), ((_S9)).xy, ((_S9)).z)));
    var odd_x_0 : bool = ((_S1 & (i32(1)))) == i32(1);
    var odd_y_0 : bool = ((_S2 & (i32(1)))) == i32(1);
    var nearest_0 : f32;
    if(odd_x_0)
    {
        var _S11 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(2), i32(0)), _S3, _S5), i32(0));
        var _S12 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(2), i32(1)), _S3, _S5), i32(0));
        nearest_0 = max(max(_S10, (textureLoad((source_0), ((_S11)).xy, ((_S11)).z))), (textureLoad((source_0), ((_S12)).xy, ((_S12)).z)));
    }
    else
    {
        nearest_0 = _S10;
    }
    if(odd_y_0)
    {
        var _S13 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(0), i32(2)), _S3, _S5), i32(0));
        var _S14 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(1), i32(2)), _S3, _S5), i32(0));
        nearest_0 = max(max(nearest_0, (textureLoad((source_0), ((_S13)).xy, ((_S13)).z))), (textureLoad((source_0), ((_S14)).xy, ((_S14)).z)));
    }
    else
    {
    }
    var _S15 : bool;
    if(odd_x_0)
    {
        _S15 = odd_y_0;
    }
    else
    {
        _S15 = false;
    }
    if(_S15)
    {
        var _S16 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(2), i32(2)), _S3, _S5), i32(0));
        nearest_0 = max(nearest_0, (textureLoad((source_0), ((_S16)).xy, ((_S16)).z)));
    }
    else
    {
    }
    var output_1 : HizOutput_0;
    output_1.depth_0 = nearest_0;
    return output_1;
}

