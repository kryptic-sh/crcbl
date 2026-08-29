struct DrawConstants_std140_0
{
    @align(16) base_0 : u32,
    @align(4) mesh_0 : u32,
    @align(8) pad0_0 : u32,
    @align(4) pad1_0 : u32,
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
    @align(16) previous_transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) mesh_1 : u32,
    @align(4) material_0 : u32,
    @align(8) sector_0 : u32,
    @align(4) flags_0 : u32,
    @align(16) base_vertex_0 : u32,
    @align(4) pad0_1 : u32,
    @align(8) pad1_1 : u32,
    @align(4) pad2_0 : u32,
};

@binding(2) @group(0) var<storage, read> instances_0 : array<GpuInstance_std430_0>;

struct GpuMesh_std430_0
{
    @align(4) base_vertex_1 : u32,
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

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0
{
    @align(16) data_3 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(14)>,
};

struct FrameUniforms_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) ambient_0 : vec4<f32>,
    @align(16) shadow_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0,
    @align(16) cascade_far_0 : vec4<f32>,
    @align(16) shadow_params_0 : vec4<f32>,
    @align(16) cluster_grid_0 : vec4<u32>,
    @align(16) light_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0,
    @align(16) probe_origin_0 : vec4<f32>,
    @align(16) probe_inv_spacing_0 : vec4<f32>,
    @align(16) probe_counts_0 : vec4<u32>,
    @align(16) lod_params_0 : vec4<f32>,
    @align(16) fog_params_0 : vec4<f32>,
    @align(16) fog_color_0 : vec4<f32>,
    @align(16) sky_sh_r_0 : vec4<f32>,
    @align(16) sky_sh_g_0 : vec4<f32>,
    @align(16) sky_sh_b_0 : vec4<f32>,
    @align(16) previous_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
};

@binding(0) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
@binding(22) @group(0) var ambient_occlusion_0 : texture_2d<f32>;

struct GpuMaterial_std430_0
{
    @align(16) base_color_0 : vec4<f32>,
    @align(16) base_color_texture_0 : u32,
    @align(4) metallic_0 : f32,
    @align(8) roughness_0 : f32,
    @align(4) tiling_0 : u32,
    @align(16) tile_metres_0 : f32,
    @align(4) emissive_r_0 : f32,
    @align(8) emissive_g_0 : f32,
    @align(4) emissive_b_0 : f32,
};

@binding(6) @group(0) var<storage, read> materials_0 : array<GpuMaterial_std430_0>;

@binding(7) @group(0) var base_color_textures_0 : texture_2d_array<f32>;

@binding(8) @group(0) var base_color_sampler_0 : sampler;

@binding(21) @group(0) var<storage, read> cluster_lights_0 : array<u32>;

struct GpuLight_std430_0
{
    @align(16) position_1 : vec4<f32>,
    @align(16) color_1 : vec4<f32>,
    @align(16) direction_0 : vec4<f32>,
    @align(16) kind_0 : u32,
    @align(4) cos_inner_0 : f32,
    @align(8) shadow_tile_0 : u32,
    @align(4) pad1_2 : u32,
};

@binding(20) @group(0) var<storage, read> lights_0 : array<GpuLight_std430_0>;

@binding(15) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(16) @group(0) var shadow_sampler_0 : sampler_comparison;

@binding(25) @group(0) var specular_albedo_0 : texture_2d<f32>;

struct GpuProbe_std430_0
{
    @align(16) sh_r_0 : vec4<f32>,
    @align(16) sh_g_0 : vec4<f32>,
    @align(16) sh_b_0 : vec4<f32>,
};

@binding(23) @group(0) var<storage, read> probes_0 : array<GpuProbe_std430_0>;

var<private> FOG_RATIO_KERNEL_0 : array<f32, i32(5)> = array<f32, i32(5)>( 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f );
var<private> FOG_KERNEL_0 : array<f32, i32(8)> = array<f32, i32(8)>( 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f );
var<private> SHADOW_DISC_0 : array<vec2<f32>, i32(32)> = array<vec2<f32>, i32(32)>( vec2<f32>(0.125f, 0.0f), vec2<f32>(-0.15964500606060028f, 0.14624799787998199f), vec2<f32>(0.02443600073456764f, -0.27843800187110901f), vec2<f32>(0.2012220025062561f, 0.26245900988578796f), vec2<f32>(-0.36926800012588501f, -0.06531800329685211f), vec2<f32>(0.34980198740959167f, -0.22251600027084351f), vec2<f32>(-0.11700200289487839f, 0.43524199724197388f), vec2<f32>(-0.22313599288463593f, -0.42963400483131409f), vec2<f32>(0.48411500453948975f, 0.17679800093173981f), vec2<f32>(-0.50364100933074951f, 0.20789599418640137f), vec2<f32>(0.24278800189495087f, -0.51882398128509521f), vec2<f32>(0.17941400408744812f, 0.57200098037719727f), vec2<f32>(-0.54075700044631958f, -0.31338000297546387f), vec2<f32>(0.63437002897262573f, -0.13946400582790375f), vec2<f32>(-0.38714599609375f, 0.55067497491836548f), vec2<f32>(-0.0894400030374527f, -0.69019997119903564f), vec2<f32>(0.5490720272064209f, 0.46275800466537476f), vec2<f32>(-0.73887801170349121f, 0.0305550005286932f), vec2<f32>(0.5389549732208252f, -0.53633201122283936f), vec2<f32>(-0.03605800122022629f, 0.77979201078414917f), vec2<f32>(-0.51281797885894775f, -0.61452698707580566f), vec2<f32>(0.81235998868942261f, 0.10930199921131134f), vec2<f32>(-0.68831098079681396f, 0.47890898585319519f), vec2<f32>(0.18808600306510925f, -0.83606100082397461f), vec2<f32>(0.43503299355506897f, 0.75919097661972046f), vec2<f32>(-0.85044801235198975f, -0.27131599187850952f), vec2<f32>(0.82610201835632324f, -0.38168001174926758f), vec2<f32>(-0.35788801312446594f, 0.85515600442886353f), vec2<f32>(-0.31940698623657227f, -0.88803398609161377f), vec2<f32>(0.84990900754928589f, 0.44668799638748169f), vec2<f32>(-0.94403499364852905f, 0.24884499609470367f), vec2<f32>(0.53659600019454956f, -0.83452999591827393f) );
var<private> SHADOW_PROBE_INDEX_0 : array<u32, i32(5)> = array<u32, i32(5)>( u32(0), u32(23), u32(25), u32(27), u32(29) );
var<private> SHADOW_SEARCH_DISC_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(0.17677700519561768f, 0.0f), vec2<f32>(-0.22577199339866638f, 0.20682600140571594f), vec2<f32>(0.0345579981803894f, -0.39377099275588989f), vec2<f32>(0.28457099199295044f, 0.37117299437522888f), vec2<f32>(-0.52222299575805664f, -0.09237399697303772f), vec2<f32>(0.49469500780105591f, -0.31468498706817627f), vec2<f32>(-0.16546599566936493f, 0.6155250072479248f), vec2<f32>(-0.31556099653244019f, -0.60759401321411133f), vec2<f32>(0.68464201688766479f, 0.25003001093864441f), vec2<f32>(-0.71225601434707642f, 0.2940090000629425f), vec2<f32>(0.3433539867401123f, -0.73372900485992432f), vec2<f32>(0.25372999906539917f, 0.80893200635910034f), vec2<f32>(-0.76474601030349731f, -0.44318601489067078f), vec2<f32>(0.89713400602340698f, -0.19723199307918549f), vec2<f32>(-0.54750698804855347f, 0.77877199649810791f), vec2<f32>(-0.12648700177669525f, -0.97609001398086548f) );
var<private> SHADOW_ROTATIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(1.0f, 0.0f), vec2<f32>(0.92387998104095459f, 0.38268300890922546f), vec2<f32>(0.70710700750350952f, 0.70710700750350952f), vec2<f32>(0.38268300890922546f, 0.92387998104095459f), vec2<f32>(0.0f, 1.0f), vec2<f32>(-0.38268300890922546f, 0.92387998104095459f), vec2<f32>(-0.70710700750350952f, 0.70710700750350952f), vec2<f32>(-0.92387998104095459f, 0.38268300890922546f), vec2<f32>(-1.0f, 0.0f), vec2<f32>(-0.92387998104095459f, -0.38268300890922546f), vec2<f32>(-0.70710700750350952f, -0.70710700750350952f), vec2<f32>(-0.38268300890922546f, -0.92387998104095459f), vec2<f32>(-0.0f, -1.0f), vec2<f32>(0.38268300890922546f, -0.92387998104095459f), vec2<f32>(0.70710700750350952f, -0.70710700750350952f), vec2<f32>(0.92387998104095459f, -0.38268300890922546f) );
var<private> SHADOW_DITHER_0 : array<u32, i32(16)> = array<u32, i32(16)>( u32(0), u32(8), u32(2), u32(10), u32(12), u32(4), u32(14), u32(6), u32(3), u32(11), u32(1), u32(9), u32(15), u32(7), u32(13), u32(5) );
fn normal_basis_0( basis_0 : mat3x3<f32>) -> mat3x3<f32>
{
    return mat3x3<f32>(cross(basis_0[i32(1)], basis_0[i32(2)]), cross(basis_0[i32(2)], basis_0[i32(0)]), cross(basis_0[i32(0)], basis_0[i32(1)]));
}

struct VertexOutput_0
{
    @builtin(position) position_2 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @location(4) world_normal_0 : vec3<f32>,
    @location(5) color_2 : vec4<f32>,
    @interpolate(flat) @location(6) material_1 : u32,
    @location(1) uv_1 : vec2<f32>,
    @location(2) clip_position_0 : vec4<f32>,
    @location(3) previous_clip_position_0 : vec4<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32, @builtin(instance_index) instance_id_0 : u32) -> VertexOutput_0
{
    var mesh_2 : GpuMesh_std430_0 = meshes_0[draw_0.mesh_0];
    var base_vertex_2 : u32;
    if((((instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].flags_0) & (u32(2)))) != u32(0))
    {
        base_vertex_2 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].base_vertex_0;
    }
    else
    {
        base_vertex_2 = mesh_2.base_vertex_1;
    }
    var vertex_0 : MeshVertex_std430_0 = vertices_0[index_0 + base_vertex_2];
    var _S1 : mat4x4<f32> = mat4x4<f32>(instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(3)]);
    var _S2 : vec4<f32> = vec4<f32>(vertex_0.position_0.xyz, 1.0f);
    var world_0 : vec4<f32> = (((_S2) * (_S1)));
    var output_0 : VertexOutput_0;
    output_0.position_2 = (((world_0) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)]))));
    output_0.world_position_0 = world_0.xyz;
    output_0.world_normal_0 = (((vertex_0.normal_0.xyz) * (normal_basis_0(mat3x3<f32>(_S1[i32(0)].xyz, _S1[i32(1)].xyz, _S1[i32(2)].xyz)))));
    var _S3 : vec4<f32>;
    if((frame_0.ambient_0.w) >= 1.5f)
    {
        _S3 = vec4<f32>(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);
    }
    else
    {
        _S3 = vertex_0.color_0;
    }
    output_0.color_2 = _S3;
    output_0.material_1 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].material_0;
    output_0.uv_1 = vertex_0.uv_0.xy;
    output_0.clip_position_0 = output_0.position_2;
    output_0.previous_clip_position_0 = ((((((_S2) * (mat4x4<f32>(instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(3)]))))) * (mat4x4<f32>(frame_0.previous_view_proj_0.data_1[i32(0)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(0)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(0)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(0)][i32(3)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(3)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(3)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(3)]))));
    return output_0;
}

fn motion_vector_0( current_0 : vec4<f32>,  previous_0 : vec4<f32>) -> vec2<f32>
{
    var _S4 : f32 = previous_0.w;
    if(_S4 <= 0.0f)
    {
        return vec2<f32>(0.0f, 0.0f);
    }
    return (current_0.xy / vec2<f32>(current_0.w) - previous_0.xy / vec2<f32>(_S4)) * vec2<f32>(0.5f, -0.5f);
}

fn occlusion_at_0( position_3 : vec2<f32>) -> f32
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((ambient_occlusion_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var _S5 : vec3<i32> = vec3<i32>(min(vec2<i32>(position_3), vec2<i32>(i32(width_0), i32(height_0)) - vec2<i32>(i32(1))), i32(0));
    return (textureLoad((ambient_occlusion_0), ((_S5)).xy, ((_S5)).z).x);
}

fn geometric_normal_of_0( world_position_1 : vec3<f32>,  shading_normal_0 : vec3<f32>) -> vec3<f32>
{
    var facet_0 : vec3<f32> = cross(dpdx(world_position_1), dpdy(world_position_1));
    var extent_0 : f32 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {
        return shading_normal_0;
    }
    var facet_1 : vec3<f32> = facet_0 / vec3<f32>(extent_0);
    var _S6 : vec3<f32>;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {
        _S6 = (vec3<f32>(0) - facet_1);
    }
    else
    {
        _S6 = facet_1;
    }
    return _S6;
}

fn physical_tile_uv_0( world_position_2 : vec3<f32>,  normal_1 : vec3<f32>,  tile_metres_1 : f32) -> vec2<f32>
{
    var axis_0 : vec3<f32> = abs(normal_1);
    var _S7 : f32 = axis_0.x;
    var _S8 : f32 = axis_0.y;
    var _S9 : bool;
    if(_S7 >= _S8)
    {
        _S9 = _S7 >= (axis_0.z);
    }
    else
    {
        _S9 = false;
    }
    var planar_0 : vec2<f32>;
    if(_S9)
    {
        planar_0 = world_position_2.zy;
    }
    else
    {
        if(_S8 >= (axis_0.z))
        {
            planar_0 = world_position_2.xz;
        }
        else
        {
            planar_0 = world_position_2.xy;
        }
    }
    return planar_0 / vec2<f32>(max(tile_metres_1, 0.00009999999747379f));
}

fn froxel_of_0( pixel_0 : vec2<f32>,  depth_0 : f32) -> u32
{
    var _S10 : u32 = max(frame_0.cluster_grid_0.x, u32(1));
    var _S11 : u32 = max(frame_0.cluster_grid_0.y, u32(1));
    var _S12 : u32 = max(frame_0.cluster_grid_0.z, u32(1));
    var _S13 : u32 = max(frame_0.cluster_grid_0.w, u32(1));
    var _S14 : u32 = u32(pixel_0.x) / _S13;
    var _S15 : u32 = min(_S14, _S10 - u32(1));
    var _S16 : u32 = u32(pixel_0.y) / _S13;
    var scale_0 : f32 = 24.0f / log2(10000.0f);
    return (u32(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, f32(_S12 - u32(1)))) * _S11 + min(_S16, _S11 - u32(1))) * _S10 + _S15;
}

fn punctual_falloff_0( distance_0 : f32,  radius_0 : f32) -> f32
{
    var ratio_0 : f32 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    var window_0 : f32 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}

fn spot_cone_0( to_light_0 : vec3<f32>,  axis_1 : vec3<f32>,  cos_outer_0 : f32,  cos_inner_1 : f32) -> f32
{
    return saturate((dot((vec3<f32>(0) - to_light_0), normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}

fn ggx_lobe_0( alpha2_0 : f32,  f0_0 : vec3<f32>,  n_dot_l_0 : f32,  n_dot_v_0 : f32,  n_dot_h_0 : f32,  v_dot_h_0 : f32) -> vec3<f32>
{
    var shape_0 : f32 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;
    var _S17 : f32 = 1.0f - alpha2_0;
    var grazing_0 : f32 = 1.0f - v_dot_h_0;
    var grazing2_0 : f32 = grazing_0 * grazing_0;
    return vec3<f32>((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S17 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S17 + alpha2_0), 9.99999997475242708e-07f)))) * (f0_0 + (vec3<f32>(1.0f, 1.0f, 1.0f) - f0_0) * vec3<f32>((grazing2_0 * grazing2_0 * grazing_0)));
}

fn shadow_normal_offset_0( geometric_normal_0 : vec3<f32>,  to_light_1 : vec3<f32>) -> f32
{
    var cosine_0 : f32 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}

fn shadow_rotation_0( pixel_1 : vec2<f32>) -> vec2<f32>
{
    var cell_0 : vec2<u32> = (vec2<u32>(pixel_1) & (vec2<u32>(u32(3))));
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * u32(4) + cell_0.x]];
}

fn atlas_uv_0( tile_0 : u32,  tile_uv_0 : vec2<f32>) -> vec2<f32>
{
    return (vec2<f32>(f32(tile_0 % u32(4)), f32(tile_0 / u32(4))) + tile_uv_0) / vec2<f32>(4.0f, 4.0f);
}

fn sun_penumbra_texels_0( cascade_0 : u32,  tile_uv_1 : vec2<f32>,  reference_0 : f32,  rotation_0 : vec2<f32>) -> f32
{
    var texel_0 : vec2<f32> = frame_0.shadow_params_0.xy;
    const grid_0 : vec2<f32> = vec2<f32>(4.0f, 4.0f);
    var _S18 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_0 * grid_0;
    const _S19 : vec2<f32> = vec2<f32>(1.0f, 1.0f);
    var _S20 : vec2<f32> = _S19 / texel_0;
    var index_1 : u32 = u32(0);
    var sum_0 : f32 = 0.0f;
    var found_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_1 < u32(16))
        {
        }
        else
        {
            break;
        }
        var spoke_0 : vec2<f32> = SHADOW_SEARCH_DISC_0[index_1] * vec2<f32>(8.0f);
        var _S21 : f32 = spoke_0.x;
        var _S22 : f32 = rotation_0.x;
        var _S23 : f32 = spoke_0.y;
        var _S24 : f32 = rotation_0.y;
        var _S25 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + vec2<f32>(_S21 * _S22 - _S23 * _S24, _S21 * _S24 + _S23 * _S22) * texel_0 * grid_0, _S18, vec2<f32>(1.0f) - _S18)) * _S20, _S20 - _S19)), i32(0));
        var depth_1 : f32 = (textureLoad((shadow_atlas_0), ((_S25)).xy, ((_S25)).z));
        if(depth_1 > reference_0)
        {
            var found_1 : f32 = found_0 + 1.0f;
            sum_0 = sum_0 + depth_1;
            found_0 = found_1;
        }
        index_1 = index_1 + u32(1);
    }
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }
    var _S26 : f32 = 2.0f * frame_0.cascade_far_0[cascade_0];
    return clamp((sum_0 / found_0 - reference_0) * (_S26 + 40.0f) * 0.01999999955296516f / (_S26 / 768.0f), 2.0f, 8.0f);
}

fn tile_tap_0( tile_1 : u32,  tile_uv_2 : vec2<f32>,  spoke_1 : vec2<f32>,  rotation_1 : vec2<f32>,  reference_1 : f32) -> f32
{
    var texel_1 : vec2<f32> = frame_0.shadow_params_0.xy;
    const grid_1 : vec2<f32> = vec2<f32>(4.0f, 4.0f);
    var tile_min_0 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_1 * grid_1;
    var _S27 : f32 = spoke_1.x;
    var _S28 : f32 = rotation_1.x;
    var _S29 : f32 = spoke_1.y;
    var _S30 : f32 = rotation_1.y;
    return (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + vec2<f32>(_S27 * _S28 - _S29 * _S30, _S27 * _S30 + _S29 * _S28) * texel_1 * grid_1, tile_min_0, vec2<f32>(1.0f) - tile_min_0))), (reference_1)));
}

fn tile_pcf_0( tile_2 : u32,  tile_uv_3 : vec2<f32>,  reference_2 : f32,  pixel_2 : vec2<f32>,  radius_1 : f32) -> f32
{
    var _S31 : vec2<f32> = shadow_rotation_0(pixel_2);
    var spot_0 : u32 = u32(0);
    var probe_0 : f32 = 0.0f;
    for(;;)
    {
        if(spot_0 < u32(5))
        {
        }
        else
        {
            break;
        }
        var probe_1 : f32 = probe_0 + tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * vec2<f32>(radius_1), _S31, reference_2);
        spot_0 = spot_0 + u32(1);
        probe_0 = probe_1;
    }
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }
    var index_2 : u32 = u32(0);
    var visibility_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_2 < u32(32))
        {
        }
        else
        {
            break;
        }
        var visibility_1 : f32 = visibility_0 + tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[index_2] * vec2<f32>(radius_1), _S31, reference_2);
        index_2 = index_2 + u32(1);
        visibility_0 = visibility_1;
    }
    return visibility_0 / 32.0f;
}

fn cascade_visibility_0( cascade_1 : u32,  world_position_3 : vec3<f32>,  to_light_2 : vec3<f32>,  geometric_normal_1 : vec3<f32>,  pixel_3 : vec2<f32>) -> f32
{
    var texel_world_0 : f32 = 2.0f * frame_0.cascade_far_0[cascade_1] / 768.0f;
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_3 + geometric_normal_1 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2))) + to_light_2 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.z)), 1.0f)) * (mat4x4<f32>(frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(3)]))));
    var ndc_0 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S32 : bool;
    if((any(((abs(ndc_0.xy)) > vec2<f32>(1.0f)))))
    {
        _S32 = true;
    }
    else
    {
        _S32 = (ndc_0.z) <= 0.0f;
    }
    if(_S32)
    {
        return 1.0f;
    }
    var tile_uv_4 : vec2<f32> = vec2<f32>(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);
    var _S33 : f32 = ndc_0.z;
    return tile_pcf_0(cascade_1, tile_uv_4, _S33, pixel_3, sun_penumbra_texels_0(cascade_1, tile_uv_4, _S33, shadow_rotation_0(pixel_3)));
}

fn sun_visibility_0( world_position_4 : vec3<f32>,  to_light_3 : vec3<f32>,  n_dot_l_1 : f32,  geometric_normal_2 : vec3<f32>,  pixel_4 : vec2<f32>) -> f32
{
    var cascade_2 : u32;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }
    var eye_distance_0 : f32 = length(world_position_4 - frame_0.camera_position_0.xyz);
    var index_3 : u32 = u32(0);
    for(;;)
    {
        if(index_3 < u32(2))
        {
        }
        else
        {
            cascade_2 = u32(1);
            break;
        }
        if(eye_distance_0 < (frame_0.cascade_far_0[index_3]))
        {
            cascade_2 = index_3;
            break;
        }
        index_3 = index_3 + u32(1);
    }
    var visibility_2 : f32 = cascade_visibility_0(cascade_2, world_position_4, to_light_3, geometric_normal_2, pixel_4);
    var _S34 : u32 = cascade_2 + u32(1);
    if(_S34 >= u32(2))
    {
        return visibility_2;
    }
    var band_0 : f32 = frame_0.cascade_far_0[cascade_2] * 0.10000000149011612f;
    var blend_0 : f32 = saturate((eye_distance_0 - (frame_0.cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return visibility_2;
    }
    return mix(visibility_2, cascade_visibility_0(_S34, world_position_4, to_light_3, geometric_normal_2, pixel_4), blend_0);
}

fn point_face_0( from_light_0 : vec3<f32>) -> u32
{
    var axis_2 : vec3<f32> = abs(from_light_0);
    var _S35 : f32 = axis_2.x;
    var _S36 : f32 = axis_2.y;
    var _S37 : bool;
    if(_S35 >= _S36)
    {
        _S37 = _S35 >= (axis_2.z);
    }
    else
    {
        _S37 = false;
    }
    var _S38 : u32;
    if(_S37)
    {
        if((from_light_0.x) >= 0.0f)
        {
            _S38 = u32(0);
        }
        else
        {
            _S38 = u32(1);
        }
        return _S38;
    }
    if(_S36 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {
            _S38 = u32(2);
        }
        else
        {
            _S38 = u32(3);
        }
        return _S38;
    }
    if((from_light_0.z) >= 0.0f)
    {
        _S38 = u32(4);
    }
    else
    {
        _S38 = u32(5);
    }
    return _S38;
}

fn light_tile_0( tile_3 : u32) -> u32
{
    return u32(2) + tile_3;
}

fn punctual_visibility_0( tile_4 : u32,  world_position_5 : vec3<f32>,  to_light_4 : vec3<f32>,  n_dot_l_2 : f32,  texel_world_1 : f32,  geometric_normal_3 : vec3<f32>,  pixel_5 : vec2<f32>) -> f32
{
    var clip_1 : vec4<f32> = (((vec4<f32>(world_position_5 + geometric_normal_3 * vec3<f32>((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4))) + to_light_4 * vec3<f32>((texel_world_1 * 2.0f)), 1.0f)) * (mat4x4<f32>(frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(0)][i32(0)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(1)][i32(0)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(2)][i32(0)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(3)][i32(0)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(0)][i32(1)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(1)][i32(1)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(2)][i32(1)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(3)][i32(1)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(0)][i32(2)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(1)][i32(2)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(2)][i32(2)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(3)][i32(2)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(0)][i32(3)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(1)][i32(3)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(2)][i32(3)], frame_0.light_view_proj_0.data_3[tile_4].data_1[i32(3)][i32(3)]))));
    var _S39 : f32 = clip_1.w;
    if(_S39 <= 0.0f)
    {
        return 1.0f;
    }
    var ndc_1 : vec3<f32> = clip_1.xyz / vec3<f32>(_S39);
    var _S40 : bool;
    if((any(((abs(ndc_1.xy)) > vec2<f32>(1.0f)))))
    {
        _S40 = true;
    }
    else
    {
        _S40 = (ndc_1.z) <= 0.0f;
    }
    if(_S40)
    {
        _S40 = true;
    }
    else
    {
        _S40 = (ndc_1.z) > 1.0f;
    }
    if(_S40)
    {
        return 1.0f;
    }
    return tile_pcf_0(light_tile_0(tile_4), vec2<f32>(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f);
}

fn point_visibility_0( light_0 : ptr<function, GpuLight_std430_0>,  base_1 : u32,  world_position_6 : vec3<f32>,  to_light_5 : vec3<f32>,  n_dot_l_3 : f32,  geometric_normal_4 : vec3<f32>,  pixel_6 : vec2<f32>) -> f32
{
    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }
    var from_light_1 : vec3<f32> = world_position_6 - (*light_0).position_1.xyz;
    return punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_6, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6);
}

fn spot_visibility_0( light_1 : ptr<function, GpuLight_std430_0>,  tile_5 : u32,  world_position_7 : vec3<f32>,  to_light_6 : vec3<f32>,  n_dot_l_4 : f32,  geometric_normal_5 : vec3<f32>,  pixel_7 : vec2<f32>) -> f32
{
    if(n_dot_l_4 <= 0.0f)
    {
        return 1.0f;
    }
    var cos_outer_1 : f32 = (*light_1).direction_0.w;
    return punctual_visibility_0(tile_5, world_position_7, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_7 - (*light_1).position_1.xyz, normalize((*light_1).direction_0.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7);
}

fn decode_specular_albedo_0( texel_2 : vec2<f32>) -> f32
{
    return (texel_2.x * 65280.0f + texel_2.y * 255.0f) / 65535.0f;
}

fn specular_albedo_at_0( n_dot_v_1 : f32,  roughness_1 : f32) -> f32
{
    var width_1 : u32;
    var height_1 : u32;
    {var dim = textureDimensions((specular_albedo_0));((width_1)) = dim.x;((height_1)) = dim.y;};
    var extent_1 : vec2<f32> = vec2<f32>(f32(width_1), f32(height_1));
    var scaled_0 : vec2<f32> = vec2<f32>(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - vec2<f32>(0.5f);
    var _S41 : vec2<f32> = vec2<f32>(1.0f);
    var _S42 : vec2<f32> = extent_1 - _S41;
    var low_0 : vec2<f32> = clamp(floor(scaled_0), vec2<f32>(0.0f, 0.0f), _S42);
    var weight_0 : vec2<f32> = clamp(scaled_0 - low_0, vec2<f32>(0.0f), vec2<f32>(1.0f));
    var _S43 : vec2<i32> = vec2<i32>(low_0);
    var _S44 : vec2<i32> = vec2<i32>(min(low_0 + _S41, _S42));
    var _S45 : i32 = _S43.x;
    var _S46 : i32 = _S43.y;
    var _S47 : vec3<i32> = vec3<i32>(_S45, _S46, i32(0));
    var _S48 : i32 = _S44.x;
    var _S49 : vec3<i32> = vec3<i32>(_S48, _S46, i32(0));
    var _S50 : f32 = weight_0.x;
    var _S51 : i32 = _S44.y;
    var _S52 : vec3<i32> = vec3<i32>(_S45, _S51, i32(0));
    var _S53 : vec3<i32> = vec3<i32>(_S48, _S51, i32(0));
    return mix(mix(decode_specular_albedo_0((textureLoad((specular_albedo_0), ((_S47)).xy, ((_S47)).z).xy)), decode_specular_albedo_0((textureLoad((specular_albedo_0), ((_S49)).xy, ((_S49)).z).xy)), _S50), mix(decode_specular_albedo_0((textureLoad((specular_albedo_0), ((_S52)).xy, ((_S52)).z).xy)), decode_specular_albedo_0((textureLoad((specular_albedo_0), ((_S53)).xy, ((_S53)).z).xy)), _S50), weight_0.y);
}

fn specular_compensation_0( f0_1 : vec3<f32>,  n_dot_v_2 : f32,  roughness_2 : f32) -> vec3<f32>
{
    return vec3<f32>(1.0f, 1.0f, 1.0f) + f0_1 * vec3<f32>((1.0f / clamp(specular_albedo_at_0(n_dot_v_2, roughness_2), 0.00009999999747379f, 1.0f) - 1.0f));
}

fn sky_irradiance_0( normal_2 : vec3<f32>) -> vec3<f32>
{
    var basis_1 : vec4<f32> = vec4<f32>(normal_2, 1.0f);
    return max(vec3<f32>(dot(frame_0.sky_sh_r_0, basis_1), dot(frame_0.sky_sh_g_0, basis_1), dot(frame_0.sky_sh_b_0, basis_1)), vec3<f32>(0.0f, 0.0f, 0.0f));
}

struct GpuProbe_0
{
     sh_r_0 : vec4<f32>,
     sh_g_0 : vec4<f32>,
     sh_b_0 : vec4<f32>,
};

fn probe_at_0( cell_1 : vec3<u32>) -> GpuProbe_0
{
    var _S54 : GpuProbe_std430_0 = probes_0[min((cell_1.z * frame_0.probe_counts_0.y + cell_1.y) * frame_0.probe_counts_0.x + cell_1.x, max(frame_0.probe_counts_0.w, u32(1)) - u32(1))];
    var _S55 : GpuProbe_0 = GpuProbe_0( _S54.sh_r_0, _S54.sh_g_0, _S54.sh_b_0 );
    return _S55;
}

fn lerp_probe_0( a_0 : GpuProbe_0,  b_0 : GpuProbe_0,  t_0 : f32) -> GpuProbe_0
{
    var blended_0 : GpuProbe_0;
    var _S56 : vec4<f32> = vec4<f32>(t_0);
    blended_0.sh_r_0 = mix(a_0.sh_r_0, b_0.sh_r_0, _S56);
    blended_0.sh_g_0 = mix(a_0.sh_g_0, b_0.sh_g_0, _S56);
    blended_0.sh_b_0 = mix(a_0.sh_b_0, b_0.sh_b_0, _S56);
    return blended_0;
}

fn probe_irradiance_0( world_position_8 : vec3<f32>,  normal_3 : vec3<f32>) -> vec3<f32>
{
    var _S57 : vec3<f32> = vec3<f32>(1.0f);
    const _S58 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var last_0 : vec3<f32> = max(vec3<f32>(frame_0.probe_counts_0.xyz) - _S57, _S58);
    var grid_2 : vec3<f32> = clamp((world_position_8 - frame_0.probe_origin_0.xyz) * frame_0.probe_inv_spacing_0.xyz, _S58, last_0);
    var base_2 : vec3<f32> = floor(grid_2);
    var f_0 : vec3<f32> = grid_2 - base_2;
    var _S59 : vec3<u32> = vec3<u32>(base_2);
    var _S60 : vec3<u32> = vec3<u32>(min(base_2 + _S57, last_0));
    var _S61 : u32 = _S59.x;
    var _S62 : u32 = _S59.y;
    var _S63 : u32 = _S59.z;
    var _S64 : u32 = _S60.x;
    var _S65 : f32 = f_0.x;
    var _S66 : u32 = _S60.y;
    var _S67 : u32 = _S60.z;
    var _S68 : f32 = f_0.y;
    var cell_2 : GpuProbe_0 = lerp_probe_0(lerp_probe_0(lerp_probe_0(probe_at_0(vec3<u32>(_S61, _S62, _S63)), probe_at_0(vec3<u32>(_S64, _S62, _S63)), _S65), lerp_probe_0(probe_at_0(vec3<u32>(_S61, _S66, _S63)), probe_at_0(vec3<u32>(_S64, _S66, _S63)), _S65), _S68), lerp_probe_0(lerp_probe_0(probe_at_0(vec3<u32>(_S61, _S62, _S67)), probe_at_0(vec3<u32>(_S64, _S62, _S67)), _S65), lerp_probe_0(probe_at_0(vec3<u32>(_S61, _S66, _S67)), probe_at_0(vec3<u32>(_S64, _S66, _S67)), _S65), _S68), f_0.z);
    var basis_2 : vec4<f32> = vec4<f32>(normal_3, 1.0f);
    return max(vec3<f32>(dot(cell_2.sh_r_0, basis_2), dot(cell_2.sh_g_0, basis_2), dot(cell_2.sh_b_0, basis_2)), _S58);
}

fn emissive_of_0( material_2 : ptr<function, GpuMaterial_std430_0>) -> vec3<f32>
{
    return vec3<f32>((*material_2).emissive_r_0, (*material_2).emissive_g_0, (*material_2).emissive_b_0);
}

fn fog_exp_neg_0( x_0 : f32) -> f32
{
    var clamped_0 : f32 = clamp(x_0, -87.0f, 87.0f);
    var n_0 : f32 = floor(clamped_0 * 1.4426950216293335f + 0.5f);
    var _S69 : f32 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);
    var kernel_0 : f32 = 0.0001984127011383f;
    var term_0 : i32 = i32(6);
    for(;;)
    {
        if(term_0 >= i32(0))
        {
        }
        else
        {
            break;
        }
        var _S70 : f32 = kernel_0 * _S69 + FOG_KERNEL_0[term_0];
        var term_1 : i32 = term_0 - i32(1);
        kernel_0 = _S70;
        term_0 = term_1;
    }
    return kernel_0 * (bitcast<f32>(((u32(i32(127) - i32(n_0)) << (u32(23))))));
}

fn fog_one_minus_exp_over_0( d_0 : f32) -> f32
{
    if((abs(d_0)) < 0.125f)
    {
        var _S71 : f32 = - d_0;
        var series_0 : f32 = 0.00833333376795053f;
        var term_2 : i32 = i32(3);
        for(;;)
        {
            if(term_2 >= i32(0))
            {
            }
            else
            {
                break;
            }
            var _S72 : f32 = series_0 * _S71 + FOG_RATIO_KERNEL_0[term_2];
            var term_3 : i32 = term_2 - i32(1);
            series_0 = _S72;
            term_2 = term_3;
        }
        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}

fn fog_optical_depth_0( density_0 : f32,  falloff_0 : f32,  height_a_0 : f32,  height_b_0 : f32,  distance_1 : f32) -> f32
{
    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_1, 0.0f, 32.0f);
    }
    return clamp(density_0 * distance_1 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}

fn fog_transmittance_0( optical_depth_0 : f32) -> f32
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}

struct FragmentOutput_0
{
    @location(0) lit_0 : vec4<f32>,
    @location(1) reflectivity_0 : vec4<f32>,
    @location(2) motion_0 : vec2<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_9 : vec3<f32>,
    @location(4) world_normal_1 : vec3<f32>,
    @location(5) color_3 : vec4<f32>,
    @interpolate(flat) @location(6) material_3 : u32,
    @location(1) uv_2 : vec2<f32>,
    @location(2) clip_position_1 : vec4<f32>,
    @location(3) previous_clip_position_1 : vec4<f32>,
};

@fragment
fn fragmentMain( _S73 : pixelInput_0, @builtin(position) position_4 : vec4<f32>) -> FragmentOutput_0
{
    var normal_4 : vec3<f32> = normalize(_S73.world_normal_1);
    var motion_1 : vec2<f32> = motion_vector_0(_S73.clip_position_1, _S73.previous_clip_position_1);
    if((frame_0.ambient_0.w) >= 4.5f)
    {
        var moved_0 : FragmentOutput_0;
        moved_0.lit_0 = vec4<f32>(motion_1 * vec2<f32>(8.0f) + vec2<f32>(0.5f), 0.0f, 1.0f);
        moved_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        moved_0.motion_0 = motion_1;
        return moved_0;
    }
    if((frame_0.ambient_0.w) >= 3.5f)
    {
        var value_0 : f32 = occlusion_at_0(position_4.xy);
        var occlusion_0 : FragmentOutput_0;
        occlusion_0.lit_0 = vec4<f32>(value_0, value_0, value_0, 1.0f);
        occlusion_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        occlusion_0.motion_0 = motion_1;
        return occlusion_0;
    }
    if((frame_0.ambient_0.w) >= 1.5f)
    {
        var tint_0 : FragmentOutput_0;
        tint_0.lit_0 = vec4<f32>(_S73.color_3.xyz, 1.0f);
        tint_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        tint_0.motion_0 = motion_1;
        return tint_0;
    }
    if((frame_0.ambient_0.w) >= 0.5f)
    {
        var normals_0 : FragmentOutput_0;
        var _S74 : vec3<f32> = vec3<f32>(0.5f);
        normals_0.lit_0 = vec4<f32>(normal_4 * _S74 + _S74, 1.0f);
        normals_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        normals_0.motion_0 = motion_1;
        return normals_0;
    }
    var to_eye_0 : vec3<f32> = normalize(frame_0.camera_position_0.xyz - _S73.world_position_9);
    var _S75 : vec3<f32> = geometric_normal_of_0(_S73.world_position_9, normal_4);
    var _S76 : GpuMaterial_std430_0 = materials_0[_S73.material_3];
    var uv_3 : vec2<f32>;
    if((_S76.tiling_0) == u32(1))
    {
        uv_3 = physical_tile_uv_0(_S73.world_position_9, normal_4, _S76.tile_metres_0);
    }
    else
    {
        uv_3 = _S73.uv_2;
    }
    var _S77 : vec3<f32> = vec3<f32>(uv_3, f32(_S76.base_color_texture_0));
    var albedo_0 : vec4<f32> = _S73.color_3 * _S76.base_color_0 * (textureSample((base_color_textures_0), (base_color_sampler_0), ((_S77)).xy, i32(((_S77)).z)));
    var metallic_1 : f32 = saturate(_S76.metallic_0);
    var roughness_3 : f32 = clamp(_S76.roughness_0, 0.04500000178813934f, 1.0f);
    var alpha_0 : f32 = roughness_3 * roughness_3;
    var _S78 : f32 = alpha_0 * alpha_0;
    var _S79 : vec3<f32> = albedo_0.xyz;
    var f0_2 : vec3<f32> = mix(vec3<f32>(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S79, vec3<f32>(metallic_1));
    var diffuse_albedo_0 : vec3<f32> = _S79 * vec3<f32>((1.0f - metallic_1));
    var _S80 : f32 = max(dot(normal_4, to_eye_0), 0.00009999999747379f);
    var _S81 : vec2<f32> = position_4.xy;
    var _S82 : u32 = froxel_of_0(_S81, (((vec4<f32>(_S73.world_position_9, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)])))).w);
    var base_3 : u32 = _S82 * u32(17);
    var _S83 : u32 = min(cluster_lights_0[base_3], u32(16));
    const _S84 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slot_0 : u32 = u32(0);
    var direct_0 : vec3<f32> = _S84;
    var gloss_0 : vec3<f32> = _S84;
    for(;;)
    {
        if(slot_0 < _S83)
        {
        }
        else
        {
            break;
        }
        var _S85 : GpuLight_std430_0 = lights_0[cluster_lights_0[base_3 + u32(1) + slot_0]];
        var _S86 : u32 = _S85.kind_0;
        var _S87 : bool = (_S85.kind_0) == u32(0);
        var to_light_7 : vec3<f32>;
        var reach_0 : f32;
        if(_S87)
        {
            to_light_7 = normalize(_S85.direction_0.xyz);
            reach_0 = 1.0f;
        }
        else
        {
            var offset_0 : vec3<f32> = _S85.position_1.xyz - _S73.world_position_9;
            var distance_2 : f32 = length(offset_0);
            var to_light_8 : vec3<f32> = offset_0 / vec3<f32>(max(distance_2, 9.99999997475242708e-07f));
            var reach_1 : f32 = punctual_falloff_0(distance_2, _S85.position_1.w);
            if(_S86 == u32(2))
            {
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S85.direction_0.xyz, _S85.direction_0.w, _S85.cos_inner_0);
            }
            else
            {
                reach_0 = reach_1;
            }
            to_light_7 = to_light_8;
        }
        var n_dot_l_5 : f32 = dot(normal_4, to_light_7);
        var _S88 : f32 = max(n_dot_l_5, 0.0f);
        var half_vector_0 : vec3<f32> = normalize(to_light_7 + to_eye_0);
        var specular_0 : vec3<f32> = ggx_lobe_0(_S78, f0_2, _S88, _S80, max(dot(normal_4, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * vec3<f32>(_S88);
        var reach_2 : f32;
        if(_S87)
        {
            reach_2 = sun_visibility_0(_S73.world_position_9, to_light_7, n_dot_l_5, _S75, _S81);
        }
        else
        {
            if(_S86 == u32(1))
            {
                var _S89 : u32 = _S85.shadow_tile_0;
                if((_S85.shadow_tile_0) <= u32(8))
                {
                    var _S90 : f32 = point_visibility_0(&(_S85), _S89, _S73.world_position_9, to_light_7, n_dot_l_5, _S75, _S81);
                    reach_2 = reach_0 * _S90;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
            else
            {
                var _S91 : u32 = _S85.shadow_tile_0;
                if((_S85.shadow_tile_0) < u32(14))
                {
                    var _S92 : f32 = spot_visibility_0(&(_S85), _S91, _S73.world_position_9, to_light_7, n_dot_l_5, _S75, _S81);
                    reach_2 = reach_0 * _S92;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
        }
        var _S93 : vec3<f32> = _S85.color_1.xyz;
        var direct_1 : vec3<f32> = direct_0 + _S93 * vec3<f32>((_S88 * reach_2));
        var gloss_1 : vec3<f32> = gloss_0 + _S93 * (specular_0 * vec3<f32>(reach_2));
        slot_0 = slot_0 + u32(1);
        direct_0 = direct_1;
        gloss_0 = gloss_1;
    }
    var lit_1 : vec3<f32> = diffuse_albedo_0 * ((frame_0.ambient_0.xyz + sky_irradiance_0(normal_4) + probe_irradiance_0(_S73.world_position_9, normal_4)) * vec3<f32>(occlusion_at_0(_S81)) + direct_0) + gloss_0 * specular_compensation_0(f0_2, _S80, roughness_3);
    var _S94 : vec3<f32> = emissive_of_0(&(_S76));
    var fog_survives_0 : f32 = fog_transmittance_0(fog_optical_depth_0(frame_0.fog_params_0.x, frame_0.fog_params_0.y, frame_0.camera_position_0.y - frame_0.fog_params_0.z, _S73.world_position_9.y - frame_0.fog_params_0.z, length(frame_0.camera_position_0.xyz - _S73.world_position_9)));
    var output_1 : FragmentOutput_0;
    output_1.lit_0 = vec4<f32>((lit_1 + _S94) * vec3<f32>(fog_survives_0) + frame_0.fog_color_0.xyz * vec3<f32>((1.0f - fog_survives_0)), albedo_0.w);
    output_1.reflectivity_0 = vec4<f32>(f0_2, floor(roughness_3 * 255.0f + 0.5f) / 255.0f);
    output_1.motion_0 = motion_1;
    return output_1;
}

