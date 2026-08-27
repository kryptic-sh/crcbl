struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct SkyParams_std140_0
{
    @align(16) inv_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) inv_view_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) sky_0 : array<vec4<f32>, i32(3)>,
};

@binding(0) @group(0) var<uniform> camera_0 : SkyParams_std140_0;
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

fn sky_radiance_0( direction_0 : vec3<f32>) -> vec3<f32>
{
    var up_0 : f32 = clamp(direction_0.y, -1.0f, 1.0f);
    var far_0 : vec3<f32>;
    if(up_0 >= 0.0f)
    {
        far_0 = camera_0.sky_0[i32(0)].xyz;
    }
    else
    {
        far_0 = camera_0.sky_0[i32(2)].xyz;
    }
    var u_0 : f32 = abs(up_0);
    var blend_0 : f32 = u_0 * u_0 * (3.0f - 2.0f * u_0);
    return camera_0.sky_0[i32(1)].xyz * vec3<f32>((1.0f - blend_0)) + far_0 * vec3<f32>(blend_0);
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
    var ndc_0 : vec2<f32> = vec2<f32>(_S2.uv_1.x * 2.0f - 1.0f, 1.0f - _S2.uv_1.y * 2.0f);
    var near_plane_0 : vec4<f32> = (((vec4<f32>(ndc_0, 1.0f, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
    var beyond_0 : vec4<f32> = (((vec4<f32>(ndc_0, 0.5f, 1.0f)) * (mat4x4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(0)], camera_0.inv_proj_0.data_0[i32(2)][i32(0)], camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(0)][i32(1)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)], camera_0.inv_proj_0.data_0[i32(2)][i32(1)], camera_0.inv_proj_0.data_0[i32(3)][i32(1)], camera_0.inv_proj_0.data_0[i32(0)][i32(2)], camera_0.inv_proj_0.data_0[i32(1)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(2)], camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(0)][i32(3)], camera_0.inv_proj_0.data_0[i32(1)][i32(3)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)], camera_0.inv_proj_0.data_0[i32(3)][i32(3)]))));
    var _S3 : pixelOutput_0 = pixelOutput_0( vec4<f32>(sky_radiance_0(normalize((((vec4<f32>(beyond_0.xyz / vec3<f32>(beyond_0.w) - near_plane_0.xyz / vec3<f32>(near_plane_0.w), 0.0f)) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz)), 1.0f) );
    return _S3;
}

