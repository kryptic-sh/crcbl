#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2737 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2732
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 3734
constant array<float3, int(2)> CASCADE_TINTS_0 = { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) };

#line 3217
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 3004
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 3064
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 3079
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 3107
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1297
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1982
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1982
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint previous_base_vertex_0;
    uint pad1_1;
    uint pad2_0;
};


#line 874
struct GpuMesh_0
{
    uint base_vertex_1;
    uint base_index_0;
    uint index_count_0;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
    float uv_scale_u_0;
    float uv_scale_v_0;
    float uv_offset_u_0;
    float uv_offset_v_0;
    uint flags_1;
};


#line 1988
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1988
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 363 "shaders/mesh.slang"
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint4 cluster_grid_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_0;
    uint4 probe_counts_0;
    uint4 probe_levels_0;
    array<float4, int(4)> probe_level_origin_0;
    array<float4, int(4)> probe_level_inv_spacing_0;
    array<uint4, int(4)> probe_level_offset_0;
    float4 lod_params_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sky_sh_r_0;
    float4 sky_sh_g_0;
    float4 sky_sh_b_0;
    _MatrixStorage_float4x4_ColMajornatural_1 previous_view_proj_0;
    uint4 vertex_pool_0;
    array<float4, int(16)> shadow_atlas_rect_0;
    uint4 shadow_filter_0;
};


#line 363
struct GpuMaterial_natural_0
{
    packed_float4 base_color_0;
    uint color_normal_pages_0;
    float metallic_0;
    float roughness_0;
    uint tiling_0;
    float tile_metres_0;
    float emissive_r_0;
    float emissive_g_0;
    float emissive_b_0;
    uint mro_emissive_pages_0;
    float normal_scale_0;
    float alpha_cutoff_0;
    uint flags_2;
};


#line 363
struct GpuLight_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 direction_0;
    packed_float4 tangent_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint flags_3;
};


#line 363
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 363
struct KernelContext_0
{
    DrawConstants_0 constant* draw_0;
    uint device* visible_instances_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    FrameUniforms_natural_0 constant* frame_0;
    uint device* vertices_0;
    texture2d<float, access::sample> ambient_occlusion_0;
    GpuMaterial_natural_0 device* materials_0;
    texture2d_array<float, access::sample> base_color_textures_0;
    sampler base_color_sampler_0;
    texture2d_array<float, access::sample> normal_textures_0;
    uint device* cluster_lights_0;
    texture2d<float, access::sample> specular_dfg_0;
    GpuLight_natural_0 device* lights_0;
    texture2d<float, access::sample> ltc_matrix_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> contact_shadow_0;
    GpuProbe_natural_0 device* probes_0;
    texture2d_array<float, access::sample> probe_visibility_0;
};


#line 1340
float3 load_position_0(uint at_0, KernelContext_0 thread* kernelContext_0)
{
    uint word_0 = at_0 * 3U;
    return float3((as_type<float>((kernelContext_0->vertices_0[word_0]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 1U]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 2U]))));
}


#line 196
float dequantise_snorm_0(int lane_0)
{
    return max(float(lane_0) / 32767.0f, -1.0f);
}


float4 unpack_snorm16x4_0(uint low_0, uint high_0)
{
    return float4(dequantise_snorm_0((as_type<int>((low_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((low_0))) >> 16U), dequantise_snorm_0((as_type<int>((high_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((high_0))) >> 16U));
}


#line 228
float3 rotate_by_0(float4 q_0, float3 v_0)
{
    float3 _S1 = q_0.xyz;

#line 230
    float3 t_0 = float3(2.0f)  * cross(_S1, v_0);
    return v_0 + float3(q_0.w)  * t_0 + cross(_S1, t_0);
}


#line 186
struct TangentFrame_0
{
    float3 tangent_1;
    float3 bitangent_0;
    float3 normal_0;
};


#line 242
TangentFrame_0 decode_qtangent_0(float4 lanes_0)
{
    float4 q_1 = normalize(lanes_0);
    thread TangentFrame_0 basis_0;
    float3 _S2 = rotate_by_0(q_1, float3(1.0f, 0.0f, 0.0f));

#line 246
    (&basis_0)->tangent_1 = _S2;
    float3 _S3 = rotate_by_0(q_1, float3(0.0f, 0.0f, 1.0f));

#line 247
    (&basis_0)->normal_0 = _S3;
    float3 _S4 = cross(_S3, _S2);

#line 248
    float _S5;

#line 248
    if((lanes_0.w) < 0.0f)
    {

#line 248
        _S5 = -1.0f;

#line 248
    }
    else
    {

#line 248
        _S5 = 1.0f;

#line 248
    }

#line 248
    (&basis_0)->bitangent_0 = _S4 * float3(_S5) ;
    return basis_0;
}


#line 211
float2 unpack_unorm16x2_0(uint word_1)
{
    return float2(float(word_1 & 65535U), float(word_1 >> 16U)) / float2(65535.0f) ;
}


float4 unpack_rgba8_0(uint word_2)
{
    return float4(float(word_2 & 255U), float((word_2 >> 8U) & 255U), float((word_2 >> 16U) & 255U), float(word_2 >> 24U)) / float4(255.0f) ;
}


#line 257
struct MeshVertex_0
{
    float3 position_1;
    TangentFrame_0 basis_1;
    float2 uv0_0;
    float4 color_1;
};


#line 1351
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1354
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1846
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1969
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1969
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1971
        word_4 = 1U;

#line 1971
    }
    else
    {

#line 1971
        word_4 = 0U;

#line 1971
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1975
        word_4 = word_4 | 2U;

#line 1975
    }

#line 1974
    return word_4;
}


#line 1974
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 2090
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 2090
    thread KernelContext_0 kernelContext_2;

#line 2090
    (&kernelContext_2)->draw_0 = draw_1;

#line 2090
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 2090
    (&kernelContext_2)->instances_0 = instances_1;

#line 2090
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 2090
    (&kernelContext_2)->frame_0 = frame_1;

#line 2090
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 2090
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2090
    (&kernelContext_2)->materials_0 = materials_1;

#line 2090
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 2090
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 2090
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 2090
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 2090
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 2090
    (&kernelContext_2)->lights_0 = lights_1;

#line 2090
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 2090
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 2090
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 2090
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 2090
    (&kernelContext_2)->probes_0 = probes_1;

#line 2090
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 2090
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 2093
    uint base_vertex_2;

#line 2099
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 2099
        base_vertex_2 = _S7->base_vertex_0;

#line 2099
    }
    else
    {

#line 2099
        base_vertex_2 = mesh_2.base_vertex_1;

#line 2099
    }

#line 2099
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 2099
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 2099
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 2102
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 2123
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 2123
    thread KernelContext_0 kernelContext_3;

#line 2123
    (&kernelContext_3)->draw_0 = draw_2;

#line 2123
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 2123
    (&kernelContext_3)->instances_0 = instances_2;

#line 2123
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 2123
    (&kernelContext_3)->frame_0 = frame_2;

#line 2123
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 2123
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 2123
    (&kernelContext_3)->materials_0 = materials_2;

#line 2123
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 2123
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 2123
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 2123
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 2123
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 2123
    (&kernelContext_3)->lights_0 = lights_2;

#line 2123
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 2123
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 2123
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 2123
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 2123
    (&kernelContext_3)->probes_0 = probes_2;

#line 2123
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 2123
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 5139
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 5141
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 5015
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 5015
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 5021
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4749
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4753
    float _S16 = axis_0.y;

#line 4753
    bool _S17;

#line 4753
    if(_S15 >= _S16)
    {

#line 4753
        _S17 = _S15 >= (axis_0.z);

#line 4753
    }
    else
    {

#line 4753
        _S17 = false;

#line 4753
    }

#line 4753
    float2 planar_0;

#line 4753
    if(_S17)
    {

#line 4753
        planar_0 = world_position_0.zy;

#line 4753
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4757
            planar_0 = world_position_0.xz;

#line 4757
        }
        else
        {

#line 4757
            planar_0 = world_position_0.xy;

#line 4757
        }

#line 4753
    }

#line 4765
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1058
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) & 65535U;
}


#line 1464
float4 base_color_texel_0(const GpuMaterial_natural_0 thread* material_2, float2 uv_0, KernelContext_0 thread* kernelContext_5)
{

#line 1464
    uint _S18 = base_color_layer_0(material_2);


    bool named_0 = _S18 != 65535U;

#line 1467
    uint _S19;

    if(named_0)
    {

#line 1469
        _S19 = _S18;

#line 1469
    }
    else
    {

#line 1469
        _S19 = 0U;

#line 1469
    }

#line 1469
    float3 _S20 = float3(uv_0, float(_S19));

#line 1468
    float4 texel_0 = ((kernelContext_5->base_color_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S20)).xy, uint(((_S20)).z)));

#line 1468
    float4 _S21;

    if(named_0)
    {

#line 1470
        _S21 = texel_0;

#line 1470
    }
    else
    {

#line 1470
        _S21 = float4(1.0f, 1.0f, 1.0f, 1.0f);

#line 1470
    }

#line 1470
    return _S21;
}


#line 1137
bool alpha_masked_0(const GpuMaterial_natural_0 thread* material_3, float alpha_0)
{

#line 1137
    bool _S22;

    if(((material_3->flags_2) & 1U) != 0U)
    {

#line 1139
        _S22 = alpha_0 < (material_3->alpha_cutoff_0);

#line 1139
    }
    else
    {

#line 1139
        _S22 = false;

#line 1139
    }

#line 1139
    return _S22;
}


#line 1172
float3 double_sided_normal_0(const GpuMaterial_natural_0 thread* material_4, float3 normal_2, bool front_facing_0)
{

#line 1172
    bool _S23;

    if(((material_4->flags_2) & 2U) != 0U)
    {

#line 1174
        _S23 = !front_facing_0;

#line 1174
    }
    else
    {

#line 1174
        _S23 = false;

#line 1174
    }

#line 1174
    float3 _S24;

#line 1174
    if(_S23)
    {

#line 1174
        _S24 = - normal_2;

#line 1174
    }
    else
    {

#line 1174
        _S24 = normal_2;

#line 1174
    }

#line 1174
    return _S24;
}


#line 1073
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_5)
{
    return (material_5->color_normal_pages_0) >> 16U;
}


#line 4786
float3 orthonormal_tangent_0(float3 normal_3)
{
    float _S25 = normal_3.z;

#line 4788
    float sign_z_0;

#line 4788
    if(_S25 >= 0.0f)
    {

#line 4788
        sign_z_0 = 1.0f;

#line 4788
    }
    else
    {

#line 4788
        sign_z_0 = -1.0f;

#line 4788
    }
    float a_0 = -1.0f / (sign_z_0 + _S25);
    float _S26 = normal_3.x;

#line 4790
    float _S27 = sign_z_0 * _S26;

#line 4790
    return float3(1.0f + _S27 * _S26 * a_0, _S27 * normal_3.y * a_0, - sign_z_0 * _S26);
}


#line 4840
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_4)
{
    float _S28 = duvdy_0.y;

#line 4842
    float _S29 = duvdx_0.y;

#line 4842
    float winding_0;
    if((duvdx_0.x * _S28 - duvdy_0.x * _S29) < 0.0f)
    {

#line 4843
        winding_0 = -1.0f;

#line 4843
    }
    else
    {

#line 4843
        winding_0 = 1.0f;

#line 4843
    }
    float3 tangent_2 = (float3(_S28)  * dpdx_0 - float3(_S29)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_4;

#line 4852
    float3 tangent_3 = tangent_2 - normal_4 * float3(dot(normal_4, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 4853
    float3 _S30;

#line 4862
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 4862
        _S30 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 4862
    }
    else
    {

#line 4862
        _S30 = orthonormal_tangent_0(normal_4);

#line 4862
    }

#line 4862
    (&basis_4)->tangent_1 = _S30;

    (&basis_4)->bitangent_0 = cross(normal_4, _S30);
    return basis_4;
}


#line 1853
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_1;
    float3 world_normal_0;
    float4 color_2;
    [[flat]] uint material_6;
    float2 uv_1;
    float4 clip_position_0;
    float4 previous_clip_position_0;
    float3 world_tangent_0;
    [[flat]] uint frame_3;
};


#line 4922
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_5, float2 uv_2, KernelContext_0 thread* kernelContext_6)
{

#line 4934
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_2);
    float2 duvdy_1 = dfdy(uv_2);

    if(layer_0 == 65535U)
    {
        return normal_5;
    }

    thread TangentFrame_0 basis_5;

#line 4944
    uint _S31 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4953
        (&basis_5)->normal_0 = normal_5;
        float3 tangent_4 = input_0->world_tangent_0 - normal_5 * float3(dot(normal_5, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4955
        float3 _S32;

#line 4960
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4960
            _S32 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4960
        }
        else
        {

#line 4960
            _S32 = orthonormal_tangent_0(normal_5);

#line 4960
        }

#line 4960
        (&basis_5)->tangent_1 = _S32;

#line 4966
        float3 _S33 = cross((&basis_5)->normal_0, _S32);

#line 4966
        float _S34;
        if((_S31 & 2U) != 0U)
        {

#line 4967
            _S34 = -1.0f;

#line 4967
        }
        else
        {

#line 4967
            _S34 = 1.0f;

#line 4967
        }

#line 4966
        (&basis_5)->bitangent_0 = _S33 * float3(_S34) ;

#line 4945
    }
    else
    {

#line 4971
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_5);

#line 4945
    }

#line 4975
    float3 _S35 = float3(uv_2, float(layer_0));
    float3 _S36 = ((kernelContext_6->normal_textures_0).sample((kernelContext_6->base_color_sampler_0), ((_S35)).xy, uint(((_S35)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4976
    thread float3 tangent_space_0 = _S36;
    tangent_space_0.xy = _S36.xy * float2(normal_scale_1) ;

#line 4982
    float3 _S37 = normalize(tangent_space_0);

#line 4982
    tangent_space_0 = _S37;
    return normalize(float3(_S37.x)  * (&basis_5)->tangent_1 + float3(_S37.y)  * (&basis_5)->bitangent_0 + float3(_S37.z)  * (&basis_5)->normal_0);
}


#line 2872
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2883
    float3 _S38;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2884
        _S38 = - facet_1;

#line 2884
    }
    else
    {

#line 2884
        _S38 = facet_1;

#line 2884
    }

#line 2884
    return _S38;
}


#line 2277
float specular_aa_kernel_0(float3 normal_6)
{
    float3 dndx_0 = dfdx(normal_6);
    float3 dndy_0 = dfdy(normal_6);


    return min(2.0f * (0.25f * (dot(dndx_0, dndx_0) + dot(dndy_0, dndy_0))), 0.18000000715255737f);
}


#line 4171
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_7)
{
    uint _S39 = max(kernelContext_7->frame_0->cluster_grid_0.x, 1U);
    uint _S40 = max(kernelContext_7->frame_0->cluster_grid_0.y, 1U);
    uint _S41 = max(kernelContext_7->frame_0->cluster_grid_0.z, 1U);
    uint _S42 = max(kernelContext_7->frame_0->cluster_grid_0.w, 1U);

#line 4181
    uint _S43 = uint(pixel_0.x) / _S42;

#line 4181
    uint _S44 = min(_S43, _S39 - 1U);
    uint _S45 = uint(pixel_0.y) / _S42;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4192
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S41 - 1U))) * _S40 + min(_S45, _S40 - 1U)) * _S39 + _S44;
}


#line 2304
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2325
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_8)
{

#line 2325
    texture2d<float, access::sample> _S46 = kernelContext_8->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S46).get_width(0)),(*((&height_1)) = (_S46).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2331
    float2 _S47 = float2(1.0f) ;
    float2 _S48 = extent_1 - _S47;

#line 2332
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S48);
    float2 high_1 = min(low_1 + _S47, _S48);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2350
float2 decode_dfg_pair_0(float4 texel_1)
{
    return float2(texel_1.x * 65280.0f + texel_1.y * 255.0f, texel_1.z * 65280.0f + texel_1.w * 255.0f) / float2(65535.0f) ;
}


#line 2362
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_9)
{
    int _S49 = tap_1->lo_0.x;

#line 2364
    int _S50 = tap_1->lo_0.y;

#line 2364
    int3 _S51 = int3(_S49, _S50, int(0));
    int _S52 = tap_1->hi_0.x;

#line 2365
    int3 _S53 = int3(_S52, _S50, int(0));
    float2 _S54 = float2(tap_1->weight_0.x) ;
    int _S55 = tap_1->hi_0.y;

#line 2367
    int3 _S56 = int3(_S49, _S55, int(0));
    int3 _S57 = int3(_S52, _S55, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_9->specular_dfg_0).read(vec<uint,2>(((_S51)).xy), uint(((_S51)).z)))), decode_dfg_pair_0(((kernelContext_9->specular_dfg_0).read(vec<uint,2>(((_S53)).xy), uint(((_S53)).z)))), _S54), mix(decode_dfg_pair_0(((kernelContext_9->specular_dfg_0).read(vec<uint,2>(((_S56)).xy), uint(((_S56)).z)))), decode_dfg_pair_0(((kernelContext_9->specular_dfg_0).read(vec<uint,2>(((_S57)).xy), uint(((_S57)).z)))), _S54), float2(tap_1->weight_0.y) );
}


#line 4122
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 4138
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 4150
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 4157
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2691
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2691
    float4 _S58 = float4(light_0->tangent_0) ;

    float3 _S59 = _S58.xyz;

#line 2693
    float3 across_0 = _S59 * float3(_S58.w) ;

#line 2693
    float4 _S60 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S59, _S60.xyz) * float3(_S60.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S61 = centre_0 - across_0;

#line 2696
    (*corners_0)[int(0)] = _S61 - down_0;
    float3 _S62 = centre_0 + across_0;

#line 2697
    (*corners_0)[int(1)] = _S62 - down_0;
    (*corners_0)[int(2)] = _S62 + down_0;
    (*corners_0)[int(3)] = _S61 + down_0;
    return;
}


#line 2449
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_7, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_7 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2452
    float3 seed_0;
    if((abs(normal_7.z)) < 0.89999997615814209f)
    {

#line 2453
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2453
    }
    else
    {

#line 2453
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2453
    }

#line 2453
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2454
        tangent_5 = across_1 / float3(span_0) ;

#line 2454
    }
    else
    {

#line 2454
        tangent_5 = normalize(cross(seed_0, normal_7));

#line 2454
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_7, tangent_5), normal_7);
}


#line 2430
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2520
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2520
    float3 _S63 = polygon_0->corner_0[int(0)];

#line 2520
    float3 _S64 = polygon_0->corner_0[int(1)];

#line 2520
    float3 _S65 = polygon_0->corner_0[int(2)];

#line 2520
    float3 _S66 = polygon_0->corner_0[int(3)];

#line 2526
    float3 _S67 = float3(0.0f, 0.0f, 0.0f);


    float _S68 = polygon_0->corner_0[int(0)].z;

#line 2529
    int count_1;

#line 2529
    if(_S68 > 0.0f)
    {

#line 2529
        count_1 = int(1);

#line 2529
    }
    else
    {

#line 2529
        count_1 = int(0);

#line 2529
    }
    float _S69 = _S64.z;

#line 2530
    int _S70;

#line 2530
    if(_S69 > 0.0f)
    {

#line 2530
        _S70 = int(2);

#line 2530
    }
    else
    {

#line 2530
        _S70 = int(0);

#line 2530
    }

#line 2530
    int config_0 = count_1 + _S70;
    float _S71 = _S65.z;

#line 2531
    if(_S71 > 0.0f)
    {

#line 2531
        count_1 = int(4);

#line 2531
    }
    else
    {

#line 2531
        count_1 = int(0);

#line 2531
    }

#line 2531
    int config_1 = config_0 + count_1;
    float _S72 = _S66.z;

#line 2532
    if(_S72 > 0.0f)
    {

#line 2532
        count_1 = int(8);

#line 2532
    }
    else
    {

#line 2532
        count_1 = int(0);

#line 2532
    }

#line 2532
    int config_2 = config_1 + count_1;

#line 2532
    float3 l0_0;

#line 2532
    float3 l1_0;

#line 2532
    float3 l2_0;

#line 2532
    float3 l3_0;

#line 2532
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2535
        float3 _S73 = float3(_S68) ;


        float3 _S74 = float3(- _S69)  * _S63 + _S73 * _S64;
        float3 _S75 = float3(- _S72)  * _S63 + _S73 * _S66;

#line 2539
        count_1 = int(3);

#line 2539
        l0_0 = _S63;

#line 2539
        l1_0 = _S74;

#line 2539
        l2_0 = _S75;

#line 2539
        l3_0 = _S66;

#line 2539
        l4_0 = _S67;

#line 2535
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2541
            float3 _S76 = float3(_S69) ;


            float3 _S77 = float3(- _S68)  * _S64 + _S76 * _S63;
            float3 _S78 = float3(- _S71)  * _S64 + _S76 * _S65;

#line 2545
            count_1 = int(3);

#line 2545
            l0_0 = _S77;

#line 2545
            l1_0 = _S64;

#line 2545
            l2_0 = _S78;

#line 2545
            l3_0 = _S66;

#line 2545
            l4_0 = _S67;

#line 2541
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S79 = float3(- _S71)  * _S64 + float3(_S69)  * _S65;
                float3 _S80 = float3(- _S72)  * _S63 + float3(_S68)  * _S66;

#line 2551
                count_1 = int(4);

#line 2551
                l0_0 = _S63;

#line 2551
                l1_0 = _S64;

#line 2551
                l2_0 = _S79;

#line 2551
                l3_0 = _S80;

#line 2551
                l4_0 = _S67;

#line 2547
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2553
                    float3 _S81 = float3(_S71) ;


                    float3 _S82 = float3(- _S72)  * _S65 + _S81 * _S66;
                    float3 _S83 = float3(- _S69)  * _S65 + _S81 * _S64;

#line 2557
                    count_1 = int(3);

#line 2557
                    l0_0 = _S82;

#line 2557
                    l1_0 = _S83;

#line 2557
                    l2_0 = _S65;

#line 2557
                    l3_0 = _S66;

#line 2557
                    l4_0 = _S67;

#line 2553
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S84 = float3(- _S68)  * _S64 + float3(_S69)  * _S63;
                        float3 _S85 = float3(- _S72)  * _S65 + float3(_S71)  * _S66;

#line 2563
                        count_1 = int(4);

#line 2563
                        l0_0 = _S84;

#line 2563
                        l1_0 = _S64;

#line 2563
                        l2_0 = _S65;

#line 2563
                        l3_0 = _S85;

#line 2563
                        l4_0 = _S67;

#line 2559
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2565
                            float3 _S86 = float3(- _S72) ;


                            float3 _S87 = _S86 * _S63 + float3(_S68)  * _S66;
                            float3 _S88 = _S86 * _S65 + float3(_S71)  * _S66;

#line 2569
                            count_1 = int(5);

#line 2569
                            l0_0 = _S63;

#line 2569
                            l1_0 = _S64;

#line 2569
                            l2_0 = _S65;

#line 2569
                            l3_0 = _S88;

#line 2569
                            l4_0 = _S87;

#line 2565
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2571
                                float3 _S89 = float3(_S72) ;


                                float3 _S90 = float3(- _S68)  * _S66 + _S89 * _S63;
                                float3 _S91 = float3(- _S71)  * _S66 + _S89 * _S65;

#line 2575
                                count_1 = int(3);

#line 2575
                                l0_0 = _S90;

#line 2575
                                l1_0 = _S91;

#line 2575
                                l2_0 = _S66;

#line 2575
                                l3_0 = _S66;

#line 2575
                                l4_0 = _S67;

#line 2571
                            }
                            else
                            {

#line 2578
                                if(config_2 == int(9))
                                {

                                    float3 _S92 = float3(- _S69)  * _S63 + float3(_S68)  * _S64;
                                    float3 _S93 = float3(- _S71)  * _S66 + float3(_S72)  * _S65;

#line 2582
                                    count_1 = int(4);

#line 2582
                                    l0_0 = _S63;

#line 2582
                                    l1_0 = _S92;

#line 2582
                                    l2_0 = _S93;

#line 2582
                                    l3_0 = _S66;

#line 2582
                                    l4_0 = _S67;

#line 2578
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S94 = float3(- _S72)  * _S65 + float3(_S71)  * _S66;
                                        float3 _S95 = float3(- _S71)  * _S64 + float3(_S69)  * _S65;

#line 2589
                                        count_1 = int(5);

#line 2589
                                        l0_0 = _S63;

#line 2589
                                        l1_0 = _S64;

#line 2589
                                        l2_0 = _S95;

#line 2589
                                        l3_0 = _S94;

#line 2589
                                        l4_0 = _S66;

#line 2584
                                    }
                                    else
                                    {

#line 2591
                                        if(config_2 == int(12))
                                        {

                                            float3 _S96 = float3(- _S69)  * _S65 + float3(_S71)  * _S64;
                                            float3 _S97 = float3(- _S68)  * _S66 + float3(_S72)  * _S63;

#line 2595
                                            count_1 = int(4);

#line 2595
                                            l0_0 = _S97;

#line 2595
                                            l1_0 = _S96;

#line 2595
                                            l2_0 = _S65;

#line 2595
                                            l3_0 = _S66;

#line 2595
                                            l4_0 = _S67;

#line 2591
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S98 = float3(- _S71)  * _S64 + float3(_S69)  * _S65;
                                                float3 _S99 = float3(- _S69)  * _S63 + float3(_S68)  * _S64;

#line 2603
                                                count_1 = int(5);

#line 2603
                                                l0_0 = _S63;

#line 2603
                                                l1_0 = _S99;

#line 2603
                                                l2_0 = _S98;

#line 2603
                                                l3_0 = _S65;

#line 2603
                                                l4_0 = _S66;

#line 2597
                                            }
                                            else
                                            {

#line 2605
                                                if(config_2 == int(14))
                                                {

#line 2605
                                                    float3 _S100 = float3(- _S68) ;


                                                    float3 _S101 = _S100 * _S66 + float3(_S72)  * _S63;
                                                    float3 _S102 = _S100 * _S64 + float3(_S69)  * _S63;

#line 2609
                                                    count_1 = int(5);

#line 2609
                                                    l0_0 = _S102;

#line 2609
                                                    l1_0 = _S101;

#line 2605
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2611
                                                        count_1 = int(4);

#line 2611
                                                    }
                                                    else
                                                    {

#line 2611
                                                        count_1 = int(0);

#line 2611
                                                    }

#line 2611
                                                    l0_0 = _S63;

#line 2611
                                                    l1_0 = _S67;

#line 2605
                                                }

#line 2526
                                                float3 _S103 = l1_0;

#line 2526
                                                l1_0 = _S64;

#line 2526
                                                l2_0 = _S65;

#line 2526
                                                l3_0 = _S66;

#line 2526
                                                l4_0 = _S103;

#line 2597
                                            }

#line 2591
                                        }

#line 2584
                                    }

#line 2578
                                }

#line 2571
                            }

#line 2565
                        }

#line 2559
                    }

#line 2553
                }

#line 2547
            }

#line 2541
        }

#line 2535
    }

#line 2619
    if(count_1 <= int(3))
    {

#line 2619
        l3_0 = l0_0;

#line 2619
        l4_0 = l0_0;

#line 2619
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2624
            l4_0 = l0_0;

#line 2624
        }

#line 2619
    }

#line 2629
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2492
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2498
    float weight_1;

#line 2503
    if(cosine_0 > 0.0f)
    {

#line 2503
        weight_1 = fit_0;

#line 2503
    }
    else
    {

#line 2503
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2503
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2649
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2651
    int corner_1 = int(0);
    for(;;)
    {

#line 2652
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2652
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2652
        corner_1 = corner_1 + int(1);

#line 2652
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2657
    thread LtcPolygon_0 _S104 = polygon_1;

#line 2657
    LtcPolygon_0 _S105 = ltc_clip_0(&_S104);
    polygon_1 = _S105;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2661
    int at_2 = int(0);

    for(;;)
    {

#line 2663
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2663
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2663
        at_2 = at_2 + int(1);

#line 2663
    }

#line 2670
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2670
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2671
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2671
    }
    else
    {

#line 2671
        sum_1 = sum_0;

#line 2671
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2675
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2675
    }

#line 2682
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2378
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_10)
{
    int _S106 = tap_2->lo_0.x;

#line 2380
    int _S107 = tap_2->lo_0.y;

#line 2380
    int3 _S108 = int3(_S106, _S107, int(0));
    int _S109 = tap_2->hi_0.x;

#line 2381
    int3 _S110 = int3(_S109, _S107, int(0));
    float4 _S111 = float4(tap_2->weight_0.x) ;
    int _S112 = tap_2->hi_0.y;

#line 2383
    int3 _S113 = int3(_S106, _S112, int(0));
    int3 _S114 = int3(_S109, _S112, int(0));

    return mix(mix(((kernelContext_10->ltc_matrix_0).read(vec<uint,2>(((_S108)).xy), uint(((_S108)).z))), ((kernelContext_10->ltc_matrix_0).read(vec<uint,2>(((_S110)).xy), uint(((_S110)).z))), _S111), mix(((kernelContext_10->ltc_matrix_0).read(vec<uint,2>(((_S113)).xy), uint(((_S113)).z))), ((kernelContext_10->ltc_matrix_0).read(vec<uint,2>(((_S114)).xy), uint(((_S114)).z))), _S111), float4(tap_2->weight_0.y) );
}


#line 2465
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2202
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2209
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2216
    float _S115 = 1.0f - alpha2_0;

#line 2221
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S115 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S115 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3294
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3294
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_12)
{
    return kernelContext_12->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3354
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3326
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_13)
{
    return rect_1.x / kernelContext_13->frame_0->shadow_params_0.x;
}


#line 2923
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3281
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_14)
{

#line 3281
    uint _S116;

    if(uint(pixel_1.x) < (kernelContext_14->frame_0->shadow_filter_0.z))
    {

#line 3283
        _S116 = kernelContext_14->frame_0->shadow_filter_0.x;

#line 3283
    }
    else
    {

#line 3283
        _S116 = kernelContext_14->frame_0->shadow_filter_0.y;

#line 3283
    }

#line 3283
    return _S116;
}


#line 3306
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3306
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_16)
{
    return kernelContext_16->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 349
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3376
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_17)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S117 = spoke_0.x;

#line 3381
    float _S118 = rotation_0.x;

#line 3381
    float _S119 = spoke_0.y;

#line 3381
    float _S120 = rotation_0.y;


    float _S121 = ((kernelContext_17->shadow_atlas_0).sample_compare((kernelContext_17->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S117 * _S118 - _S119 * _S120, _S117 * _S120 + _S119 * _S118) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3384
    return _S121;
}


#line 3464
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_18)
{

#line 3464
    float4 _S122 = atlas_rect_1(tile_2, kernelContext_18);


    if(atlas_rect_is_empty_0(_S122))
    {
        return 1.0f;
    }

#line 3469
    float2 _S123 = atlas_step_1(_S122, kernelContext_18);

#line 3469
    int y_1 = int(-1);

#line 3469
    float visibility_0 = 0.0f;

#line 3474
    for(;;)
    {

#line 3474
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3474
            break;
        }

#line 3474
        int x_0 = int(-1);

        for(;;)
        {

#line 3476
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3476
                break;
            }

#line 3476
            float _S124 = tile_tap_0(_S122, _S123, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_18);

            float visibility_1 = visibility_0 + _S124;

#line 3476
            x_0 = x_0 + int(1);

#line 3476
            visibility_0 = visibility_1;

#line 3476
        }

#line 3474
        y_1 = y_1 + int(1);

#line 3474
    }

#line 3482
    return visibility_0 / 9.0f;
}


#line 3239
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3406
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_19)
{
    float2 _S125 = shadow_rotation_0(pixel_3);

#line 3408
    float4 _S126 = atlas_rect_1(tile_3, kernelContext_19);

    if(atlas_rect_is_empty_0(_S126))
    {
        return 1.0f;
    }

#line 3412
    float2 _S127 = atlas_step_1(_S126, kernelContext_19);

#line 3412
    uint spot_0 = 0U;

#line 3412
    float probe_0 = 0.0f;

#line 3417
    for(;;)
    {

#line 3417
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3417
            break;
        }

#line 3417
        float _S128 = tile_tap_0(_S126, _S127, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S125, reference_2, kernelContext_19);

        float probe_1 = probe_0 + _S128;

#line 3417
        spot_0 = spot_0 + 1U;

#line 3417
        probe_0 = probe_1;

#line 3417
    }

#line 3426
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3432
    uint index_2 = 0U;

#line 3432
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3436
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3436
            break;
        }

#line 3436
        float _S129 = tile_tap_0(_S126, _S127, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S125, reference_2, kernelContext_19);

        float visibility_3 = visibility_2 + _S129;

#line 3436
        index_2 = index_2 + 1U;

#line 3436
        visibility_2 = visibility_3;

#line 3436
    }

#line 3441
    return visibility_2 / 32.0f;
}


#line 3517
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_20)
{
    float2 texel_2 = kernelContext_20->frame_0->shadow_params_0.xy;

#line 3519
    float4 _S130 = atlas_rect_0(cascade_0, kernelContext_20);

#line 3519
    float2 _S131 = atlas_step_0(_S130, kernelContext_20);


    float2 _S132 = float2(0.5f, 0.5f) * _S131;


    float2 _S133 = float2(1.0f, 1.0f);

#line 3525
    float2 _S134 = _S133 / texel_2;

#line 3525
    uint index_3 = 0U;

#line 3525
    float sum_2 = 0.0f;

#line 3525
    float found_0 = 0.0f;



    for(;;)
    {

#line 3529
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3529
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S135 = spoke_1.x;

#line 3532
        float _S136 = rotation_1.x;

#line 3532
        float _S137 = spoke_1.y;

#line 3532
        float _S138 = rotation_1.y;

#line 3540
        int3 _S139 = int3(int2(min(atlas_uv_0(_S130, clamp(tile_uv_4 + float2(_S135 * _S136 - _S137 * _S138, _S135 * _S138 + _S137 * _S136) * _S131, _S132, float2(1.0f)  - _S132)) * _S134, _S134 - _S133)), int(0));

#line 3540
        float depth_1 = ((kernelContext_20->shadow_atlas_0).read(vec<uint,2>(((_S139)).xy), uint(((_S139)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3544
            sum_2 = sum_2 + depth_1;

#line 3544
            found_0 = found_1;

#line 3541
        }

#line 3529
        index_3 = index_3 + 1U;

#line 3529
    }

#line 3548
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3559
    float _S140 = 2.0f * kernelContext_20->frame_0->cascade_far_0[cascade_0];

#line 3559
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S140 + 40.0f);

#line 3559
    float _S141 = tile_texels_0(_S130, kernelContext_20);

    return clamp(separation_0 * 0.01999999955296516f / (_S140 / _S141), 2.0f, 8.0f);
}


#line 3613
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_21)
{

#line 3614
    float4 _S142 = atlas_rect_0(cascade_1, kernelContext_21);

#line 3648
    if(atlas_rect_is_empty_0(_S142))
    {


        return 1.0f;
    }
    float _S143 = 2.0f * kernelContext_21->frame_0->cascade_far_0[cascade_1];

#line 3654
    float _S144 = tile_texels_0(_S142, kernelContext_21);

#line 3654
    float texel_world_0 = _S143 / _S144;

#line 3661
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_21->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_21->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_21->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3665
    bool _S145;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3666
        _S145 = true;

#line 3666
    }
    else
    {

#line 3666
        _S145 = (ndc_0.z) <= 0.0f;

#line 3666
    }

#line 3666
    if(_S145)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3676
    uint _S146 = shadow_filter_mode_0(pixel_4, kernelContext_21);

#line 3693
    if(_S146 == 2U)
    {

#line 3693
        float _S147 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_21);

        return _S147;
    }
    if(_S146 == 1U)
    {

#line 3697
        float _S148 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_21);



        return _S148;
    }

    float _S149 = ndc_0.z;

#line 3704
    float _S150 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S149, shadow_rotation_0(pixel_4), kernelContext_21);

#line 3704
    float _S151 = tile_pcf_0(cascade_1, tile_uv_5, _S149, pixel_4, _S150, kernelContext_21);
    return _S151;
}


#line 3784
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_22)
{
    uint cascade_2;

#line 3786
    bool covered_0;

#line 3795
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3807
    float eye_distance_0 = length(world_position_5 - kernelContext_22->frame_0->camera_position_0.xyz);

#line 3807
    uint index_4 = 0U;

#line 3815
    for(;;)
    {

#line 3815
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3815
            covered_0 = false;

#line 3815
            cascade_2 = 1U;

#line 3815
            break;
        }
        if(eye_distance_0 < kernelContext_22->frame_0->cascade_far_0[index_4])
        {

#line 3817
            covered_0 = true;

#line 3817
            cascade_2 = index_4;



            break;
        }

#line 3815
        index_4 = index_4 + 1U;

#line 3815
    }

#line 3824
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 3824
    }

#line 3824
    float _S152 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_22);

#line 3831
    uint _S153 = cascade_2 + 1U;

#line 3831
    if(_S153 >= 2U)
    {



        return _S152;
    }

#line 3844
    float band_0 = kernelContext_22->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_22->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S152;
    }

#line 3852
    float _S154 = cascade_visibility_0(_S153, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_22);

#line 3863
    return mix(_S152, _S154, blend_0);
}


#line 5051
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_23)
{

#line 5051
    texture2d<float, access::sample> _S155 = kernelContext_23->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S155).get_width(0)),(*((&height_2)) = (_S155).get_height(0));

    int3 _S156 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 5057
    return ((kernelContext_23->contact_shadow_0).read(vec<uint,2>(((_S156)).xy), uint(((_S156)).z)).x);
}


#line 3756
float3 cascade_tint_0(uint cascade_3, float blend_1)
{
    if(cascade_3 >= 2U)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }
    uint _S157 = cascade_3 + 1U;

#line 3762
    if(_S157 >= 2U)
    {


        return CASCADE_TINTS_0[cascade_3];
    }
    return mix(CASCADE_TINTS_0[cascade_3], CASCADE_TINTS_0[_S157], float3(blend_1) );
}


#line 4074
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S158 = axis_2.x;

#line 4077
    float _S159 = axis_2.y;

#line 4077
    bool _S160;

#line 4077
    if(_S158 >= _S159)
    {

#line 4077
        _S160 = _S158 >= (axis_2.z);

#line 4077
    }
    else
    {

#line 4077
        _S160 = false;

#line 4077
    }

#line 4077
    uint _S161;

#line 4077
    if(_S160)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 4079
            _S161 = 0U;

#line 4079
        }
        else
        {

#line 4079
            _S161 = 1U;

#line 4079
        }

#line 4079
        return _S161;
    }
    if(_S159 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 4083
            _S161 = 2U;

#line 4083
        }
        else
        {

#line 4083
            _S161 = 3U;

#line 4083
        }

#line 4083
        return _S161;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 4085
        _S161 = 4U;

#line 4085
    }
    else
    {

#line 4085
        _S161 = 5U;

#line 4085
    }

#line 4085
    return _S161;
}


#line 336
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 3970
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_24)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 3973
    float4 _S162 = atlas_rect_0(atlas_0, kernelContext_24);

    if(atlas_rect_is_empty_0(_S162))
    {


        return 1.0f;
    }

#line 3979
    float _S163 = tile_texels_0(_S162, kernelContext_24);

    float texel_world_1 = map_world_0 / _S163;

#line 3991
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_24->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 3998
    float _S164 = clip_1.w;

#line 3998
    if(_S164 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S164) ;

#line 4002
    bool _S165;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 4003
        _S165 = true;

#line 4003
    }
    else
    {

#line 4003
        _S165 = (ndc_1.z) <= 0.0f;

#line 4003
    }

#line 4003
    if(_S165)
    {

#line 4003
        _S165 = true;

#line 4003
    }
    else
    {

#line 4003
        _S165 = (ndc_1.z) > 1.0f;

#line 4003
    }

#line 4003
    if(_S165)
    {

#line 4010
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 4015
    uint _S166 = shadow_filter_mode_0(pixel_6, kernelContext_24);

#line 4024
    if(_S166 == 2U)
    {

#line 4024
        float _S167 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_24);

        return _S167;
    }

#line 4026
    float _S168 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_24);

    return _S168;
}


#line 4093
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_25)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 4101
    float _S169 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_25);

#line 4107
    return _S169;
}


#line 4035
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_26)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 4042
    float4 _S170 = float4(light_2->direction_0) ;

#line 4049
    float cos_outer_1 = _S170.w;

#line 4049
    float _S171 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S170.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_26);

#line 4056
    return _S171;
}


#line 2406
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 5038
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 5040
    float3 _S172;
    if((length(decoded_0)) < 0.5f)
    {

#line 5041
        _S172 = shading_normal_1;

#line 5041
    }
    else
    {

#line 5041
        _S172 = normalize(decoded_0);

#line 5041
    }

#line 5041
    return _S172;
}


#line 4676
float3 sky_irradiance_0(float3 normal_8, KernelContext_0 thread* kernelContext_27)
{
    float4 basis_6 = float4(normal_8, 1.0f);
    return max(float3(dot(kernelContext_27->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_27->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_27->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4580
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4580
    float reach_0 = 0.0f;

#line 4580
    uint axis_3 = 0U;


    for(;;)
    {

#line 4583
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4583
            break;
        }

#line 4583
        uint _S173 = axis_3;

#line 4583
        bool _S174;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4585
            _S174 = true;

#line 4585
        }
        else
        {

#line 4585
            _S174 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4585
        }

#line 4585
        if(_S174)
        {

#line 4586
            axis_3 = axis_3 + 1U;

#line 4583
            continue;
        }

#line 4583
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S173] - 1.0f));

#line 4583
        axis_3 = axis_3 + 1U;

#line 4583
    }

#line 4590
    return reach_0;
}


#line 4610
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4610
    uint level_0 = 0U;

    for(;;)
    {

#line 4612
        uint _S175 = level_0 + 1U;

#line 4612
        if(_S175 < levels_0)
        {
        }
        else
        {

#line 4612
            break;
        }
        float _S176 = float(level_0);

#line 4614
        float at_3 = reach_1 * exp2(- _S176);
        if(at_3 < 1.0f)
        {

#line 4616
            return float2(_S176, saturate((1.0f - at_3) / 0.25f));
        }

#line 4612
        level_0 = _S175;

#line 4612
    }

#line 4618
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4367
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4369
    uint _S177;
    if(at_4 >= count_2)
    {

#line 4370
        _S177 = at_4 - count_2;

#line 4370
    }
    else
    {

#line 4370
        _S177 = at_4;

#line 4370
    }

#line 4370
    return _S177;
}


#line 4393
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_28)
{
    uint3 counts_0 = kernelContext_28->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_28->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S178 = counts_0.x;
    uint _S179 = counts_0.y;



    return min(kernelContext_28->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S179 + probe_wrap_0(cell_2.y, offset_1.y, _S179)) * _S178 + probe_wrap_0(cell_2.x, offset_1.x, _S178), max(kernelContext_28->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4234
float sign_not_zero_0(float value_0)
{

#line 4234
    float _S180;

    if(value_0 >= 0.0f)
    {

#line 4236
        _S180 = 1.0f;

#line 4236
    }
    else
    {

#line 4236
        _S180 = -1.0f;

#line 4236
    }

#line 4236
    return _S180;
}


#line 4253
float2 oct_encode_0(float3 direction_1)
{
    float _S181 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S181) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4256
    float2 p_1;
    if(_S181 < 0.0f)
    {
        float _S182 = p_0.y;

#line 4259
        float _S183 = p_0.x;

#line 4259
        p_1 = float2((1.0f - abs(_S182)) * sign_not_zero_0(_S183), (1.0f - abs(_S183)) * sign_not_zero_0(_S182));

#line 4257
    }
    else
    {

#line 4257
        p_1 = p_0;

#line 4257
    }

#line 4262
    return p_1;
}


#line 4282
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_29)
{

#line 4282
    texture2d_array<float, access::sample> _S184 = kernelContext_29->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S184).get_width(0)),(*((&height_3)) = (_S184).get_height(0)),(*((&layers_0)) = (_S184).get_array_size());

#line 4287
    float2 _S185 = float2(0.5f) ;

#line 4287
    float2 _S186 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S185 + _S185) * float2(16.0f)  + _S186 - _S185;
    float2 _S187 = float2(float(width_3), float(height_3)) - _S186;

#line 4291
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S187);
    float2 high_2 = min(low_2 + _S186, _S187);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S188 = int(low_2.x);

#line 4296
    int _S189 = int(low_2.y);

#line 4296
    int4 _S190 = int4(_S188, _S189, layer_1, int(0));
    int _S191 = int(high_2.x);

#line 4297
    int4 _S192 = int4(_S191, _S189, layer_1, int(0));
    int _S193 = int(high_2.y);

#line 4298
    int4 _S194 = int4(_S188, _S193, layer_1, int(0));
    int4 _S195 = int4(_S191, _S193, layer_1, int(0));
    float2 _S196 = float2(weight_2.x) ;

#line 4300
    return mix(mix(((kernelContext_29->probe_visibility_0).read(vec<uint,2>(((_S190)).xy), uint(((_S190)).z), uint(((_S190)).w))).xy, ((kernelContext_29->probe_visibility_0).read(vec<uint,2>(((_S192)).xy), uint(((_S192)).z), uint(((_S192)).w))).xy, _S196), mix(((kernelContext_29->probe_visibility_0).read(vec<uint,2>(((_S194)).xy), uint(((_S194)).z), uint(((_S194)).w))).xy, ((kernelContext_29->probe_visibility_0).read(vec<uint,2>(((_S195)).xy), uint(((_S195)).z), uint(((_S195)).w))).xy, _S196), float2(weight_2.y) );
}


#line 4328
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_9, KernelContext_0 thread* kernelContext_30)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_9 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4331
    float2 _S197 = probe_moments_0(index_6, - to_probe_0, kernelContext_30);

#line 4337
    float _S198 = _S197.x;

#line 4337
    float _S199 = max(_S197.y - _S198 * _S198, 0.0f);
    float behind_0 = to_surface_0 - _S198;
    float bound_0 = _S199 / (_S199 + behind_0 * behind_0);

#line 4339
    float _S200;
    if(to_surface_0 <= _S198)
    {

#line 4340
        _S200 = 1.0f;

#line 4340
    }
    else
    {

#line 4340
        _S200 = bound_0 * bound_0 * bound_0;

#line 4340
    }

#line 4340
    return _S200;
}


#line 4350
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_10, KernelContext_0 thread* kernelContext_31)
{

#line 4350
    float _S201 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_10, kernelContext_31);

    return max(_S201, 0.00009999999747379f);
}


#line 1188
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4412
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4439
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_11, KernelContext_0 thread* kernelContext_32)
{

#line 4440
    uint _S202 = probe_row_0(level_2, cell_3, kernelContext_32);


    GpuProbe_natural_0 stored_0 = kernelContext_32->probes_0[_S202];

#line 4443
    float _S203 = probe_weight_0(_S202, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_11, kernelContext_32);



    thread WeightedProbe_0 corner_2;

#line 4447
    float4 _S204 = float4(_S203) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S204;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S204;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S204;
    (&corner_2)->weight_3 = _S203;
    return corner_2;
}


#line 4423
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S205 = float4(t_1) ;

#line 4426
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S205);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S205);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S205);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4511
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_12, KernelContext_0 thread* kernelContext_33)
{

#line 4511
    float3 _S206 = float3(1.0f) ;

#line 4516
    float3 _S207 = float3(0.0f, 0.0f, 0.0f);

#line 4516
    float3 last_1 = max(float3(kernelContext_33->frame_0->probe_counts_0.xyz) - _S206, _S207);



    float3 origin_2 = kernelContext_33->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_33->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S207, last_1);
    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S208 = uint3(base_2);



    uint3 _S209 = uint3(min(base_2 + _S206, last_1));

#line 4536
    float _S210 = inv_0.x;

#line 4536
    float _S211;

#line 4536
    if(_S210 != 0.0f)
    {

#line 4536
        _S211 = 1.0f / _S210;

#line 4536
    }
    else
    {

#line 4536
        _S211 = 0.0f;

#line 4536
    }
    float _S212 = inv_0.y;

#line 4537
    float _S213;

#line 4537
    if(_S212 != 0.0f)
    {

#line 4537
        _S213 = 1.0f / _S212;

#line 4537
    }
    else
    {

#line 4537
        _S213 = 0.0f;

#line 4537
    }
    float _S214 = inv_0.z;

#line 4538
    float _S215;

#line 4538
    if(_S214 != 0.0f)
    {

#line 4538
        _S215 = 1.0f / _S214;

#line 4538
    }
    else
    {

#line 4538
        _S215 = 0.0f;

#line 4538
    }

#line 4536
    float3 spacing_1 = float3(_S211, _S213, _S215);

#line 4545
    uint _S216 = _S208.x;

#line 4545
    uint _S217 = _S208.y;

#line 4545
    uint _S218 = _S208.z;

#line 4545
    WeightedProbe_0 _S219 = probe_corner_0(level_3, uint3(_S216, _S217, _S218), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);
    uint _S220 = _S209.x;

#line 4546
    WeightedProbe_0 _S221 = probe_corner_0(level_3, uint3(_S220, _S217, _S218), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);

#line 4546
    float _S222 = f_0.x;

#line 4546
    thread WeightedProbe_0 _S223 = _S219;

#line 4546
    thread WeightedProbe_0 _S224 = _S221;

#line 4546
    WeightedProbe_0 _S225 = lerp_probe_0(&_S223, &_S224, _S222);
    uint _S226 = _S209.y;

#line 4547
    WeightedProbe_0 _S227 = probe_corner_0(level_3, uint3(_S216, _S226, _S218), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);

#line 4547
    WeightedProbe_0 _S228 = probe_corner_0(level_3, uint3(_S220, _S226, _S218), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);

#line 4547
    thread WeightedProbe_0 _S229 = _S227;

#line 4547
    thread WeightedProbe_0 _S230 = _S228;

#line 4547
    WeightedProbe_0 _S231 = lerp_probe_0(&_S229, &_S230, _S222);

    uint _S232 = _S209.z;

#line 4549
    WeightedProbe_0 _S233 = probe_corner_0(level_3, uint3(_S216, _S217, _S232), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);

#line 4549
    WeightedProbe_0 _S234 = probe_corner_0(level_3, uint3(_S220, _S217, _S232), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);

#line 4549
    thread WeightedProbe_0 _S235 = _S233;

#line 4549
    thread WeightedProbe_0 _S236 = _S234;

#line 4549
    WeightedProbe_0 _S237 = lerp_probe_0(&_S235, &_S236, _S222);

#line 4549
    WeightedProbe_0 _S238 = probe_corner_0(level_3, uint3(_S216, _S226, _S232), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);

#line 4549
    WeightedProbe_0 _S239 = probe_corner_0(level_3, uint3(_S220, _S226, _S232), origin_2, spacing_1, world_position_13, normal_12, kernelContext_33);

#line 4549
    thread WeightedProbe_0 _S240 = _S238;

#line 4549
    thread WeightedProbe_0 _S241 = _S239;

#line 4549
    WeightedProbe_0 _S242 = lerp_probe_0(&_S240, &_S241, _S222);



    float _S243 = f_0.y;

#line 4553
    thread WeightedProbe_0 _S244 = _S225;

#line 4553
    thread WeightedProbe_0 _S245 = _S231;

#line 4553
    WeightedProbe_0 _S246 = lerp_probe_0(&_S244, &_S245, _S243);

#line 4553
    thread WeightedProbe_0 _S247 = _S237;

#line 4553
    thread WeightedProbe_0 _S248 = _S242;

#line 4553
    WeightedProbe_0 _S249 = lerp_probe_0(&_S247, &_S248, _S243);

    float _S250 = f_0.z;

#line 4555
    thread WeightedProbe_0 _S251 = _S246;

#line 4555
    thread WeightedProbe_0 _S252 = _S249;

#line 4555
    WeightedProbe_0 _S253 = lerp_probe_0(&_S251, &_S252, _S250);

    float4 basis_7 = float4(normal_12, 1.0f);
    return max(float3(dot(_S253.sh_0.sh_r_0, basis_7), dot(_S253.sh_0.sh_g_0, basis_7), dot(_S253.sh_0.sh_b_0, basis_7)) / float3(_S253.weight_3) , _S207);
}


#line 4645
float3 probe_irradiance_0(float3 world_position_14, float3 normal_13, KernelContext_0 thread* kernelContext_34)
{

#line 4653
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_34->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_34->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_34->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_34->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 4655
    float3 _S254 = probe_level_irradiance_0(level_4, world_position_14, normal_13, kernelContext_34);


    if(share_0 >= 1.0f)
    {

#line 4659
        return _S254;
    }

#line 4659
    float3 _S255 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_13, kernelContext_34);

    return _S255 * float3((1.0f - share_0))  + _S254 * float3(share_0) ;
}


#line 5107
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 5107
    float3 _S256 = float3(visibility_4) ;

#line 5113
    return min(float3(1.0f) , max(_S256, ((_S256 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S256 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S256));
}


#line 1083
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_7)
{
    return float3(material_7->emissive_r_0, material_7->emissive_g_0, material_7->emissive_b_0);
}


#line 2757
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S257 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2765
    float kernel_0 = 0.0001984127011383f;

#line 2765
    int term_0 = int(6);

    for(;;)
    {

#line 2767
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2767
            break;
        }
        float _S258 = kernel_0 * _S257 + FOG_KERNEL_0[term_0];

#line 2767
        int term_1 = term_0 - int(1);

#line 2767
        kernel_0 = _S258;

#line 2767
        term_0 = term_1;

#line 2767
    }

#line 2774
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2784
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S259 = - d_0;

#line 2788
        float series_0 = 0.00833333376795053f;

#line 2788
        int term_2 = int(3);

        for(;;)
        {

#line 2790
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2790
                break;
            }
            float _S260 = series_0 * _S259 + FOG_RATIO_KERNEL_0[term_2];

#line 2790
            int term_3 = term_2 - int(1);

#line 2790
            series_0 = _S260;

#line 2790
            term_2 = term_3;

#line 2790
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2818
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2829
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2837
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4702
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4702
struct pixelInput_0
{
    float3 world_position_15 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    [[flat]] uint material_8 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
    float3 world_tangent_1 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_4 [[user(TEXCOORD_5)]];
};


#line 5149
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S261 [[stage_in]], bool front_facing_1 [[front_facing]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 5149
    thread KernelContext_0 kernelContext_35;

#line 5149
    (&kernelContext_35)->draw_0 = draw_3;

#line 5149
    (&kernelContext_35)->visible_instances_0 = visible_instances_3;

#line 5149
    (&kernelContext_35)->instances_0 = instances_3;

#line 5149
    (&kernelContext_35)->meshes_0 = meshes_3;

#line 5149
    (&kernelContext_35)->frame_0 = frame_5;

#line 5149
    (&kernelContext_35)->vertices_0 = vertices_3;

#line 5149
    (&kernelContext_35)->ambient_occlusion_0 = ambient_occlusion_3;

#line 5149
    (&kernelContext_35)->materials_0 = materials_3;

#line 5149
    (&kernelContext_35)->base_color_textures_0 = base_color_textures_3;

#line 5149
    (&kernelContext_35)->base_color_sampler_0 = base_color_sampler_3;

#line 5149
    (&kernelContext_35)->normal_textures_0 = normal_textures_3;

#line 5149
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_3;

#line 5149
    (&kernelContext_35)->specular_dfg_0 = specular_dfg_3;

#line 5149
    (&kernelContext_35)->lights_0 = lights_3;

#line 5149
    (&kernelContext_35)->ltc_matrix_0 = ltc_matrix_3;

#line 5149
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_3;

#line 5149
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_3;

#line 5149
    (&kernelContext_35)->contact_shadow_0 = contact_shadow_3;

#line 5149
    (&kernelContext_35)->probes_0 = probes_3;

#line 5149
    (&kernelContext_35)->probe_visibility_0 = probe_visibility_3;

#line 5161
    float3 vertex_normal_0 = normalize(_S261.world_normal_1);

#line 5166
    float2 motion_1 = motion_vector_0(_S261.clip_position_1, _S261.previous_clip_position_1);

#line 5182
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 5184
        float4 _S262 = occlusion_at_0(position_5.xy, &kernelContext_35);



        (&bent_0)->lit_0 = float4(_S262.yzw, 1.0f);


        (&bent_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&bent_0)->motion_0 = motion_1;
        return bent_0;
    }

    if((frame_5->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 5238
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5238
        float4 _S263 = occlusion_at_0(position_5.xy, &kernelContext_35);


        float value_1 = _S263.x;

#line 5240
        thread FragmentOutput_0 occlusion_1;

#line 5249
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S261.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 5266
    thread GpuMaterial_natural_0 _S264 = (&kernelContext_35)->materials_0[_S261.material_8];

#line 5266
    float2 uv_4;

#line 5291
    if(((&_S264)->tiling_0) == 1U)
    {

#line 5291
        uv_4 = physical_tile_uv_0(_S261.world_position_15, vertex_normal_0, (&_S264)->tile_metres_0);

#line 5291
    }
    else
    {

#line 5291
        uv_4 = _S261.uv_3;

#line 5291
    }

#line 5291
    float4 _S265 = base_color_texel_0(&_S264, uv_4, &kernelContext_35);

#line 5313
    float4 albedo_1 = _S261.color_3 * float4((&_S264)->base_color_0)  * _S265;

#line 5327
    float _S266 = albedo_1.w;

#line 5327
    bool _S267 = alpha_masked_0(&_S264, _S266);

#line 5327
    if(_S267)
    {
        discard_fragment();

#line 5327
    }

#line 5327
    float3 _S268 = double_sided_normal_0(&_S264, vertex_normal_0, front_facing_1);

#line 5327
    uint _S269 = normal_layer_0(&_S264);

#line 5327
    thread VertexOutput_0 _S270;

#line 5327
    (&_S270)->position_3 = position_5;

#line 5327
    (&_S270)->world_position_1 = _S261.world_position_15;

#line 5327
    (&_S270)->world_normal_0 = _S261.world_normal_1;

#line 5327
    (&_S270)->color_2 = _S261.color_3;

#line 5327
    (&_S270)->material_6 = _S261.material_8;

#line 5327
    (&_S270)->uv_1 = _S261.uv_3;

#line 5327
    (&_S270)->clip_position_0 = _S261.clip_position_1;

#line 5327
    (&_S270)->previous_clip_position_0 = _S261.previous_clip_position_1;

#line 5327
    (&_S270)->world_tangent_0 = _S261.world_tangent_1;

#line 5327
    (&_S270)->frame_3 = _S261.frame_4;

#line 5327
    float3 _S271 = shading_normal_of_0(_S269, (&_S264)->normal_scale_0, &_S270, _S268, uv_4, &kernelContext_35);

#line 5346
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5348
        float3 _S272 = float3(0.5f) ;

#line 5360
        (&normals_0)->lit_0 = float4(_S271 * _S272 + _S272, 1.0f);

#line 5366
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_35)->frame_0->camera_position_0.xyz - _S261.world_position_15);



    float3 _S273 = geometric_normal_of_0(_S261.world_position_15, _S268);

#line 5381
    float metallic_1 = saturate((&_S264)->metallic_0);
    float roughness_2 = clamp((&_S264)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_1 = roughness_2 * roughness_2;

#line 5416
    float _S274 = saturate(alpha_1 * alpha_1 + specular_aa_kernel_0(_S271));

#line 5422
    float3 _S275 = albedo_1.xyz;

#line 5422
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S275, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S275 * float3((1.0f - metallic_1)) ;

#line 5429
    float _S276 = max(dot(_S271, to_eye_1), 0.00009999999747379f);

#line 5439
    float2 _S277 = position_5.xy;

#line 5439
    uint _S278 = froxel_of_0(_S277, (((float4(_S261.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_35)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_35)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_35);

#line 5439
    uint base_3 = _S278 * 17U;

#line 5444
    uint _S279 = min((&kernelContext_35)->cluster_lights_0[base_3], 16U);

#line 5444
    TableTap_0 _S280 = table_tap_0(_S276, roughness_2, &kernelContext_35);

#line 5444
    thread TableTap_0 _S281 = _S280;

#line 5444
    float2 _S282 = dfg_at_0(&_S281, &kernelContext_35);

#line 5453
    float _S283 = _S282.x;

#line 5453
    float _S284 = _S282.y;

#line 5453
    float3 _S285 = f0_2 * float3(_S283)  + float3(_S284) ;

#line 5459
    float3 _S286 = float3(0.0f, 0.0f, 0.0f);

#line 5459
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5459
    uint slot_0 = 0U;

#line 5459
    float3 direct_0 = _S286;

#line 5459
    float3 gloss_0 = _S286;

#line 5469
    for(;;)
    {

#line 5469
        if(slot_0 < _S279)
        {
        }
        else
        {

#line 5469
            break;
        }

#line 5469
        thread GpuLight_natural_0 _S287 = (&kernelContext_35)->lights_0[(&kernelContext_35)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 5469
        uint _S288 = (&_S287)->kind_0;

#line 5478
        bool _S289 = ((&_S287)->kind_0) == 0U;

#line 5478
        float3 to_light_7;

#line 5478
        float reach_2;

#line 5478
        if(_S289)
        {

#line 5478
            to_light_7 = normalize((float4((&_S287)->direction_0) ).xyz);

#line 5478
            reach_2 = 1.0f;

#line 5478
        }
        else
        {


            if(_S288 == 3U)
            {

#line 5483
                float4 _S290 = float4((&_S287)->position_0) ;

#line 5491
                float3 offset_2 = _S290.xyz - _S261.world_position_15;
                float distance_3 = length(offset_2);

                float _S291 = range_window_0(distance_3, _S290.w);

#line 5494
                to_light_7 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5494
                reach_2 = _S291;

#line 5483
            }
            else
            {

#line 5483
                float4 _S292 = float4((&_S287)->position_0) ;

#line 5498
                float3 offset_3 = _S292.xyz - _S261.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_8 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S292.w);
                if(_S288 == 2U)
                {

#line 5502
                    float4 _S293 = float4((&_S287)->direction_0) ;

#line 5502
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S293.xyz, _S293.w, (&_S287)->cos_inner_0);

#line 5502
                }
                else
                {

#line 5502
                    reach_2 = reach_3;

#line 5502
                }

#line 5502
                to_light_7 = to_light_8;

#line 5483
            }

#line 5478
        }

#line 5511
        float n_dot_l_5 = dot(_S271, to_light_7);

#line 5511
        float3 specular_0;

#line 5511
        float diffuse_0;


        if(_S288 == 3U)
        {

#line 5524
            thread array<float3, int(4)> corners_2;

#line 5524
            rect_corners_0(&_S287, _S261.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S271, to_eye_1, _S276);

#line 5526
            thread array<float3, int(4)> _S294 = corners_2;

#line 5526
            float _S295 = ltc_irradiance_0(to_local_0, &_S294);

#line 5526
            thread TableTap_0 _S296 = _S280;

#line 5526
            float4 _S297 = ltc_at_0(&_S296, &kernelContext_35);

            matrix<float,int(3),int(3)>  _S298 = (((to_local_0) * (ltc_transform_0(_S297))));

#line 5528
            thread array<float3, int(4)> _S299 = corners_2;

#line 5528
            float _S300 = ltc_irradiance_0(_S298, &_S299);
            float3 _S301 = float3(_S300)  * _S285;

#line 5529
            diffuse_0 = _S295;

#line 5529
            specular_0 = _S301;

#line 5514
        }
        else
        {

#line 5534
            float _S302 = max(n_dot_l_5, 0.0f);

#line 5541
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5549
            float3 specular_1 = ggx_lobe_0(_S274, f0_2, _S302, _S276, max(dot(_S271, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S302) ;

#line 5549
            diffuse_0 = _S302;

#line 5549
            specular_0 = specular_1;

#line 5514
        }

#line 5514
        float3 specular_2;

#line 5557
        if((((&_S287)->flags_3) & 1U) != 0U)
        {

#line 5557
            specular_2 = _S286;

#line 5557
        }
        else
        {

#line 5557
            specular_2 = specular_0;

#line 5557
        }

#line 5557
        float reach_4;

#line 5575
        if(_S289)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 5578
            float _S303 = sun_visibility_0(_S261.world_position_15, to_light_7, n_dot_l_5, _S273, _S277, &sun_cascade_0, &sun_fade_0, &kernelContext_35);

#line 5578
            float _S304 = contact_at_0(_S277, &kernelContext_35);

#line 5587
            float _S305 = _S303 * _S304;

#line 5587
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 5587
            reach_4 = _S305;

#line 5575
        }
        else
        {

#line 5592
            if(_S288 == 1U)
            {

#line 5592
                uint _S306 = (&_S287)->shadow_tile_0;

#line 5604
                if(((&_S287)->shadow_tile_0) <= 8U)
                {

#line 5604
                    float _S307 = point_visibility_0(&_S287, _S306, _S261.world_position_15, to_light_7, n_dot_l_5, _S273, _S277, &kernelContext_35);

#line 5604
                    reach_4 = reach_2 * _S307;

#line 5604
                }
                else
                {

#line 5604
                    reach_4 = reach_2;

#line 5604
                }

#line 5592
            }
            else
            {

#line 5592
                uint _S308 = (&_S287)->shadow_tile_0;

#line 5610
                if(((&_S287)->shadow_tile_0) < 14U)
                {

#line 5610
                    float _S309 = spot_visibility_0(&_S287, _S308, _S261.world_position_15, to_light_7, n_dot_l_5, _S273, _S277, &kernelContext_35);

#line 5610
                    reach_4 = reach_2 * _S309;

#line 5610
                }
                else
                {

#line 5610
                    reach_4 = reach_2;

#line 5610
                }

#line 5592
            }

#line 5575
        }

#line 5618
        float3 _S310 = (float4((&_S287)->color_0) ).xyz;

#line 5618
        float3 direct_1 = direct_0 + _S310 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S310 * (specular_2 * float3(reach_4) );

#line 5469
        slot_0 = slot_0 + 1U;

#line 5469
        direct_0 = direct_1;

#line 5469
        gloss_0 = gloss_1;

#line 5469
    }

#line 5633
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S283 + _S284);

#line 5633
    float4 _S311 = occlusion_at_0(_S277, &kernelContext_35);

#line 5652
    float occluded_0 = _S311.x;

#line 5661
    float3 bent_normal_0 = bent_normal_at_0(_S311, _S271);

#line 5684
    float3 _S312 = frame_5->ambient_0.xyz;

#line 5684
    float3 _S313 = sky_irradiance_0(bent_normal_0, &kernelContext_35);

#line 5684
    float3 _S314 = _S312 + _S313;

#line 5684
    float3 _S315 = probe_irradiance_0(_S261.world_position_15, bent_normal_0, &kernelContext_35);

#line 5720
    float3 lit_1 = diffuse_albedo_0 * ((_S314 + _S315) * multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) + direct_0) + gloss_2;

#line 5720
    float3 _S316 = emissive_of_0(&_S264);

#line 5756
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_35)->frame_0->fog_params_0.x, (&kernelContext_35)->frame_0->fog_params_0.y, (&kernelContext_35)->frame_0->camera_position_0.y - (&kernelContext_35)->frame_0->fog_params_0.z, _S261.world_position_15.y - (&kernelContext_35)->frame_0->fog_params_0.z, length((&kernelContext_35)->frame_0->camera_position_0.xyz - _S261.world_position_15)));
    float3 lit_2 = (lit_1 + _S316) * float3(fog_survives_0)  + (&kernelContext_35)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;



    (&output_2)->lit_0 = float4(lit_2, _S266);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 5776
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, _S266);

#line 5785
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 5776
    }

#line 5787
    return output_2;
}


#line 5787
struct pixelInput_1
{
    float3 world_position_16 [[user(POSITION)]];
    float3 world_normal_2 [[user(NORMAL)]];
    float4 color_4 [[user(COLOR)]];
    [[flat]] uint material_9 [[user(TEXCOORD)]];
    float2 uv_5 [[user(TEXCOORD_1)]];
    float4 clip_position_2 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_2 [[user(TEXCOORD_3)]];
    float3 world_tangent_2 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_6 [[user(TEXCOORD_5)]];
};


#line 5820
[[fragment]] void depthMaskedFragmentMain(pixelInput_1 _S317 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 5820
    thread KernelContext_0 kernelContext_36;

#line 5820
    (&kernelContext_36)->draw_0 = draw_4;

#line 5820
    (&kernelContext_36)->visible_instances_0 = visible_instances_4;

#line 5820
    (&kernelContext_36)->instances_0 = instances_4;

#line 5820
    (&kernelContext_36)->meshes_0 = meshes_4;

#line 5820
    (&kernelContext_36)->frame_0 = frame_7;

#line 5820
    (&kernelContext_36)->vertices_0 = vertices_4;

#line 5820
    (&kernelContext_36)->ambient_occlusion_0 = ambient_occlusion_4;

#line 5820
    (&kernelContext_36)->materials_0 = materials_4;

#line 5820
    (&kernelContext_36)->base_color_textures_0 = base_color_textures_4;

#line 5820
    (&kernelContext_36)->base_color_sampler_0 = base_color_sampler_4;

#line 5820
    (&kernelContext_36)->normal_textures_0 = normal_textures_4;

#line 5820
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_4;

#line 5820
    (&kernelContext_36)->specular_dfg_0 = specular_dfg_4;

#line 5820
    (&kernelContext_36)->lights_0 = lights_4;

#line 5820
    (&kernelContext_36)->ltc_matrix_0 = ltc_matrix_4;

#line 5820
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_4;

#line 5820
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_4;

#line 5820
    (&kernelContext_36)->contact_shadow_0 = contact_shadow_4;

#line 5820
    (&kernelContext_36)->probes_0 = probes_4;

#line 5820
    (&kernelContext_36)->probe_visibility_0 = probe_visibility_4;

#line 5820
    thread GpuMaterial_natural_0 _S318 = materials_4[_S317.material_9];

#line 5820
    float2 uv_6;

#line 5829
    if(((&_S318)->tiling_0) == 1U)
    {

#line 5829
        uv_6 = physical_tile_uv_0(_S317.world_position_16, normalize(_S317.world_normal_2), (&_S318)->tile_metres_0);

#line 5829
    }
    else
    {

#line 5829
        uv_6 = _S317.uv_5;

#line 5829
    }

#line 5829
    float4 _S319 = base_color_texel_0(&_S318, uv_6, &kernelContext_36);

#line 5829
    bool _S320 = alpha_masked_0(&_S318, _S317.color_4.w * (float4((&_S318)->base_color_0) ).w * _S319.w);

#line 5838
    if(_S320)
    {
        discard_fragment();

#line 5838
    }



    return;
}


#line 5872
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_14 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 5872
struct pixelInput_2
{
    float3 world_position_17 [[user(POSITION)]];
    float3 world_normal_3 [[user(NORMAL)]];
    float4 color_5 [[user(COLOR)]];
    [[flat]] uint material_10 [[user(TEXCOORD)]];
    float2 uv_7 [[user(TEXCOORD_1)]];
    float4 clip_position_3 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_3 [[user(TEXCOORD_3)]];
    float3 world_tangent_3 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_8 [[user(TEXCOORD_5)]];
};


#line 5915
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_2 _S321 [[stage_in]], bool front_facing_2 [[front_facing]], float4 position_7 [[position]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 5915
    thread KernelContext_0 kernelContext_37;

#line 5915
    (&kernelContext_37)->draw_0 = draw_5;

#line 5915
    (&kernelContext_37)->visible_instances_0 = visible_instances_5;

#line 5915
    (&kernelContext_37)->instances_0 = instances_5;

#line 5915
    (&kernelContext_37)->meshes_0 = meshes_5;

#line 5915
    (&kernelContext_37)->frame_0 = frame_9;

#line 5915
    (&kernelContext_37)->vertices_0 = vertices_5;

#line 5915
    (&kernelContext_37)->ambient_occlusion_0 = ambient_occlusion_5;

#line 5915
    (&kernelContext_37)->materials_0 = materials_5;

#line 5915
    (&kernelContext_37)->base_color_textures_0 = base_color_textures_5;

#line 5915
    (&kernelContext_37)->base_color_sampler_0 = base_color_sampler_5;

#line 5915
    (&kernelContext_37)->normal_textures_0 = normal_textures_5;

#line 5915
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_5;

#line 5915
    (&kernelContext_37)->specular_dfg_0 = specular_dfg_5;

#line 5915
    (&kernelContext_37)->lights_0 = lights_5;

#line 5915
    (&kernelContext_37)->ltc_matrix_0 = ltc_matrix_5;

#line 5915
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_5;

#line 5915
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_5;

#line 5915
    (&kernelContext_37)->contact_shadow_0 = contact_shadow_5;

#line 5915
    (&kernelContext_37)->probes_0 = probes_5;

#line 5915
    (&kernelContext_37)->probe_visibility_0 = probe_visibility_5;

#line 5920
    float3 vertex_normal_1 = normalize(_S321.world_normal_3);

#line 5920
    thread GpuMaterial_natural_0 _S322 = materials_5[_S321.material_10];

#line 5920
    float2 uv_8;

#line 5927
    if(((&_S322)->tiling_0) == 1U)
    {

#line 5927
        uv_8 = physical_tile_uv_0(_S321.world_position_17, vertex_normal_1, (&_S322)->tile_metres_0);

#line 5927
    }
    else
    {

#line 5927
        uv_8 = _S321.uv_7;

#line 5927
    }

#line 5927
    float4 _S323 = base_color_texel_0(&_S322, uv_8, &kernelContext_37);

#line 5932
    float4 albedo_3 = _S321.color_5 * float4((&_S322)->base_color_0)  * _S323;

#line 5932
    bool _S324 = alpha_masked_0(&_S322, albedo_3.w);

#line 5938
    if(_S324)
    {
        discard_fragment();

#line 5938
    }

#line 5943
    thread RsmOutput_0 written_0;



    (&written_0)->albedo_2 = float4(albedo_3.xyz * float3((1.0f - saturate((&_S322)->metallic_0))) , 1.0f);

#line 5947
    float3 _S325 = double_sided_normal_0(&_S322, vertex_normal_1, front_facing_2);

#line 5947
    float3 _S326 = float3(0.5f) ;

#line 5953
    (&written_0)->normal_14 = float4(_S325 * _S326 + _S326, 1.0f);

    (&written_0)->world_0 = float4(_S321.world_position_17, 1.0f);
    return written_0;
}


#line 5956
struct vertexMain_Result_0
{
    float4 position_8 [[position]];
    float3 world_position_18 [[user(POSITION)]];
    float3 world_normal_4 [[user(NORMAL)]];
    float4 color_6 [[user(COLOR)]];
    uint material_11 [[user(TEXCOORD)]];
    float2 uv_9 [[user(TEXCOORD_1)]];
    float4 clip_position_4 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_4 [[user(TEXCOORD_3)]];
    float3 world_tangent_4 [[user(TEXCOORD_4)]];
    uint frame_10 [[user(TEXCOORD_5)]];
};


#line 5956
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_6 [[buffer(3)]], uint device* visible_instances_6 [[buffer(5)]], GpuInstance_natural_0 device* instances_6 [[buffer(2)]], GpuMesh_0 device* meshes_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_11 [[buffer(0)]], uint device* vertices_6 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_6 [[texture(2)]], GpuMaterial_natural_0 device* materials_6 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_6 [[texture(0)]], sampler base_color_sampler_6 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_6 [[texture(4)]], uint device* cluster_lights_6 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_6 [[texture(3)]], GpuLight_natural_0 device* lights_6 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_6 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_6 [[texture(6)]], GpuProbe_natural_0 device* probes_6 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_6 [[texture(7)]])
{

#line 5956
    thread KernelContext_0 kernelContext_38;

#line 5956
    (&kernelContext_38)->draw_0 = draw_6;

#line 5956
    (&kernelContext_38)->visible_instances_0 = visible_instances_6;

#line 5956
    (&kernelContext_38)->instances_0 = instances_6;

#line 5956
    (&kernelContext_38)->meshes_0 = meshes_6;

#line 5956
    (&kernelContext_38)->frame_0 = frame_11;

#line 5956
    (&kernelContext_38)->vertices_0 = vertices_6;

#line 5956
    (&kernelContext_38)->ambient_occlusion_0 = ambient_occlusion_6;

#line 5956
    (&kernelContext_38)->materials_0 = materials_6;

#line 5956
    (&kernelContext_38)->base_color_textures_0 = base_color_textures_6;

#line 5956
    (&kernelContext_38)->base_color_sampler_0 = base_color_sampler_6;

#line 5956
    (&kernelContext_38)->normal_textures_0 = normal_textures_6;

#line 5956
    (&kernelContext_38)->cluster_lights_0 = cluster_lights_6;

#line 5956
    (&kernelContext_38)->specular_dfg_0 = specular_dfg_6;

#line 5956
    (&kernelContext_38)->lights_0 = lights_6;

#line 5956
    (&kernelContext_38)->ltc_matrix_0 = ltc_matrix_6;

#line 5956
    (&kernelContext_38)->shadow_atlas_0 = shadow_atlas_6;

#line 5956
    (&kernelContext_38)->shadow_sampler_0 = shadow_sampler_6;

#line 5956
    (&kernelContext_38)->contact_shadow_0 = contact_shadow_6;

#line 5956
    (&kernelContext_38)->probes_0 = probes_6;

#line 5956
    (&kernelContext_38)->probe_visibility_0 = probe_visibility_6;

#line 5956
    GpuInstance_natural_0 device* _S327 = instances_6+visible_instances_6[draw_6->base_0 + instance_id_1];

#line 1988
    GpuMesh_0 mesh_3 = meshes_6[draw_6->mesh_0];

#line 1996
    bool _S328 = ((_S327->flags_0) & 2U) != 0U;

#line 1996
    uint base_vertex_3;
    if(_S328)
    {

#line 1997
        base_vertex_3 = _S327->base_vertex_0;

#line 1997
    }
    else
    {

#line 1997
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1997
    }

#line 1997
    MeshVertex_0 _S329 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_38);

#line 1997
    uint previous_base_0;

#line 2010
    if(_S328)
    {

#line 2010
        previous_base_0 = _S327->previous_base_vertex_0;

#line 2010
    }
    else
    {

#line 2010
        previous_base_0 = base_vertex_3;

#line 2010
    }

#line 2010
    float3 _S330 = load_position_0(index_8 + previous_base_0, &kernelContext_38);

#line 2010
    matrix<float,int(4),int(4)>  _S331 = matrix<float,int(4),int(4)> (_S327->transform_0.data_0[int(0)][int(0)], _S327->transform_0.data_0[int(1)][int(0)], _S327->transform_0.data_0[int(2)][int(0)], _S327->transform_0.data_0[int(3)][int(0)], _S327->transform_0.data_0[int(0)][int(1)], _S327->transform_0.data_0[int(1)][int(1)], _S327->transform_0.data_0[int(2)][int(1)], _S327->transform_0.data_0[int(3)][int(1)], _S327->transform_0.data_0[int(0)][int(2)], _S327->transform_0.data_0[int(1)][int(2)], _S327->transform_0.data_0[int(2)][int(2)], _S327->transform_0.data_0[int(3)][int(2)], _S327->transform_0.data_0[int(0)][int(3)], _S327->transform_0.data_0[int(1)][int(3)], _S327->transform_0.data_0[int(2)][int(3)], _S327->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S329.position_1, 1.0f)) * (_S331)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 2024
    matrix<float,int(3),int(3)>  _S332 = matrix<float,int(3),int(3)> (_S331[int(0)].xyz, _S331[int(1)].xyz, _S331[int(2)].xyz);

#line 2024
    (&output_3)->world_normal_0 = (((_S329.basis_1.normal_0) * (normal_basis_0(_S332))));

#line 2030
    (&output_3)->world_tangent_0 = (((_S329.basis_1.tangent_1) * (_S332)));

#line 2030
    thread TangentFrame_0 _S333 = _S329.basis_1;

#line 2030
    uint _S334 = frame_word_0(mesh_3.flags_1, &_S333);
    (&output_3)->frame_3 = _S334;

#line 2031
    float4 _S335;

#line 2038
    if(((&kernelContext_38)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 2038
        _S335 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 2038
    }
    else
    {

#line 2038
        _S335 = _S329.color_1;

#line 2038
    }

#line 2037
    (&output_3)->color_2 = _S335;

#line 2044
    (&output_3)->material_6 = _S327->material_0;
    (&output_3)->uv_1 = _S329.uv0_0;

#line 2051
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S330, 1.0f)) * (matrix<float,int(4),int(4)> (_S327->previous_transform_0.data_0[int(0)][int(0)], _S327->previous_transform_0.data_0[int(1)][int(0)], _S327->previous_transform_0.data_0[int(2)][int(0)], _S327->previous_transform_0.data_0[int(3)][int(0)], _S327->previous_transform_0.data_0[int(0)][int(1)], _S327->previous_transform_0.data_0[int(1)][int(1)], _S327->previous_transform_0.data_0[int(2)][int(1)], _S327->previous_transform_0.data_0[int(3)][int(1)], _S327->previous_transform_0.data_0[int(0)][int(2)], _S327->previous_transform_0.data_0[int(1)][int(2)], _S327->previous_transform_0.data_0[int(2)][int(2)], _S327->previous_transform_0.data_0[int(3)][int(2)], _S327->previous_transform_0.data_0[int(0)][int(3)], _S327->previous_transform_0.data_0[int(1)][int(3)], _S327->previous_transform_0.data_0[int(2)][int(3)], _S327->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_38)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S336 = output_3;

#line 2055
    thread vertexMain_Result_0 _S337;

#line 2055
    (&_S337)->position_8 = _S336.position_3;

#line 2055
    (&_S337)->world_position_18 = _S336.world_position_1;

#line 2055
    (&_S337)->world_normal_4 = _S336.world_normal_0;

#line 2055
    (&_S337)->color_6 = _S336.color_2;

#line 2055
    (&_S337)->material_11 = _S336.material_6;

#line 2055
    (&_S337)->uv_9 = _S336.uv_1;

#line 2055
    (&_S337)->clip_position_4 = _S336.clip_position_0;

#line 2055
    (&_S337)->previous_clip_position_4 = _S336.previous_clip_position_0;

#line 2055
    (&_S337)->world_tangent_4 = _S336.world_tangent_0;

#line 2055
    (&_S337)->frame_10 = _S336.frame_3;

#line 2055
    return _S337;
}

