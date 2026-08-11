struct DrawConstants_std140_0
{
    @align(16) base_0 : u32,
    @align(4) pad0_0 : u32,
    @align(8) pad1_0 : u32,
    @align(4) pad2_0 : u32,
};

@binding(3) @group(0) var<uniform> draw_0 : DrawConstants_std140_0;
@binding(5) @group(0) var<storage, read> visible_instances_0 : array<u32>;

struct _MatrixStorage_float4x4_ColMajorstd430_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct GpuInstance_std430_0
{
    @align(16) transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) mesh_0 : u32,
    @align(4) material_0 : u32,
    @align(8) sector_0 : u32,
    @align(4) flags_0 : u32,
};

@binding(2) @group(0) var<storage, read> instances_0 : array<GpuInstance_std430_0>;

struct GpuMesh_std430_0
{
    @align(4) base_vertex_0 : u32,
    @align(4) base_index_0 : u32,
    @align(4) index_count_0 : u32,
    @align(4) min_x_0 : f32,
    @align(4) min_y_0 : f32,
    @align(4) min_z_0 : f32,
    @align(4) max_x_0 : f32,
    @align(4) max_y_0 : f32,
    @align(4) max_z_0 : f32,
};

@binding(4) @group(0) var<storage, read> meshes_0 : array<GpuMesh_std430_0>;

struct MeshVertex_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) normal_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) uv_0 : vec4<f32>,
};

@binding(1) @group(0) var<storage, read> vertices_0 : array<MeshVertex_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_1 : array<vec4<f32>, i32(4)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0
{
    @align(16) data_2 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(2)>,
};

struct FrameUniforms_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) light_direction_0 : vec4<f32>,
    @align(16) light_color_0 : vec4<f32>,
    @align(16) ambient_0 : vec4<f32>,
    @align(16) shadow_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0,
    @align(16) cascade_far_0 : vec4<f32>,
    @align(16) shadow_params_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
struct GpuMaterial_std430_0
{
    @align(16) base_color_0 : vec4<f32>,
    @align(16) base_color_texture_0 : u32,
    @align(4) pad0_1 : u32,
    @align(8) pad1_1 : u32,
    @align(4) pad2_1 : u32,
};

@binding(6) @group(0) var<storage, read> materials_0 : array<GpuMaterial_std430_0>;

@binding(7) @group(0) var base_color_textures_0 : texture_2d_array<f32>;

@binding(8) @group(0) var base_color_sampler_0 : sampler;

@binding(15) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(16) @group(0) var shadow_sampler_0 : sampler_comparison;

struct VertexOutput_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @location(2) world_normal_0 : vec3<f32>,
    @location(3) color_1 : vec4<f32>,
    @interpolate(flat) @location(4) material_1 : u32,
    @location(1) uv_1 : vec2<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32, @builtin(instance_index) instance_id_0 : u32) -> VertexOutput_0
{
    var instance_0 : GpuInstance_std430_0 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]];
    var vertex_0 : MeshVertex_std430_0 = vertices_0[index_0 + meshes_0[instance_0.mesh_0].base_vertex_0];
    var _S1 : mat4x4<f32> = mat4x4<f32>(instance_0.transform_0.data_0[i32(0)][i32(0)], instance_0.transform_0.data_0[i32(1)][i32(0)], instance_0.transform_0.data_0[i32(2)][i32(0)], instance_0.transform_0.data_0[i32(3)][i32(0)], instance_0.transform_0.data_0[i32(0)][i32(1)], instance_0.transform_0.data_0[i32(1)][i32(1)], instance_0.transform_0.data_0[i32(2)][i32(1)], instance_0.transform_0.data_0[i32(3)][i32(1)], instance_0.transform_0.data_0[i32(0)][i32(2)], instance_0.transform_0.data_0[i32(1)][i32(2)], instance_0.transform_0.data_0[i32(2)][i32(2)], instance_0.transform_0.data_0[i32(3)][i32(2)], instance_0.transform_0.data_0[i32(0)][i32(3)], instance_0.transform_0.data_0[i32(1)][i32(3)], instance_0.transform_0.data_0[i32(2)][i32(3)], instance_0.transform_0.data_0[i32(3)][i32(3)]);
    var world_0 : vec4<f32> = (((vec4<f32>(vertex_0.position_0.xyz, 1.0f)) * (_S1)));
    var output_0 : VertexOutput_0;
    output_0.position_1 = (((world_0) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)]))));
    output_0.world_position_0 = world_0.xyz;
    output_0.world_normal_0 = (((vertex_0.normal_0.xyz) * (mat3x3<f32>(_S1[i32(0)].xyz, _S1[i32(1)].xyz, _S1[i32(2)].xyz))));
    output_0.color_1 = vertex_0.color_0;
    output_0.material_1 = instance_0.material_0;
    output_0.uv_1 = vertex_0.uv_0.xy;
    return output_0;
}

fn sun_visibility_0( world_position_1 : vec3<f32>,  n_dot_l_0 : f32) -> f32
{
    var cascade_0 : u32;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }
    var _S2 : f32 = length(world_position_1 - frame_0.camera_position_0.xyz);
    var index_1 : u32 = u32(0);
    for(;;)
    {
        if(index_1 < u32(2))
        {
        }
        else
        {
            cascade_0 = u32(1);
            break;
        }
        if(_S2 < (frame_0.cascade_far_0[index_1]))
        {
            cascade_0 = index_1;
            break;
        }
        index_1 = index_1 + u32(1);
    }
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_1, 1.0f)) * (mat4x4<f32>(frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(3)]))));
    var ndc_0 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S3 : vec2<f32> = vec2<f32>(1.0f);
    var _S4 : bool;
    if((any(((abs(ndc_0.xy)) > _S3))))
    {
        _S4 = true;
    }
    else
    {
        _S4 = (ndc_0.z) <= 0.0f;
    }
    if(_S4)
    {
        return 1.0f;
    }
    var _S5 : vec2<f32> = vec2<f32>(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);
    var texel_0 : vec2<f32> = frame_0.shadow_params_0.xy;
    var cosine_0 : f32 = saturate(n_dot_l_0);
    var _S6 : f32 = ndc_0.z + (frame_0.shadow_params_0.z + frame_0.shadow_params_0.w * min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f));
    const _S7 : vec2<f32> = vec2<f32>(2.0f, 1.0f);
    var _S8 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_0 * _S7;
    var y_0 : i32 = i32(-1);
    var visibility_0 : f32 = 0.0f;
    for(;;)
    {
        if(y_0 <= i32(1))
        {
        }
        else
        {
            break;
        }
        var x_0 : i32 = i32(-1);
        for(;;)
        {
            if(x_0 <= i32(1))
            {
            }
            else
            {
                break;
            }
            var tap_0 : vec2<f32> = clamp(_S5 + vec2<f32>(f32(x_0), f32(y_0)) * texel_0 * _S7, _S8, _S3 - _S8);
            var visibility_1 : f32 = visibility_0 + (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (vec2<f32>((f32(cascade_0) + tap_0.x) / 2.0f, tap_0.y)), (_S6)));
            x_0 = x_0 + i32(1);
            visibility_0 = visibility_1;
        }
        y_0 = y_0 + i32(1);
    }
    return visibility_0 / 9.0f;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_2 : vec3<f32>,
    @location(2) world_normal_1 : vec3<f32>,
    @location(3) color_2 : vec4<f32>,
    @interpolate(flat) @location(4) material_2 : u32,
    @location(1) uv_2 : vec2<f32>,
};

@fragment
fn fragmentMain( _S9 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var normal_1 : vec3<f32> = normalize(_S9.world_normal_1);
    var to_light_0 : vec3<f32> = normalize(frame_0.light_direction_0.xyz);
    var material_3 : GpuMaterial_std430_0 = materials_0[_S9.material_2];
    var _S10 : vec3<f32> = vec3<f32>(_S9.uv_2, f32(material_3.base_color_texture_0));
    var albedo_0 : vec4<f32> = _S9.color_2 * material_3.base_color_0 * (textureSample((base_color_textures_0), (base_color_sampler_0), ((_S10)).xy, i32(((_S10)).z)));
    var n_dot_l_1 : f32 = dot(normal_1, to_light_0);
    var _S11 : f32 = max(n_dot_l_1, 0.0f);
    var visibility_2 : f32 = sun_visibility_0(_S9.world_position_2, n_dot_l_1);
    var _S12 : pixelOutput_0 = pixelOutput_0( vec4<f32>(albedo_0.xyz * (frame_0.ambient_0.xyz + frame_0.light_color_0.xyz * vec3<f32>((_S11 * visibility_2))) + frame_0.light_color_0.xyz * vec3<f32>((pow(max(dot(normal_1, normalize(to_light_0 + normalize(frame_0.camera_position_0.xyz - _S9.world_position_2))), 0.0f), 32.0f) * (step(0.0f, _S11) * _S11) * visibility_2 * 0.34999999403953552f)), albedo_0.w) );
    return _S12;
}

