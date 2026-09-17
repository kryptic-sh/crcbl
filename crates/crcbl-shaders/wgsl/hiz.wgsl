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

fn combine_0( a_0 : f32,  b_0 : f32,  farthest_0 : bool) -> f32
{
    var _S1 : f32;
    if(farthest_0)
    {
        _S1 = min(a_0, b_0);
    }
    else
    {
        _S1 = max(a_0, b_0);
    }
    return _S1;
}

fn reduce_0( input_0 : FullscreenOutput_0,  farthest_1 : bool) -> f32
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((source_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var _S2 : i32 = i32(width_0);
    var _S3 : i32 = i32(height_0);
    var base_0 : vec2<i32> = vec2<i32>(input_0.position_0.xy) * vec2<i32>(i32(2));
    const _S4 : vec2<i32> = vec2<i32>(i32(0), i32(0));
    const _S5 : vec2<i32> = vec2<i32>(i32(1), i32(1));
    var _S6 : vec2<i32> = vec2<i32>(_S2, _S3) - _S5;
    var _S7 : vec3<i32> = vec3<i32>(clamp(base_0, _S4, _S6), i32(0));
    var _S8 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(1), i32(0)), _S4, _S6), i32(0));
    var _S9 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(0), i32(1)), _S4, _S6), i32(0));
    var _S10 : vec3<i32> = vec3<i32>(clamp(base_0 + _S5, _S4, _S6), i32(0));
    var value_0 : f32 = combine_0(combine_0(combine_0((textureLoad((source_0), ((_S7)).xy, ((_S7)).z)), (textureLoad((source_0), ((_S8)).xy, ((_S8)).z)), farthest_1), (textureLoad((source_0), ((_S9)).xy, ((_S9)).z)), farthest_1), (textureLoad((source_0), ((_S10)).xy, ((_S10)).z)), farthest_1);
    var odd_x_0 : bool = ((_S2 & (i32(1)))) == i32(1);
    var odd_y_0 : bool = ((_S3 & (i32(1)))) == i32(1);
    var value_1 : f32;
    if(odd_x_0)
    {
        var _S11 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(2), i32(0)), _S4, _S6), i32(0));
        var _S12 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(2), i32(1)), _S4, _S6), i32(0));
        value_1 = combine_0(combine_0(value_0, (textureLoad((source_0), ((_S11)).xy, ((_S11)).z)), farthest_1), (textureLoad((source_0), ((_S12)).xy, ((_S12)).z)), farthest_1);
    }
    else
    {
        value_1 = value_0;
    }
    if(odd_y_0)
    {
        var _S13 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(0), i32(2)), _S4, _S6), i32(0));
        var _S14 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(1), i32(2)), _S4, _S6), i32(0));
        value_1 = combine_0(combine_0(value_1, (textureLoad((source_0), ((_S13)).xy, ((_S13)).z)), farthest_1), (textureLoad((source_0), ((_S14)).xy, ((_S14)).z)), farthest_1);
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
        var _S16 : vec3<i32> = vec3<i32>(clamp(base_0 + vec2<i32>(i32(2), i32(2)), _S4, _S6), i32(0));
        value_1 = combine_0(value_1, (textureLoad((source_0), ((_S16)).xy, ((_S16)).z)), farthest_1);
    }
    else
    {
    }
    return value_1;
}

struct HizOutput_0
{
    @builtin(frag_depth) depth_0 : f32,
};

@fragment
fn fragmentMain(@builtin(position) position_1 : vec4<f32>) -> HizOutput_0
{
    var _S17 : FullscreenOutput_0 = FullscreenOutput_0( position_1 );
    var output_1 : HizOutput_0;
    output_1.depth_0 = reduce_0(_S17, false);
    return output_1;
}

@fragment
fn farthestMain(@builtin(position) position_2 : vec4<f32>) -> HizOutput_0
{
    var _S18 : FullscreenOutput_0 = FullscreenOutput_0( position_2 );
    var output_2 : HizOutput_0;
    output_2.depth_0 = reduce_0(_S18, true);
    return output_2;
}

