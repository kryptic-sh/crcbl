@binding(0) @group(0) var occlusion_0 : texture_2d<f32>;

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
    @location(0) output_1 : f32,
};

struct pixelInput_0
{
    @location(0) uv_1 : vec2<f32>,
};

@fragment
fn fragmentMain( _S2 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((occlusion_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var _S3 : vec2<i32> = vec2<i32>(i32(width_0), i32(height_0));
    var _S4 : vec2<i32> = vec2<i32>(position_1.xy);
    var y_0 : i32 = i32(-1);
    var total_0 : f32 = 0.0f;
    for(;;)
    {
        if(y_0 < i32(3))
        {
        }
        else
        {
            break;
        }
        var x_0 : i32 = i32(-1);
        for(;;)
        {
            if(x_0 < i32(3))
            {
            }
            else
            {
                break;
            }
            var _S5 : vec3<i32> = vec3<i32>(clamp(_S4 + vec2<i32>(x_0, y_0), vec2<i32>(i32(0), i32(0)), _S3 - vec2<i32>(i32(1), i32(1))), i32(0));
            var total_1 : f32 = total_0 + (textureLoad((occlusion_0), ((_S5)).xy, ((_S5)).z).x);
            x_0 = x_0 + i32(1);
            total_0 = total_1;
        }
        y_0 = y_0 + i32(1);
    }
    var _S6 : pixelOutput_0 = pixelOutput_0( total_0 / 16.0f );
    return _S6;
}

