@binding(1) @group(0) var occlusion_0 : texture_2d<f32>;

@binding(2) @group(0) var scene_depth_0 : texture_depth_2d;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct SsaoParams_std140_0
{
    @align(16) inv_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) params_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> ssao_0 : SsaoParams_std140_0;
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

fn depth_at_0( pixel_0 : vec2<i32>,  extent_0 : vec2<i32>) -> f32
{
    var _S2 : vec3<i32> = vec3<i32>(clamp(pixel_0, vec2<i32>(i32(0), i32(0)), extent_0 - vec2<i32>(i32(1), i32(1))), i32(0));
    return (textureLoad((scene_depth_0), ((_S2)).xy, ((_S2)).z));
}

fn view_z_0( pixel_1 : vec2<i32>,  depth_0 : f32,  extent_1 : vec2<f32>) -> f32
{
    var view_0 : vec4<f32> = (((vec4<f32>(vec2<f32>((f32(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (f32(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (mat4x4<f32>(ssao_0.inv_proj_0.data_0[i32(0)][i32(0)], ssao_0.inv_proj_0.data_0[i32(1)][i32(0)], ssao_0.inv_proj_0.data_0[i32(2)][i32(0)], ssao_0.inv_proj_0.data_0[i32(3)][i32(0)], ssao_0.inv_proj_0.data_0[i32(0)][i32(1)], ssao_0.inv_proj_0.data_0[i32(1)][i32(1)], ssao_0.inv_proj_0.data_0[i32(2)][i32(1)], ssao_0.inv_proj_0.data_0[i32(3)][i32(1)], ssao_0.inv_proj_0.data_0[i32(0)][i32(2)], ssao_0.inv_proj_0.data_0[i32(1)][i32(2)], ssao_0.inv_proj_0.data_0[i32(2)][i32(2)], ssao_0.inv_proj_0.data_0[i32(3)][i32(2)], ssao_0.inv_proj_0.data_0[i32(0)][i32(3)], ssao_0.inv_proj_0.data_0[i32(1)][i32(3)], ssao_0.inv_proj_0.data_0[i32(2)][i32(3)], ssao_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
    return view_0.z / view_0.w;
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
fn fragmentMain( _S3 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((occlusion_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var extent_2 : vec2<i32> = vec2<i32>(i32(width_0), i32(height_0));
    var size_0 : vec2<f32> = vec2<f32>(f32(width_0), f32(height_0));
    var _S4 : vec2<i32> = vec2<i32>(position_1.xy);
    var centre_depth_0 : f32 = depth_at_0(_S4, extent_2);
    if(centre_depth_0 <= 0.0f)
    {
        var _S5 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S5;
    }
    var _S6 : f32 = view_z_0(_S4, centre_depth_0, size_0);
    var _S7 : f32 = ssao_0.params_0.x * 2.0f;
    var y_0 : i32 = i32(-1);
    var total_0 : f32 = 0.0f;
    var weight_0 : f32 = 0.0f;
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
            var tap_0 : vec2<i32> = clamp(_S4 + vec2<i32>(x_0, y_0), vec2<i32>(i32(0), i32(0)), extent_2 - vec2<i32>(i32(1), i32(1)));
            var _S8 : bool;
            if(x_0 != i32(0))
            {
                _S8 = true;
            }
            else
            {
                _S8 = y_0 != i32(0);
            }
            var share_0 : f32;
            if(_S8)
            {
                var depth_1 : f32 = depth_at_0(tap_0, extent_2);
                var away_0 : f32 = abs(view_z_0(tap_0, depth_1, size_0) - _S6);
                if(depth_1 <= 0.0f)
                {
                    share_0 = 0.0f;
                }
                else
                {
                    share_0 = saturate(1.0f - away_0 / _S7);
                }
            }
            else
            {
                share_0 = 1.0f;
            }
            var _S9 : vec3<i32> = vec3<i32>(tap_0, i32(0));
            var total_1 : f32 = total_0 + (textureLoad((occlusion_0), ((_S9)).xy, ((_S9)).z).x) * share_0;
            var weight_1 : f32 = weight_0 + share_0;
            x_0 = x_0 + i32(1);
            total_0 = total_1;
            weight_0 = weight_1;
        }
        y_0 = y_0 + i32(1);
    }
    var _S10 : pixelOutput_0 = pixelOutput_0( total_0 / weight_0 );
    return _S10;
}

