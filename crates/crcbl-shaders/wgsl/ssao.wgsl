@binding(1) @group(0) var scene_depth_0 : texture_depth_2d;

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

@binding(0) @group(0) var<uniform> camera_0 : SsaoParams_std140_0;
var<private> KERNEL_0 : array<vec3<f32>, i32(8)> = array<vec3<f32>, i32(8)>( vec3<f32>(0.875f, 0.0f, 0.25f), vec3<f32>(-0.75f, 0.0f, 0.375f), vec3<f32>(0.0f, 0.75f, 0.25f), vec3<f32>(0.0f, -0.625f, 0.5f), vec3<f32>(0.5f, 0.5f, 0.375f), vec3<f32>(-0.5f, 0.5f, 0.625f), vec3<f32>(0.375f, -0.375f, 0.75f), vec3<f32>(-0.25f, -0.25f, 0.875f) );
var<private> ROTATIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(2.0f, 0.0f), vec2<f32>(-2.0f, 0.0f), vec2<f32>(1.0f, 1.0f), vec2<f32>(-1.0f, -1.0f), vec2<f32>(0.0f, -2.0f), vec2<f32>(0.0f, 2.0f), vec2<f32>(1.0f, -1.0f), vec2<f32>(-1.0f, 1.0f), vec2<f32>(1.0f, 2.0f), vec2<f32>(-1.0f, -2.0f), vec2<f32>(2.0f, 1.0f), vec2<f32>(-2.0f, -1.0f), vec2<f32>(2.0f, -1.0f), vec2<f32>(-2.0f, 1.0f), vec2<f32>(1.0f, -2.0f), vec2<f32>(-1.0f, 2.0f) );
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

fn view_position_0( pixel_1 : vec2<i32>,  depth_0 : f32,  extent_1 : vec2<f32>) -> vec3<f32>
{
    var view_0 : vec4<f32> = (((vec4<f32>(vec2<f32>((f32(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (f32(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
    return view_0.xyz / vec3<f32>(view_0.w);
}

fn normal_at_0( pixel_2 : vec2<i32>,  centre_0 : vec3<f32>,  extent_2 : vec2<i32>,  size_0 : vec2<f32>) -> vec3<f32>
{
    var _S3 : vec2<i32> = pixel_2 + vec2<i32>(i32(-1), i32(0));
    var left_0 : vec3<f32> = view_position_0(_S3, depth_at_0(_S3, extent_2), size_0);
    var _S4 : vec2<i32> = pixel_2 + vec2<i32>(i32(1), i32(0));
    var right_0 : vec3<f32> = view_position_0(_S4, depth_at_0(_S4, extent_2), size_0);
    var _S5 : vec2<i32> = pixel_2 + vec2<i32>(i32(0), i32(-1));
    var up_0 : vec3<f32> = view_position_0(_S5, depth_at_0(_S5, extent_2), size_0);
    var _S6 : vec2<i32> = pixel_2 + vec2<i32>(i32(0), i32(1));
    var down_0 : vec3<f32> = view_position_0(_S6, depth_at_0(_S6, extent_2), size_0);
    var _S7 : f32 = centre_0.z;
    var horizontal_0 : vec3<f32>;
    if((abs(right_0.z - _S7)) < (abs(_S7 - left_0.z)))
    {
        horizontal_0 = right_0 - centre_0;
    }
    else
    {
        horizontal_0 = centre_0 - left_0;
    }
    var vertical_0 : vec3<f32>;
    if((abs(down_0.z - _S7)) < (abs(_S7 - up_0.z)))
    {
        vertical_0 = down_0 - centre_0;
    }
    else
    {
        vertical_0 = centre_0 - up_0;
    }
    return normalize(cross(vertical_0, horizontal_0));
}

fn occlusion_at_0( pixel_3 : vec2<i32>,  centre_1 : vec3<f32>,  normal_0 : vec3<f32>,  extent_3 : vec2<i32>,  size_1 : vec2<f32>) -> f32
{
    var _S8 : f32 = camera_0.params_0.x;
    var _S9 : f32 = camera_0.params_0.y;
    var seed_0 : vec3<f32> = vec3<f32>(ROTATIONS_0[((u32(pixel_3.y) & (u32(3)))) * u32(4) + ((u32(pixel_3.x) & (u32(3))))], 0.0f);
    var tangent_0 : vec3<f32> = seed_0 - normal_0 * vec3<f32>(dot(seed_0, normal_0));
    var across_0 : vec3<f32>;
    if((dot(tangent_0, tangent_0)) > 9.99999993922529029e-09f)
    {
        across_0 = normalize(tangent_0);
    }
    else
    {
        across_0 = vec3<f32>(1.0f, 0.0f, 0.0f);
    }
    var _S10 : vec3<f32> = cross(normal_0, across_0);
    var index_1 : u32 = u32(0);
    var blocked_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_1 < u32(8))
        {
        }
        else
        {
            break;
        }
        var at_0 : vec3<f32> = centre_1 + (across_0 * vec3<f32>(KERNEL_0[index_1].x) + _S10 * vec3<f32>(KERNEL_0[index_1].y) + normal_0 * vec3<f32>(KERNEL_0[index_1].z)) * vec3<f32>(_S8);
        var clip_0 : vec4<f32> = (((vec4<f32>(at_0, 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
        var _S11 : f32 = clip_0.w;
        if(_S11 <= 0.0f)
        {
            index_1 = index_1 + u32(1);
            continue;
        }
        var ndc_0 : vec2<f32> = clip_0.xy / vec2<f32>(_S11);
        var _S12 : i32 = i32((ndc_0.x * 0.5f + 0.5f) * size_1.x);
        var _S13 : i32 = i32((0.5f - ndc_0.y * 0.5f) * size_1.y);
        var tap_0 : vec2<i32> = vec2<i32>(_S12, _S13);
        var _S14 : bool;
        if(_S12 < i32(0))
        {
            _S14 = true;
        }
        else
        {
            _S14 = _S13 < i32(0);
        }
        var _S15 : bool;
        if(_S14)
        {
            _S15 = true;
        }
        else
        {
            _S15 = _S12 >= (extent_3.x);
        }
        var _S16 : bool;
        if(_S15)
        {
            _S16 = true;
        }
        else
        {
            _S16 = _S13 >= (extent_3.y);
        }
        if(_S16)
        {
            index_1 = index_1 + u32(1);
            continue;
        }
        var depth_1 : f32 = depth_at_0(tap_0, extent_3);
        if(depth_1 <= 0.0f)
        {
            index_1 = index_1 + u32(1);
            continue;
        }
        var _S17 : f32 = view_position_0(tap_0, depth_1, size_1).z;
        var blocked_1 : f32;
        if(_S17 >= (at_0.z + _S9))
        {
            blocked_1 = blocked_0 + saturate(_S8 / max(abs(centre_1.z - _S17), 0.00000999999974738f));
        }
        else
        {
            blocked_1 = blocked_0;
        }
        blocked_0 = blocked_1;
        index_1 = index_1 + u32(1);
    }
    return blocked_0 / 8.0f;
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
fn fragmentMain( _S18 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var extent_4 : vec2<i32> = vec2<i32>(i32(width_0), i32(height_0));
    var size_2 : vec2<f32> = vec2<f32>(f32(width_0), f32(height_0));
    var _S19 : vec2<i32> = vec2<i32>(position_1.xy);
    var depth_2 : f32 = depth_at_0(_S19, extent_4);
    if(depth_2 <= 0.0f)
    {
        var _S20 : pixelOutput_0 = pixelOutput_0( 1.0f );
        return _S20;
    }
    var centre_2 : vec3<f32> = view_position_0(_S19, depth_2, size_2);
    var _S21 : pixelOutput_0 = pixelOutput_0( saturate(1.0f - occlusion_at_0(_S19, centre_2, normal_at_0(_S19, centre_2, extent_4, size_2), extent_4, size_2)) );
    return _S21;
}

