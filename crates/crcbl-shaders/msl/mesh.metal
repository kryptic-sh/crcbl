#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2635 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2630
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 3632
constant array<float3, int(2)> CASCADE_TINTS_0 = { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) };

#line 3115
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2902
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2962
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2977
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 3005
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1236
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1880
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1880
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


#line 1886
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1886
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


#line 1279
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


#line 1290
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1293
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1744
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1867
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1867
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1869
        word_4 = 1U;

#line 1869
    }
    else
    {

#line 1869
        word_4 = 0U;

#line 1869
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1873
        word_4 = word_4 | 2U;

#line 1873
    }

#line 1872
    return word_4;
}


#line 1872
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1988
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 1988
    thread KernelContext_0 kernelContext_2;

#line 1988
    (&kernelContext_2)->draw_0 = draw_1;

#line 1988
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1988
    (&kernelContext_2)->instances_0 = instances_1;

#line 1988
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1988
    (&kernelContext_2)->frame_0 = frame_1;

#line 1988
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1988
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1988
    (&kernelContext_2)->materials_0 = materials_1;

#line 1988
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1988
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1988
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1988
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1988
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1988
    (&kernelContext_2)->lights_0 = lights_1;

#line 1988
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1988
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1988
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1988
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 1988
    (&kernelContext_2)->probes_0 = probes_1;

#line 1988
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 1988
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1991
    uint base_vertex_2;

#line 1997
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1997
        base_vertex_2 = _S7->base_vertex_0;

#line 1997
    }
    else
    {

#line 1997
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1997
    }

#line 1997
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1997
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1997
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 2000
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 2021
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 2021
    thread KernelContext_0 kernelContext_3;

#line 2021
    (&kernelContext_3)->draw_0 = draw_2;

#line 2021
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 2021
    (&kernelContext_3)->instances_0 = instances_2;

#line 2021
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 2021
    (&kernelContext_3)->frame_0 = frame_2;

#line 2021
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 2021
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 2021
    (&kernelContext_3)->materials_0 = materials_2;

#line 2021
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 2021
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 2021
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 2021
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 2021
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 2021
    (&kernelContext_3)->lights_0 = lights_2;

#line 2021
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 2021
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 2021
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 2021
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 2021
    (&kernelContext_3)->probes_0 = probes_2;

#line 2021
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 2021
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 5037
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 5039
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4913
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4913
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4919
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4647
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4651
    float _S16 = axis_0.y;

#line 4651
    bool _S17;

#line 4651
    if(_S15 >= _S16)
    {

#line 4651
        _S17 = _S15 >= (axis_0.z);

#line 4651
    }
    else
    {

#line 4651
        _S17 = false;

#line 4651
    }

#line 4651
    float2 planar_0;

#line 4651
    if(_S17)
    {

#line 4651
        planar_0 = world_position_0.zy;

#line 4651
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4655
            planar_0 = world_position_0.xz;

#line 4655
        }
        else
        {

#line 4655
            planar_0 = world_position_0.xy;

#line 4655
        }

#line 4651
    }

#line 4663
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1043
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) & 65535U;
}


#line 1111
bool alpha_masked_0(const GpuMaterial_natural_0 thread* material_2, float alpha_0)
{

#line 1111
    bool _S18;

    if(((material_2->flags_2) & 1U) != 0U)
    {

#line 1113
        _S18 = alpha_0 < (material_2->alpha_cutoff_0);

#line 1113
    }
    else
    {

#line 1113
        _S18 = false;

#line 1113
    }

#line 1113
    return _S18;
}


#line 1058
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) >> 16U;
}


#line 4684
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S19 = normal_2.z;

#line 4686
    float sign_z_0;

#line 4686
    if(_S19 >= 0.0f)
    {

#line 4686
        sign_z_0 = 1.0f;

#line 4686
    }
    else
    {

#line 4686
        sign_z_0 = -1.0f;

#line 4686
    }
    float a_0 = -1.0f / (sign_z_0 + _S19);
    float _S20 = normal_2.x;

#line 4688
    float _S21 = sign_z_0 * _S20;

#line 4688
    return float3(1.0f + _S21 * _S20 * a_0, _S21 * normal_2.y * a_0, - sign_z_0 * _S20);
}


#line 4738
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S22 = duvdy_0.y;

#line 4740
    float _S23 = duvdx_0.y;

#line 4740
    float winding_0;
    if((duvdx_0.x * _S22 - duvdy_0.x * _S23) < 0.0f)
    {

#line 4741
        winding_0 = -1.0f;

#line 4741
    }
    else
    {

#line 4741
        winding_0 = 1.0f;

#line 4741
    }
    float3 tangent_2 = (float3(_S22)  * dpdx_0 - float3(_S23)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 4750
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 4751
    float3 _S24;

#line 4760
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 4760
        _S24 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 4760
    }
    else
    {

#line 4760
        _S24 = orthonormal_tangent_0(normal_3);

#line 4760
    }

#line 4760
    (&basis_4)->tangent_1 = _S24;

    (&basis_4)->bitangent_0 = cross(normal_3, _S24);
    return basis_4;
}


#line 1751
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_1;
    float3 world_normal_0;
    float4 color_2;
    [[flat]] uint material_4;
    float2 uv_0;
    float4 clip_position_0;
    float4 previous_clip_position_0;
    float3 world_tangent_0;
    [[flat]] uint frame_3;
};


#line 4820
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 4832
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 4842
    uint _S25 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4851
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4853
        float3 _S26;

#line 4858
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4858
            _S26 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4858
        }
        else
        {

#line 4858
            _S26 = orthonormal_tangent_0(normal_4);

#line 4858
        }

#line 4858
        (&basis_5)->tangent_1 = _S26;

#line 4864
        float3 _S27 = cross((&basis_5)->normal_0, _S26);

#line 4864
        float _S28;
        if((_S25 & 2U) != 0U)
        {

#line 4865
            _S28 = -1.0f;

#line 4865
        }
        else
        {

#line 4865
            _S28 = 1.0f;

#line 4865
        }

#line 4864
        (&basis_5)->bitangent_0 = _S27 * float3(_S28) ;

#line 4843
    }
    else
    {

#line 4869
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 4843
    }

#line 4873
    float3 _S29 = float3(uv_1, float(layer_0));
    float3 _S30 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S29)).xy, uint(((_S29)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4874
    thread float3 tangent_space_0 = _S30;
    tangent_space_0.xy = _S30.xy * float2(normal_scale_1) ;

#line 4880
    float3 _S31 = normalize(tangent_space_0);

#line 4880
    tangent_space_0 = _S31;
    return normalize(float3(_S31.x)  * (&basis_5)->tangent_1 + float3(_S31.y)  * (&basis_5)->bitangent_0 + float3(_S31.z)  * (&basis_5)->normal_0);
}


#line 2770
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2781
    float3 _S32;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2782
        _S32 = - facet_1;

#line 2782
    }
    else
    {

#line 2782
        _S32 = facet_1;

#line 2782
    }

#line 2782
    return _S32;
}


#line 2175
float specular_aa_kernel_0(float3 normal_5)
{
    float3 dndx_0 = dfdx(normal_5);
    float3 dndy_0 = dfdy(normal_5);


    return min(2.0f * (0.25f * (dot(dndx_0, dndx_0) + dot(dndy_0, dndy_0))), 0.18000000715255737f);
}


#line 4069
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S33 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S34 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S36 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 4079
    uint _S37 = uint(pixel_0.x) / _S36;

#line 4079
    uint _S38 = min(_S37, _S33 - 1U);
    uint _S39 = uint(pixel_0.y) / _S36;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4090
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S35 - 1U))) * _S34 + min(_S39, _S34 - 1U)) * _S33 + _S38;
}


#line 2202
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2223
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 2223
    texture2d<float, access::sample> _S40 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S40).get_width(0)),(*((&height_1)) = (_S40).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2229
    float2 _S41 = float2(1.0f) ;
    float2 _S42 = extent_1 - _S41;

#line 2230
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S42);
    float2 high_1 = min(low_1 + _S41, _S42);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2248
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 2260
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S43 = tap_1->lo_0.x;

#line 2262
    int _S44 = tap_1->lo_0.y;

#line 2262
    int3 _S45 = int3(_S43, _S44, int(0));
    int _S46 = tap_1->hi_0.x;

#line 2263
    int3 _S47 = int3(_S46, _S44, int(0));
    float2 _S48 = float2(tap_1->weight_0.x) ;
    int _S49 = tap_1->hi_0.y;

#line 2265
    int3 _S50 = int3(_S43, _S49, int(0));
    int3 _S51 = int3(_S46, _S49, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S47)).xy), uint(((_S47)).z)))), _S48), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S51)).xy), uint(((_S51)).z)))), _S48), float2(tap_1->weight_0.y) );
}


#line 4020
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 4036
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 4048
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 4055
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2589
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2589
    float4 _S52 = float4(light_0->tangent_0) ;

    float3 _S53 = _S52.xyz;

#line 2591
    float3 across_0 = _S53 * float3(_S52.w) ;

#line 2591
    float4 _S54 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S53, _S54.xyz) * float3(_S54.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S55 = centre_0 - across_0;

#line 2594
    (*corners_0)[int(0)] = _S55 - down_0;
    float3 _S56 = centre_0 + across_0;

#line 2595
    (*corners_0)[int(1)] = _S56 - down_0;
    (*corners_0)[int(2)] = _S56 + down_0;
    (*corners_0)[int(3)] = _S55 + down_0;
    return;
}


#line 2347
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_6, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_6 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2350
    float3 seed_0;
    if((abs(normal_6.z)) < 0.89999997615814209f)
    {

#line 2351
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2351
    }
    else
    {

#line 2351
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2351
    }

#line 2351
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2352
        tangent_5 = across_1 / float3(span_0) ;

#line 2352
    }
    else
    {

#line 2352
        tangent_5 = normalize(cross(seed_0, normal_6));

#line 2352
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_6, tangent_5), normal_6);
}


#line 2328
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2418
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2418
    float3 _S57 = polygon_0->corner_0[int(0)];

#line 2418
    float3 _S58 = polygon_0->corner_0[int(1)];

#line 2418
    float3 _S59 = polygon_0->corner_0[int(2)];

#line 2418
    float3 _S60 = polygon_0->corner_0[int(3)];

#line 2424
    float3 _S61 = float3(0.0f, 0.0f, 0.0f);


    float _S62 = polygon_0->corner_0[int(0)].z;

#line 2427
    int count_1;

#line 2427
    if(_S62 > 0.0f)
    {

#line 2427
        count_1 = int(1);

#line 2427
    }
    else
    {

#line 2427
        count_1 = int(0);

#line 2427
    }
    float _S63 = _S58.z;

#line 2428
    int _S64;

#line 2428
    if(_S63 > 0.0f)
    {

#line 2428
        _S64 = int(2);

#line 2428
    }
    else
    {

#line 2428
        _S64 = int(0);

#line 2428
    }

#line 2428
    int config_0 = count_1 + _S64;
    float _S65 = _S59.z;

#line 2429
    if(_S65 > 0.0f)
    {

#line 2429
        count_1 = int(4);

#line 2429
    }
    else
    {

#line 2429
        count_1 = int(0);

#line 2429
    }

#line 2429
    int config_1 = config_0 + count_1;
    float _S66 = _S60.z;

#line 2430
    if(_S66 > 0.0f)
    {

#line 2430
        count_1 = int(8);

#line 2430
    }
    else
    {

#line 2430
        count_1 = int(0);

#line 2430
    }

#line 2430
    int config_2 = config_1 + count_1;

#line 2430
    float3 l0_0;

#line 2430
    float3 l1_0;

#line 2430
    float3 l2_0;

#line 2430
    float3 l3_0;

#line 2430
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2433
        float3 _S67 = float3(_S62) ;


        float3 _S68 = float3(- _S63)  * _S57 + _S67 * _S58;
        float3 _S69 = float3(- _S66)  * _S57 + _S67 * _S60;

#line 2437
        count_1 = int(3);

#line 2437
        l0_0 = _S57;

#line 2437
        l1_0 = _S68;

#line 2437
        l2_0 = _S69;

#line 2437
        l3_0 = _S60;

#line 2437
        l4_0 = _S61;

#line 2433
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2439
            float3 _S70 = float3(_S63) ;


            float3 _S71 = float3(- _S62)  * _S58 + _S70 * _S57;
            float3 _S72 = float3(- _S65)  * _S58 + _S70 * _S59;

#line 2443
            count_1 = int(3);

#line 2443
            l0_0 = _S71;

#line 2443
            l1_0 = _S58;

#line 2443
            l2_0 = _S72;

#line 2443
            l3_0 = _S60;

#line 2443
            l4_0 = _S61;

#line 2439
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S73 = float3(- _S65)  * _S58 + float3(_S63)  * _S59;
                float3 _S74 = float3(- _S66)  * _S57 + float3(_S62)  * _S60;

#line 2449
                count_1 = int(4);

#line 2449
                l0_0 = _S57;

#line 2449
                l1_0 = _S58;

#line 2449
                l2_0 = _S73;

#line 2449
                l3_0 = _S74;

#line 2449
                l4_0 = _S61;

#line 2445
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2451
                    float3 _S75 = float3(_S65) ;


                    float3 _S76 = float3(- _S66)  * _S59 + _S75 * _S60;
                    float3 _S77 = float3(- _S63)  * _S59 + _S75 * _S58;

#line 2455
                    count_1 = int(3);

#line 2455
                    l0_0 = _S76;

#line 2455
                    l1_0 = _S77;

#line 2455
                    l2_0 = _S59;

#line 2455
                    l3_0 = _S60;

#line 2455
                    l4_0 = _S61;

#line 2451
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S78 = float3(- _S62)  * _S58 + float3(_S63)  * _S57;
                        float3 _S79 = float3(- _S66)  * _S59 + float3(_S65)  * _S60;

#line 2461
                        count_1 = int(4);

#line 2461
                        l0_0 = _S78;

#line 2461
                        l1_0 = _S58;

#line 2461
                        l2_0 = _S59;

#line 2461
                        l3_0 = _S79;

#line 2461
                        l4_0 = _S61;

#line 2457
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2463
                            float3 _S80 = float3(- _S66) ;


                            float3 _S81 = _S80 * _S57 + float3(_S62)  * _S60;
                            float3 _S82 = _S80 * _S59 + float3(_S65)  * _S60;

#line 2467
                            count_1 = int(5);

#line 2467
                            l0_0 = _S57;

#line 2467
                            l1_0 = _S58;

#line 2467
                            l2_0 = _S59;

#line 2467
                            l3_0 = _S82;

#line 2467
                            l4_0 = _S81;

#line 2463
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2469
                                float3 _S83 = float3(_S66) ;


                                float3 _S84 = float3(- _S62)  * _S60 + _S83 * _S57;
                                float3 _S85 = float3(- _S65)  * _S60 + _S83 * _S59;

#line 2473
                                count_1 = int(3);

#line 2473
                                l0_0 = _S84;

#line 2473
                                l1_0 = _S85;

#line 2473
                                l2_0 = _S60;

#line 2473
                                l3_0 = _S60;

#line 2473
                                l4_0 = _S61;

#line 2469
                            }
                            else
                            {

#line 2476
                                if(config_2 == int(9))
                                {

                                    float3 _S86 = float3(- _S63)  * _S57 + float3(_S62)  * _S58;
                                    float3 _S87 = float3(- _S65)  * _S60 + float3(_S66)  * _S59;

#line 2480
                                    count_1 = int(4);

#line 2480
                                    l0_0 = _S57;

#line 2480
                                    l1_0 = _S86;

#line 2480
                                    l2_0 = _S87;

#line 2480
                                    l3_0 = _S60;

#line 2480
                                    l4_0 = _S61;

#line 2476
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S88 = float3(- _S66)  * _S59 + float3(_S65)  * _S60;
                                        float3 _S89 = float3(- _S65)  * _S58 + float3(_S63)  * _S59;

#line 2487
                                        count_1 = int(5);

#line 2487
                                        l0_0 = _S57;

#line 2487
                                        l1_0 = _S58;

#line 2487
                                        l2_0 = _S89;

#line 2487
                                        l3_0 = _S88;

#line 2487
                                        l4_0 = _S60;

#line 2482
                                    }
                                    else
                                    {

#line 2489
                                        if(config_2 == int(12))
                                        {

                                            float3 _S90 = float3(- _S63)  * _S59 + float3(_S65)  * _S58;
                                            float3 _S91 = float3(- _S62)  * _S60 + float3(_S66)  * _S57;

#line 2493
                                            count_1 = int(4);

#line 2493
                                            l0_0 = _S91;

#line 2493
                                            l1_0 = _S90;

#line 2493
                                            l2_0 = _S59;

#line 2493
                                            l3_0 = _S60;

#line 2493
                                            l4_0 = _S61;

#line 2489
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S92 = float3(- _S65)  * _S58 + float3(_S63)  * _S59;
                                                float3 _S93 = float3(- _S63)  * _S57 + float3(_S62)  * _S58;

#line 2501
                                                count_1 = int(5);

#line 2501
                                                l0_0 = _S57;

#line 2501
                                                l1_0 = _S93;

#line 2501
                                                l2_0 = _S92;

#line 2501
                                                l3_0 = _S59;

#line 2501
                                                l4_0 = _S60;

#line 2495
                                            }
                                            else
                                            {

#line 2503
                                                if(config_2 == int(14))
                                                {

#line 2503
                                                    float3 _S94 = float3(- _S62) ;


                                                    float3 _S95 = _S94 * _S60 + float3(_S66)  * _S57;
                                                    float3 _S96 = _S94 * _S58 + float3(_S63)  * _S57;

#line 2507
                                                    count_1 = int(5);

#line 2507
                                                    l0_0 = _S96;

#line 2507
                                                    l1_0 = _S95;

#line 2503
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2509
                                                        count_1 = int(4);

#line 2509
                                                    }
                                                    else
                                                    {

#line 2509
                                                        count_1 = int(0);

#line 2509
                                                    }

#line 2509
                                                    l0_0 = _S57;

#line 2509
                                                    l1_0 = _S61;

#line 2503
                                                }

#line 2424
                                                float3 _S97 = l1_0;

#line 2424
                                                l1_0 = _S58;

#line 2424
                                                l2_0 = _S59;

#line 2424
                                                l3_0 = _S60;

#line 2424
                                                l4_0 = _S97;

#line 2495
                                            }

#line 2489
                                        }

#line 2482
                                    }

#line 2476
                                }

#line 2469
                            }

#line 2463
                        }

#line 2457
                    }

#line 2451
                }

#line 2445
            }

#line 2439
        }

#line 2433
    }

#line 2517
    if(count_1 <= int(3))
    {

#line 2517
        l3_0 = l0_0;

#line 2517
        l4_0 = l0_0;

#line 2517
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2522
            l4_0 = l0_0;

#line 2522
        }

#line 2517
    }

#line 2527
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2390
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2396
    float weight_1;

#line 2401
    if(cosine_0 > 0.0f)
    {

#line 2401
        weight_1 = fit_0;

#line 2401
    }
    else
    {

#line 2401
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2401
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2547
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2549
    int corner_1 = int(0);
    for(;;)
    {

#line 2550
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2550
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2550
        corner_1 = corner_1 + int(1);

#line 2550
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2555
    thread LtcPolygon_0 _S98 = polygon_1;

#line 2555
    LtcPolygon_0 _S99 = ltc_clip_0(&_S98);
    polygon_1 = _S99;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2559
    int at_2 = int(0);

    for(;;)
    {

#line 2561
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2561
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2561
        at_2 = at_2 + int(1);

#line 2561
    }

#line 2568
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2568
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2569
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2569
    }
    else
    {

#line 2569
        sum_1 = sum_0;

#line 2569
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2573
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2573
    }

#line 2580
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2276
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S100 = tap_2->lo_0.x;

#line 2278
    int _S101 = tap_2->lo_0.y;

#line 2278
    int3 _S102 = int3(_S100, _S101, int(0));
    int _S103 = tap_2->hi_0.x;

#line 2279
    int3 _S104 = int3(_S103, _S101, int(0));
    float4 _S105 = float4(tap_2->weight_0.x) ;
    int _S106 = tap_2->hi_0.y;

#line 2281
    int3 _S107 = int3(_S100, _S106, int(0));
    int3 _S108 = int3(_S103, _S106, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S102)).xy), uint(((_S102)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S104)).xy), uint(((_S104)).z))), _S105), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S107)).xy), uint(((_S107)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S108)).xy), uint(((_S108)).z))), _S105), float4(tap_2->weight_0.y) );
}


#line 2363
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2100
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2107
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2114
    float _S109 = 1.0f - alpha2_0;

#line 2119
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S109 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S109 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3192
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3192
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3252
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3224
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2821
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3179
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_13)
{

#line 3179
    uint _S110;

    if(uint(pixel_1.x) < (kernelContext_13->frame_0->shadow_filter_0.z))
    {

#line 3181
        _S110 = kernelContext_13->frame_0->shadow_filter_0.x;

#line 3181
    }
    else
    {

#line 3181
        _S110 = kernelContext_13->frame_0->shadow_filter_0.y;

#line 3181
    }

#line 3181
    return _S110;
}


#line 3204
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3204
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 349
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3274
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S111 = spoke_0.x;

#line 3279
    float _S112 = rotation_0.x;

#line 3279
    float _S113 = spoke_0.y;

#line 3279
    float _S114 = rotation_0.y;


    float _S115 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S111 * _S112 - _S113 * _S114, _S111 * _S114 + _S113 * _S112) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3282
    return _S115;
}


#line 3362
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_17)
{

#line 3362
    float4 _S116 = atlas_rect_1(tile_2, kernelContext_17);


    if(atlas_rect_is_empty_0(_S116))
    {
        return 1.0f;
    }

#line 3367
    float2 _S117 = atlas_step_1(_S116, kernelContext_17);

#line 3367
    int y_1 = int(-1);

#line 3367
    float visibility_0 = 0.0f;

#line 3372
    for(;;)
    {

#line 3372
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3372
            break;
        }

#line 3372
        int x_0 = int(-1);

        for(;;)
        {

#line 3374
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3374
                break;
            }

#line 3374
            float _S118 = tile_tap_0(_S116, _S117, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_17);

            float visibility_1 = visibility_0 + _S118;

#line 3374
            x_0 = x_0 + int(1);

#line 3374
            visibility_0 = visibility_1;

#line 3374
        }

#line 3372
        y_1 = y_1 + int(1);

#line 3372
    }

#line 3380
    return visibility_0 / 9.0f;
}


#line 3137
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3304
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_18)
{
    float2 _S119 = shadow_rotation_0(pixel_3);

#line 3306
    float4 _S120 = atlas_rect_1(tile_3, kernelContext_18);

    if(atlas_rect_is_empty_0(_S120))
    {
        return 1.0f;
    }

#line 3310
    float2 _S121 = atlas_step_1(_S120, kernelContext_18);

#line 3310
    uint spot_0 = 0U;

#line 3310
    float probe_0 = 0.0f;

#line 3315
    for(;;)
    {

#line 3315
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3315
            break;
        }

#line 3315
        float _S122 = tile_tap_0(_S120, _S121, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S119, reference_2, kernelContext_18);

        float probe_1 = probe_0 + _S122;

#line 3315
        spot_0 = spot_0 + 1U;

#line 3315
        probe_0 = probe_1;

#line 3315
    }

#line 3324
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3330
    uint index_2 = 0U;

#line 3330
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3334
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3334
            break;
        }

#line 3334
        float _S123 = tile_tap_0(_S120, _S121, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S119, reference_2, kernelContext_18);

        float visibility_3 = visibility_2 + _S123;

#line 3334
        index_2 = index_2 + 1U;

#line 3334
        visibility_2 = visibility_3;

#line 3334
    }

#line 3339
    return visibility_2 / 32.0f;
}


#line 3415
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_19)
{
    float2 texel_1 = kernelContext_19->frame_0->shadow_params_0.xy;

#line 3417
    float4 _S124 = atlas_rect_0(cascade_0, kernelContext_19);

#line 3417
    float2 _S125 = atlas_step_0(_S124, kernelContext_19);


    float2 _S126 = float2(0.5f, 0.5f) * _S125;


    float2 _S127 = float2(1.0f, 1.0f);

#line 3423
    float2 _S128 = _S127 / texel_1;

#line 3423
    uint index_3 = 0U;

#line 3423
    float sum_2 = 0.0f;

#line 3423
    float found_0 = 0.0f;



    for(;;)
    {

#line 3427
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3427
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S129 = spoke_1.x;

#line 3430
        float _S130 = rotation_1.x;

#line 3430
        float _S131 = spoke_1.y;

#line 3430
        float _S132 = rotation_1.y;

#line 3438
        int3 _S133 = int3(int2(min(atlas_uv_0(_S124, clamp(tile_uv_4 + float2(_S129 * _S130 - _S131 * _S132, _S129 * _S132 + _S131 * _S130) * _S125, _S126, float2(1.0f)  - _S126)) * _S128, _S128 - _S127)), int(0));

#line 3438
        float depth_1 = ((kernelContext_19->shadow_atlas_0).read(vec<uint,2>(((_S133)).xy), uint(((_S133)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3442
            sum_2 = sum_2 + depth_1;

#line 3442
            found_0 = found_1;

#line 3439
        }

#line 3427
        index_3 = index_3 + 1U;

#line 3427
    }

#line 3446
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3457
    float _S134 = 2.0f * kernelContext_19->frame_0->cascade_far_0[cascade_0];

#line 3457
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S134 + 40.0f);

#line 3457
    float _S135 = tile_texels_0(_S124, kernelContext_19);

    return clamp(separation_0 * 0.01999999955296516f / (_S134 / _S135), 2.0f, 8.0f);
}


#line 3511
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_20)
{

#line 3512
    float4 _S136 = atlas_rect_0(cascade_1, kernelContext_20);

#line 3546
    if(atlas_rect_is_empty_0(_S136))
    {


        return 1.0f;
    }
    float _S137 = 2.0f * kernelContext_20->frame_0->cascade_far_0[cascade_1];

#line 3552
    float _S138 = tile_texels_0(_S136, kernelContext_20);

#line 3552
    float texel_world_0 = _S137 / _S138;

#line 3559
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3563
    bool _S139;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3564
        _S139 = true;

#line 3564
    }
    else
    {

#line 3564
        _S139 = (ndc_0.z) <= 0.0f;

#line 3564
    }

#line 3564
    if(_S139)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3574
    uint _S140 = shadow_filter_mode_0(pixel_4, kernelContext_20);

#line 3591
    if(_S140 == 2U)
    {

#line 3591
        float _S141 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_20);

        return _S141;
    }
    if(_S140 == 1U)
    {

#line 3595
        float _S142 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_20);



        return _S142;
    }

    float _S143 = ndc_0.z;

#line 3602
    float _S144 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S143, shadow_rotation_0(pixel_4), kernelContext_20);

#line 3602
    float _S145 = tile_pcf_0(cascade_1, tile_uv_5, _S143, pixel_4, _S144, kernelContext_20);
    return _S145;
}


#line 3682
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_21)
{
    uint cascade_2;

#line 3684
    bool covered_0;

#line 3693
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3705
    float eye_distance_0 = length(world_position_5 - kernelContext_21->frame_0->camera_position_0.xyz);

#line 3705
    uint index_4 = 0U;

#line 3713
    for(;;)
    {

#line 3713
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3713
            covered_0 = false;

#line 3713
            cascade_2 = 1U;

#line 3713
            break;
        }
        if(eye_distance_0 < kernelContext_21->frame_0->cascade_far_0[index_4])
        {

#line 3715
            covered_0 = true;

#line 3715
            cascade_2 = index_4;



            break;
        }

#line 3713
        index_4 = index_4 + 1U;

#line 3713
    }

#line 3722
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 3722
    }

#line 3722
    float _S146 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3729
    uint _S147 = cascade_2 + 1U;

#line 3729
    if(_S147 >= 2U)
    {



        return _S146;
    }

#line 3742
    float band_0 = kernelContext_21->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_21->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S146;
    }

#line 3750
    float _S148 = cascade_visibility_0(_S147, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3761
    return mix(_S146, _S148, blend_0);
}


#line 4949
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_22)
{

#line 4949
    texture2d<float, access::sample> _S149 = kernelContext_22->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S149).get_width(0)),(*((&height_2)) = (_S149).get_height(0));

    int3 _S150 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 4955
    return ((kernelContext_22->contact_shadow_0).read(vec<uint,2>(((_S150)).xy), uint(((_S150)).z)).x);
}


#line 3654
float3 cascade_tint_0(uint cascade_3, float blend_1)
{
    if(cascade_3 >= 2U)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }
    uint _S151 = cascade_3 + 1U;

#line 3660
    if(_S151 >= 2U)
    {


        return CASCADE_TINTS_0[cascade_3];
    }
    return mix(CASCADE_TINTS_0[cascade_3], CASCADE_TINTS_0[_S151], float3(blend_1) );
}


#line 3972
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S152 = axis_2.x;

#line 3975
    float _S153 = axis_2.y;

#line 3975
    bool _S154;

#line 3975
    if(_S152 >= _S153)
    {

#line 3975
        _S154 = _S152 >= (axis_2.z);

#line 3975
    }
    else
    {

#line 3975
        _S154 = false;

#line 3975
    }

#line 3975
    uint _S155;

#line 3975
    if(_S154)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3977
            _S155 = 0U;

#line 3977
        }
        else
        {

#line 3977
            _S155 = 1U;

#line 3977
        }

#line 3977
        return _S155;
    }
    if(_S153 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3981
            _S155 = 2U;

#line 3981
        }
        else
        {

#line 3981
            _S155 = 3U;

#line 3981
        }

#line 3981
        return _S155;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3983
        _S155 = 4U;

#line 3983
    }
    else
    {

#line 3983
        _S155 = 5U;

#line 3983
    }

#line 3983
    return _S155;
}


#line 336
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 3868
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_23)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 3871
    float4 _S156 = atlas_rect_0(atlas_0, kernelContext_23);

    if(atlas_rect_is_empty_0(_S156))
    {


        return 1.0f;
    }

#line 3877
    float _S157 = tile_texels_0(_S156, kernelContext_23);

    float texel_world_1 = map_world_0 / _S157;

#line 3889
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 3896
    float _S158 = clip_1.w;

#line 3896
    if(_S158 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S158) ;

#line 3900
    bool _S159;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3901
        _S159 = true;

#line 3901
    }
    else
    {

#line 3901
        _S159 = (ndc_1.z) <= 0.0f;

#line 3901
    }

#line 3901
    if(_S159)
    {

#line 3901
        _S159 = true;

#line 3901
    }
    else
    {

#line 3901
        _S159 = (ndc_1.z) > 1.0f;

#line 3901
    }

#line 3901
    if(_S159)
    {

#line 3908
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 3913
    uint _S160 = shadow_filter_mode_0(pixel_6, kernelContext_23);

#line 3922
    if(_S160 == 2U)
    {

#line 3922
        float _S161 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_23);

        return _S161;
    }

#line 3924
    float _S162 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_23);

    return _S162;
}


#line 3991
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_24)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3999
    float _S163 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_24);

#line 4005
    return _S163;
}


#line 3933
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_25)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3940
    float4 _S164 = float4(light_2->direction_0) ;

#line 3947
    float cos_outer_1 = _S164.w;

#line 3947
    float _S165 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S164.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_25);

#line 3954
    return _S165;
}


#line 2304
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 4936
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 4938
    float3 _S166;
    if((length(decoded_0)) < 0.5f)
    {

#line 4939
        _S166 = shading_normal_1;

#line 4939
    }
    else
    {

#line 4939
        _S166 = normalize(decoded_0);

#line 4939
    }

#line 4939
    return _S166;
}


#line 4574
float3 sky_irradiance_0(float3 normal_7, KernelContext_0 thread* kernelContext_26)
{
    float4 basis_6 = float4(normal_7, 1.0f);
    return max(float3(dot(kernelContext_26->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4478
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4478
    float reach_0 = 0.0f;

#line 4478
    uint axis_3 = 0U;


    for(;;)
    {

#line 4481
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4481
            break;
        }

#line 4481
        uint _S167 = axis_3;

#line 4481
        bool _S168;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4483
            _S168 = true;

#line 4483
        }
        else
        {

#line 4483
            _S168 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4483
        }

#line 4483
        if(_S168)
        {

#line 4484
            axis_3 = axis_3 + 1U;

#line 4481
            continue;
        }

#line 4481
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S167] - 1.0f));

#line 4481
        axis_3 = axis_3 + 1U;

#line 4481
    }

#line 4488
    return reach_0;
}


#line 4508
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4508
    uint level_0 = 0U;

    for(;;)
    {

#line 4510
        uint _S169 = level_0 + 1U;

#line 4510
        if(_S169 < levels_0)
        {
        }
        else
        {

#line 4510
            break;
        }
        float _S170 = float(level_0);

#line 4512
        float at_3 = reach_1 * exp2(- _S170);
        if(at_3 < 1.0f)
        {

#line 4514
            return float2(_S170, saturate((1.0f - at_3) / 0.25f));
        }

#line 4510
        level_0 = _S169;

#line 4510
    }

#line 4516
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4265
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4267
    uint _S171;
    if(at_4 >= count_2)
    {

#line 4268
        _S171 = at_4 - count_2;

#line 4268
    }
    else
    {

#line 4268
        _S171 = at_4;

#line 4268
    }

#line 4268
    return _S171;
}


#line 4291
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_27)
{
    uint3 counts_0 = kernelContext_27->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_27->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S172 = counts_0.x;
    uint _S173 = counts_0.y;



    return min(kernelContext_27->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S173 + probe_wrap_0(cell_2.y, offset_1.y, _S173)) * _S172 + probe_wrap_0(cell_2.x, offset_1.x, _S172), max(kernelContext_27->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4132
float sign_not_zero_0(float value_0)
{

#line 4132
    float _S174;

    if(value_0 >= 0.0f)
    {

#line 4134
        _S174 = 1.0f;

#line 4134
    }
    else
    {

#line 4134
        _S174 = -1.0f;

#line 4134
    }

#line 4134
    return _S174;
}


#line 4151
float2 oct_encode_0(float3 direction_1)
{
    float _S175 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S175) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4154
    float2 p_1;
    if(_S175 < 0.0f)
    {
        float _S176 = p_0.y;

#line 4157
        float _S177 = p_0.x;

#line 4157
        p_1 = float2((1.0f - abs(_S176)) * sign_not_zero_0(_S177), (1.0f - abs(_S177)) * sign_not_zero_0(_S176));

#line 4155
    }
    else
    {

#line 4155
        p_1 = p_0;

#line 4155
    }

#line 4160
    return p_1;
}


#line 4180
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_28)
{

#line 4180
    texture2d_array<float, access::sample> _S178 = kernelContext_28->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S178).get_width(0)),(*((&height_3)) = (_S178).get_height(0)),(*((&layers_0)) = (_S178).get_array_size());

#line 4185
    float2 _S179 = float2(0.5f) ;

#line 4185
    float2 _S180 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S179 + _S179) * float2(16.0f)  + _S180 - _S179;
    float2 _S181 = float2(float(width_3), float(height_3)) - _S180;

#line 4189
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S181);
    float2 high_2 = min(low_2 + _S180, _S181);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S182 = int(low_2.x);

#line 4194
    int _S183 = int(low_2.y);

#line 4194
    int4 _S184 = int4(_S182, _S183, layer_1, int(0));
    int _S185 = int(high_2.x);

#line 4195
    int4 _S186 = int4(_S185, _S183, layer_1, int(0));
    int _S187 = int(high_2.y);

#line 4196
    int4 _S188 = int4(_S182, _S187, layer_1, int(0));
    int4 _S189 = int4(_S185, _S187, layer_1, int(0));
    float2 _S190 = float2(weight_2.x) ;

#line 4198
    return mix(mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S184)).xy), uint(((_S184)).z), uint(((_S184)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S186)).xy), uint(((_S186)).z), uint(((_S186)).w))).xy, _S190), mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S188)).xy), uint(((_S188)).z), uint(((_S188)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S189)).xy), uint(((_S189)).z), uint(((_S189)).w))).xy, _S190), float2(weight_2.y) );
}


#line 4226
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_8, KernelContext_0 thread* kernelContext_29)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_8 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4229
    float2 _S191 = probe_moments_0(index_6, - to_probe_0, kernelContext_29);

#line 4235
    float _S192 = _S191.x;

#line 4235
    float _S193 = max(_S191.y - _S192 * _S192, 0.0f);
    float behind_0 = to_surface_0 - _S192;
    float bound_0 = _S193 / (_S193 + behind_0 * behind_0);

#line 4237
    float _S194;
    if(to_surface_0 <= _S192)
    {

#line 4238
        _S194 = 1.0f;

#line 4238
    }
    else
    {

#line 4238
        _S194 = bound_0 * bound_0 * bound_0;

#line 4238
    }

#line 4238
    return _S194;
}


#line 4248
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_9, KernelContext_0 thread* kernelContext_30)
{

#line 4248
    float _S195 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_9, kernelContext_30);

    return max(_S195, 0.00009999999747379f);
}


#line 1127
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4310
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4337
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_10, KernelContext_0 thread* kernelContext_31)
{

#line 4338
    uint _S196 = probe_row_0(level_2, cell_3, kernelContext_31);


    GpuProbe_natural_0 stored_0 = kernelContext_31->probes_0[_S196];

#line 4341
    float _S197 = probe_weight_0(_S196, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_10, kernelContext_31);



    thread WeightedProbe_0 corner_2;

#line 4345
    float4 _S198 = float4(_S197) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S198;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S198;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S198;
    (&corner_2)->weight_3 = _S197;
    return corner_2;
}


#line 4321
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S199 = float4(t_1) ;

#line 4324
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S199);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S199);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S199);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4409
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_11, KernelContext_0 thread* kernelContext_32)
{

#line 4409
    float3 _S200 = float3(1.0f) ;

#line 4414
    float3 _S201 = float3(0.0f, 0.0f, 0.0f);

#line 4414
    float3 last_1 = max(float3(kernelContext_32->frame_0->probe_counts_0.xyz) - _S200, _S201);



    float3 origin_2 = kernelContext_32->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_32->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S201, last_1);
    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S202 = uint3(base_2);



    uint3 _S203 = uint3(min(base_2 + _S200, last_1));

#line 4434
    float _S204 = inv_0.x;

#line 4434
    float _S205;

#line 4434
    if(_S204 != 0.0f)
    {

#line 4434
        _S205 = 1.0f / _S204;

#line 4434
    }
    else
    {

#line 4434
        _S205 = 0.0f;

#line 4434
    }
    float _S206 = inv_0.y;

#line 4435
    float _S207;

#line 4435
    if(_S206 != 0.0f)
    {

#line 4435
        _S207 = 1.0f / _S206;

#line 4435
    }
    else
    {

#line 4435
        _S207 = 0.0f;

#line 4435
    }
    float _S208 = inv_0.z;

#line 4436
    float _S209;

#line 4436
    if(_S208 != 0.0f)
    {

#line 4436
        _S209 = 1.0f / _S208;

#line 4436
    }
    else
    {

#line 4436
        _S209 = 0.0f;

#line 4436
    }

#line 4434
    float3 spacing_1 = float3(_S205, _S207, _S209);

#line 4443
    uint _S210 = _S202.x;

#line 4443
    uint _S211 = _S202.y;

#line 4443
    uint _S212 = _S202.z;

#line 4443
    WeightedProbe_0 _S213 = probe_corner_0(level_3, uint3(_S210, _S211, _S212), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);
    uint _S214 = _S203.x;

#line 4444
    WeightedProbe_0 _S215 = probe_corner_0(level_3, uint3(_S214, _S211, _S212), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);

#line 4444
    float _S216 = f_0.x;

#line 4444
    thread WeightedProbe_0 _S217 = _S213;

#line 4444
    thread WeightedProbe_0 _S218 = _S215;

#line 4444
    WeightedProbe_0 _S219 = lerp_probe_0(&_S217, &_S218, _S216);
    uint _S220 = _S203.y;

#line 4445
    WeightedProbe_0 _S221 = probe_corner_0(level_3, uint3(_S210, _S220, _S212), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);

#line 4445
    WeightedProbe_0 _S222 = probe_corner_0(level_3, uint3(_S214, _S220, _S212), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);

#line 4445
    thread WeightedProbe_0 _S223 = _S221;

#line 4445
    thread WeightedProbe_0 _S224 = _S222;

#line 4445
    WeightedProbe_0 _S225 = lerp_probe_0(&_S223, &_S224, _S216);

    uint _S226 = _S203.z;

#line 4447
    WeightedProbe_0 _S227 = probe_corner_0(level_3, uint3(_S210, _S211, _S226), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);

#line 4447
    WeightedProbe_0 _S228 = probe_corner_0(level_3, uint3(_S214, _S211, _S226), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);

#line 4447
    thread WeightedProbe_0 _S229 = _S227;

#line 4447
    thread WeightedProbe_0 _S230 = _S228;

#line 4447
    WeightedProbe_0 _S231 = lerp_probe_0(&_S229, &_S230, _S216);

#line 4447
    WeightedProbe_0 _S232 = probe_corner_0(level_3, uint3(_S210, _S220, _S226), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);

#line 4447
    WeightedProbe_0 _S233 = probe_corner_0(level_3, uint3(_S214, _S220, _S226), origin_2, spacing_1, world_position_13, normal_11, kernelContext_32);

#line 4447
    thread WeightedProbe_0 _S234 = _S232;

#line 4447
    thread WeightedProbe_0 _S235 = _S233;

#line 4447
    WeightedProbe_0 _S236 = lerp_probe_0(&_S234, &_S235, _S216);



    float _S237 = f_0.y;

#line 4451
    thread WeightedProbe_0 _S238 = _S219;

#line 4451
    thread WeightedProbe_0 _S239 = _S225;

#line 4451
    WeightedProbe_0 _S240 = lerp_probe_0(&_S238, &_S239, _S237);

#line 4451
    thread WeightedProbe_0 _S241 = _S231;

#line 4451
    thread WeightedProbe_0 _S242 = _S236;

#line 4451
    WeightedProbe_0 _S243 = lerp_probe_0(&_S241, &_S242, _S237);

    float _S244 = f_0.z;

#line 4453
    thread WeightedProbe_0 _S245 = _S240;

#line 4453
    thread WeightedProbe_0 _S246 = _S243;

#line 4453
    WeightedProbe_0 _S247 = lerp_probe_0(&_S245, &_S246, _S244);

    float4 basis_7 = float4(normal_11, 1.0f);
    return max(float3(dot(_S247.sh_0.sh_r_0, basis_7), dot(_S247.sh_0.sh_g_0, basis_7), dot(_S247.sh_0.sh_b_0, basis_7)) / float3(_S247.weight_3) , _S201);
}


#line 4543
float3 probe_irradiance_0(float3 world_position_14, float3 normal_12, KernelContext_0 thread* kernelContext_33)
{

#line 4551
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_33->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_33->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_33->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_33->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 4553
    float3 _S248 = probe_level_irradiance_0(level_4, world_position_14, normal_12, kernelContext_33);


    if(share_0 >= 1.0f)
    {

#line 4557
        return _S248;
    }

#line 4557
    float3 _S249 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_12, kernelContext_33);

    return _S249 * float3((1.0f - share_0))  + _S248 * float3(share_0) ;
}


#line 5005
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 5005
    float3 _S250 = float3(visibility_4) ;

#line 5011
    return min(float3(1.0f) , max(_S250, ((_S250 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S250 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S250));
}


#line 1068
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_5)
{
    return float3(material_5->emissive_r_0, material_5->emissive_g_0, material_5->emissive_b_0);
}


#line 2655
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S251 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2663
    float kernel_0 = 0.0001984127011383f;

#line 2663
    int term_0 = int(6);

    for(;;)
    {

#line 2665
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2665
            break;
        }
        float _S252 = kernel_0 * _S251 + FOG_KERNEL_0[term_0];

#line 2665
        int term_1 = term_0 - int(1);

#line 2665
        kernel_0 = _S252;

#line 2665
        term_0 = term_1;

#line 2665
    }

#line 2672
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2682
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S253 = - d_0;

#line 2686
        float series_0 = 0.00833333376795053f;

#line 2686
        int term_2 = int(3);

        for(;;)
        {

#line 2688
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2688
                break;
            }
            float _S254 = series_0 * _S253 + FOG_RATIO_KERNEL_0[term_2];

#line 2688
            int term_3 = term_2 - int(1);

#line 2688
            series_0 = _S254;

#line 2688
            term_2 = term_3;

#line 2688
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2716
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2727
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2735
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4600
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4600
struct pixelInput_0
{
    float3 world_position_15 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    [[flat]] uint material_6 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
    float3 world_tangent_1 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_4 [[user(TEXCOORD_5)]];
};


#line 5047
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S255 [[stage_in]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 5047
    thread KernelContext_0 kernelContext_34;

#line 5047
    (&kernelContext_34)->draw_0 = draw_3;

#line 5047
    (&kernelContext_34)->visible_instances_0 = visible_instances_3;

#line 5047
    (&kernelContext_34)->instances_0 = instances_3;

#line 5047
    (&kernelContext_34)->meshes_0 = meshes_3;

#line 5047
    (&kernelContext_34)->frame_0 = frame_5;

#line 5047
    (&kernelContext_34)->vertices_0 = vertices_3;

#line 5047
    (&kernelContext_34)->ambient_occlusion_0 = ambient_occlusion_3;

#line 5047
    (&kernelContext_34)->materials_0 = materials_3;

#line 5047
    (&kernelContext_34)->base_color_textures_0 = base_color_textures_3;

#line 5047
    (&kernelContext_34)->base_color_sampler_0 = base_color_sampler_3;

#line 5047
    (&kernelContext_34)->normal_textures_0 = normal_textures_3;

#line 5047
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_3;

#line 5047
    (&kernelContext_34)->specular_dfg_0 = specular_dfg_3;

#line 5047
    (&kernelContext_34)->lights_0 = lights_3;

#line 5047
    (&kernelContext_34)->ltc_matrix_0 = ltc_matrix_3;

#line 5047
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_3;

#line 5047
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_3;

#line 5047
    (&kernelContext_34)->contact_shadow_0 = contact_shadow_3;

#line 5047
    (&kernelContext_34)->probes_0 = probes_3;

#line 5047
    (&kernelContext_34)->probe_visibility_0 = probe_visibility_3;

#line 5059
    float3 vertex_normal_0 = normalize(_S255.world_normal_1);

#line 5064
    float2 motion_1 = motion_vector_0(_S255.clip_position_1, _S255.previous_clip_position_1);

#line 5080
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 5082
        float4 _S256 = occlusion_at_0(position_5.xy, &kernelContext_34);



        (&bent_0)->lit_0 = float4(_S256.yzw, 1.0f);


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

#line 5136
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5136
        float4 _S257 = occlusion_at_0(position_5.xy, &kernelContext_34);


        float value_1 = _S257.x;

#line 5138
        thread FragmentOutput_0 occlusion_1;

#line 5147
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S255.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 5164
    thread GpuMaterial_natural_0 _S258 = (&kernelContext_34)->materials_0[_S255.material_6];

#line 5164
    float2 uv_3;

#line 5189
    if(((&_S258)->tiling_0) == 1U)
    {

#line 5189
        uv_3 = physical_tile_uv_0(_S255.world_position_15, vertex_normal_0, (&_S258)->tile_metres_0);

#line 5189
    }
    else
    {

#line 5189
        uv_3 = _S255.uv_2;

#line 5189
    }

#line 5189
    uint _S259 = base_color_layer_0(&_S258);

#line 5207
    float3 _S260 = float3(uv_3, float(_S259));
    float4 albedo_1 = _S255.color_3 * float4((&_S258)->base_color_0)  * (((&kernelContext_34)->base_color_textures_0).sample(((&kernelContext_34)->base_color_sampler_0), ((_S260)).xy, uint(((_S260)).z)));

#line 5222
    float _S261 = albedo_1.w;

#line 5222
    bool _S262 = alpha_masked_0(&_S258, _S261);

#line 5222
    if(_S262)
    {
        discard_fragment();

#line 5222
    }

#line 5222
    uint _S263 = normal_layer_0(&_S258);

#line 5222
    thread VertexOutput_0 _S264;

#line 5222
    (&_S264)->position_3 = position_5;

#line 5222
    (&_S264)->world_position_1 = _S255.world_position_15;

#line 5222
    (&_S264)->world_normal_0 = _S255.world_normal_1;

#line 5222
    (&_S264)->color_2 = _S255.color_3;

#line 5222
    (&_S264)->material_4 = _S255.material_6;

#line 5222
    (&_S264)->uv_0 = _S255.uv_2;

#line 5222
    (&_S264)->clip_position_0 = _S255.clip_position_1;

#line 5222
    (&_S264)->previous_clip_position_0 = _S255.previous_clip_position_1;

#line 5222
    (&_S264)->world_tangent_0 = _S255.world_tangent_1;

#line 5222
    (&_S264)->frame_3 = _S255.frame_4;

#line 5222
    float3 _S265 = shading_normal_of_0(_S263, (&_S258)->normal_scale_0, &_S264, vertex_normal_0, uv_3, &kernelContext_34);

#line 5230
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5232
        float3 _S266 = float3(0.5f) ;

#line 5244
        (&normals_0)->lit_0 = float4(_S265 * _S266 + _S266, 1.0f);

#line 5250
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_34)->frame_0->camera_position_0.xyz - _S255.world_position_15);



    float3 _S267 = geometric_normal_of_0(_S255.world_position_15, vertex_normal_0);

#line 5265
    float metallic_1 = saturate((&_S258)->metallic_0);
    float roughness_2 = clamp((&_S258)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_1 = roughness_2 * roughness_2;

#line 5300
    float _S268 = saturate(alpha_1 * alpha_1 + specular_aa_kernel_0(_S265));

#line 5306
    float3 _S269 = albedo_1.xyz;

#line 5306
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S269, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S269 * float3((1.0f - metallic_1)) ;

#line 5313
    float _S270 = max(dot(_S265, to_eye_1), 0.00009999999747379f);

#line 5323
    float2 _S271 = position_5.xy;

#line 5323
    uint _S272 = froxel_of_0(_S271, (((float4(_S255.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_34);

#line 5323
    uint base_3 = _S272 * 17U;

#line 5328
    uint _S273 = min((&kernelContext_34)->cluster_lights_0[base_3], 16U);

#line 5328
    TableTap_0 _S274 = table_tap_0(_S270, roughness_2, &kernelContext_34);

#line 5328
    thread TableTap_0 _S275 = _S274;

#line 5328
    float2 _S276 = dfg_at_0(&_S275, &kernelContext_34);

#line 5337
    float _S277 = _S276.x;

#line 5337
    float _S278 = _S276.y;

#line 5337
    float3 _S279 = f0_2 * float3(_S277)  + float3(_S278) ;

#line 5343
    float3 _S280 = float3(0.0f, 0.0f, 0.0f);

#line 5343
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5343
    uint slot_0 = 0U;

#line 5343
    float3 direct_0 = _S280;

#line 5343
    float3 gloss_0 = _S280;

#line 5353
    for(;;)
    {

#line 5353
        if(slot_0 < _S273)
        {
        }
        else
        {

#line 5353
            break;
        }

#line 5353
        thread GpuLight_natural_0 _S281 = (&kernelContext_34)->lights_0[(&kernelContext_34)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 5353
        uint _S282 = (&_S281)->kind_0;

#line 5362
        bool _S283 = ((&_S281)->kind_0) == 0U;

#line 5362
        float3 to_light_7;

#line 5362
        float reach_2;

#line 5362
        if(_S283)
        {

#line 5362
            to_light_7 = normalize((float4((&_S281)->direction_0) ).xyz);

#line 5362
            reach_2 = 1.0f;

#line 5362
        }
        else
        {


            if(_S282 == 3U)
            {

#line 5367
                float4 _S284 = float4((&_S281)->position_0) ;

#line 5375
                float3 offset_2 = _S284.xyz - _S255.world_position_15;
                float distance_3 = length(offset_2);

                float _S285 = range_window_0(distance_3, _S284.w);

#line 5378
                to_light_7 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5378
                reach_2 = _S285;

#line 5367
            }
            else
            {

#line 5367
                float4 _S286 = float4((&_S281)->position_0) ;

#line 5382
                float3 offset_3 = _S286.xyz - _S255.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_8 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S286.w);
                if(_S282 == 2U)
                {

#line 5386
                    float4 _S287 = float4((&_S281)->direction_0) ;

#line 5386
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S287.xyz, _S287.w, (&_S281)->cos_inner_0);

#line 5386
                }
                else
                {

#line 5386
                    reach_2 = reach_3;

#line 5386
                }

#line 5386
                to_light_7 = to_light_8;

#line 5367
            }

#line 5362
        }

#line 5395
        float n_dot_l_5 = dot(_S265, to_light_7);

#line 5395
        float3 specular_0;

#line 5395
        float diffuse_0;


        if(_S282 == 3U)
        {

#line 5408
            thread array<float3, int(4)> corners_2;

#line 5408
            rect_corners_0(&_S281, _S255.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S265, to_eye_1, _S270);

#line 5410
            thread array<float3, int(4)> _S288 = corners_2;

#line 5410
            float _S289 = ltc_irradiance_0(to_local_0, &_S288);

#line 5410
            thread TableTap_0 _S290 = _S274;

#line 5410
            float4 _S291 = ltc_at_0(&_S290, &kernelContext_34);

            matrix<float,int(3),int(3)>  _S292 = (((to_local_0) * (ltc_transform_0(_S291))));

#line 5412
            thread array<float3, int(4)> _S293 = corners_2;

#line 5412
            float _S294 = ltc_irradiance_0(_S292, &_S293);
            float3 _S295 = float3(_S294)  * _S279;

#line 5413
            diffuse_0 = _S289;

#line 5413
            specular_0 = _S295;

#line 5398
        }
        else
        {

#line 5418
            float _S296 = max(n_dot_l_5, 0.0f);

#line 5425
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5433
            float3 specular_1 = ggx_lobe_0(_S268, f0_2, _S296, _S270, max(dot(_S265, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S296) ;

#line 5433
            diffuse_0 = _S296;

#line 5433
            specular_0 = specular_1;

#line 5398
        }

#line 5398
        float3 specular_2;

#line 5441
        if((((&_S281)->flags_3) & 1U) != 0U)
        {

#line 5441
            specular_2 = _S280;

#line 5441
        }
        else
        {

#line 5441
            specular_2 = specular_0;

#line 5441
        }

#line 5441
        float reach_4;

#line 5459
        if(_S283)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 5462
            float _S297 = sun_visibility_0(_S255.world_position_15, to_light_7, n_dot_l_5, _S267, _S271, &sun_cascade_0, &sun_fade_0, &kernelContext_34);

#line 5462
            float _S298 = contact_at_0(_S271, &kernelContext_34);

#line 5471
            float _S299 = _S297 * _S298;

#line 5471
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 5471
            reach_4 = _S299;

#line 5459
        }
        else
        {

#line 5476
            if(_S282 == 1U)
            {

#line 5476
                uint _S300 = (&_S281)->shadow_tile_0;

#line 5488
                if(((&_S281)->shadow_tile_0) <= 8U)
                {

#line 5488
                    float _S301 = point_visibility_0(&_S281, _S300, _S255.world_position_15, to_light_7, n_dot_l_5, _S267, _S271, &kernelContext_34);

#line 5488
                    reach_4 = reach_2 * _S301;

#line 5488
                }
                else
                {

#line 5488
                    reach_4 = reach_2;

#line 5488
                }

#line 5476
            }
            else
            {

#line 5476
                uint _S302 = (&_S281)->shadow_tile_0;

#line 5494
                if(((&_S281)->shadow_tile_0) < 14U)
                {

#line 5494
                    float _S303 = spot_visibility_0(&_S281, _S302, _S255.world_position_15, to_light_7, n_dot_l_5, _S267, _S271, &kernelContext_34);

#line 5494
                    reach_4 = reach_2 * _S303;

#line 5494
                }
                else
                {

#line 5494
                    reach_4 = reach_2;

#line 5494
                }

#line 5476
            }

#line 5459
        }

#line 5502
        float3 _S304 = (float4((&_S281)->color_0) ).xyz;

#line 5502
        float3 direct_1 = direct_0 + _S304 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S304 * (specular_2 * float3(reach_4) );

#line 5353
        slot_0 = slot_0 + 1U;

#line 5353
        direct_0 = direct_1;

#line 5353
        gloss_0 = gloss_1;

#line 5353
    }

#line 5517
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S277 + _S278);

#line 5517
    float4 _S305 = occlusion_at_0(_S271, &kernelContext_34);

#line 5536
    float occluded_0 = _S305.x;

#line 5545
    float3 bent_normal_0 = bent_normal_at_0(_S305, _S265);

#line 5568
    float3 _S306 = frame_5->ambient_0.xyz;

#line 5568
    float3 _S307 = sky_irradiance_0(bent_normal_0, &kernelContext_34);

#line 5568
    float3 _S308 = _S306 + _S307;

#line 5568
    float3 _S309 = probe_irradiance_0(_S255.world_position_15, bent_normal_0, &kernelContext_34);

#line 5604
    float3 lit_1 = diffuse_albedo_0 * ((_S308 + _S309) * multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) + direct_0) + gloss_2;

#line 5604
    float3 _S310 = emissive_of_0(&_S258);

#line 5640
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_34)->frame_0->fog_params_0.x, (&kernelContext_34)->frame_0->fog_params_0.y, (&kernelContext_34)->frame_0->camera_position_0.y - (&kernelContext_34)->frame_0->fog_params_0.z, _S255.world_position_15.y - (&kernelContext_34)->frame_0->fog_params_0.z, length((&kernelContext_34)->frame_0->camera_position_0.xyz - _S255.world_position_15)));
    float3 lit_2 = (lit_1 + _S310) * float3(fog_survives_0)  + (&kernelContext_34)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;



    (&output_2)->lit_0 = float4(lit_2, _S261);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 5660
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, _S261);

#line 5669
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 5660
    }

#line 5671
    return output_2;
}


#line 5671
struct pixelInput_1
{
    float3 world_position_16 [[user(POSITION)]];
    float3 world_normal_2 [[user(NORMAL)]];
    float4 color_4 [[user(COLOR)]];
    [[flat]] uint material_7 [[user(TEXCOORD)]];
    float2 uv_4 [[user(TEXCOORD_1)]];
    float4 clip_position_2 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_2 [[user(TEXCOORD_3)]];
    float3 world_tangent_2 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_6 [[user(TEXCOORD_5)]];
};


#line 5704
[[fragment]] void depthMaskedFragmentMain(pixelInput_1 _S311 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 5704
    thread KernelContext_0 kernelContext_35;

#line 5704
    (&kernelContext_35)->draw_0 = draw_4;

#line 5704
    (&kernelContext_35)->visible_instances_0 = visible_instances_4;

#line 5704
    (&kernelContext_35)->instances_0 = instances_4;

#line 5704
    (&kernelContext_35)->meshes_0 = meshes_4;

#line 5704
    (&kernelContext_35)->frame_0 = frame_7;

#line 5704
    (&kernelContext_35)->vertices_0 = vertices_4;

#line 5704
    (&kernelContext_35)->ambient_occlusion_0 = ambient_occlusion_4;

#line 5704
    (&kernelContext_35)->materials_0 = materials_4;

#line 5704
    (&kernelContext_35)->base_color_textures_0 = base_color_textures_4;

#line 5704
    (&kernelContext_35)->base_color_sampler_0 = base_color_sampler_4;

#line 5704
    (&kernelContext_35)->normal_textures_0 = normal_textures_4;

#line 5704
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_4;

#line 5704
    (&kernelContext_35)->specular_dfg_0 = specular_dfg_4;

#line 5704
    (&kernelContext_35)->lights_0 = lights_4;

#line 5704
    (&kernelContext_35)->ltc_matrix_0 = ltc_matrix_4;

#line 5704
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_4;

#line 5704
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_4;

#line 5704
    (&kernelContext_35)->contact_shadow_0 = contact_shadow_4;

#line 5704
    (&kernelContext_35)->probes_0 = probes_4;

#line 5704
    (&kernelContext_35)->probe_visibility_0 = probe_visibility_4;

#line 5704
    thread GpuMaterial_natural_0 _S312 = materials_4[_S311.material_7];

#line 5704
    float2 uv_5;

#line 5713
    if(((&_S312)->tiling_0) == 1U)
    {

#line 5713
        uv_5 = physical_tile_uv_0(_S311.world_position_16, normalize(_S311.world_normal_2), (&_S312)->tile_metres_0);

#line 5713
    }
    else
    {

#line 5713
        uv_5 = _S311.uv_4;

#line 5713
    }

#line 5713
    uint _S313 = base_color_layer_0(&_S312);

#line 5719
    float3 _S314 = float3(uv_5, float(_S313));

#line 5719
    bool _S315 = alpha_masked_0(&_S312, _S311.color_4.w * (float4((&_S312)->base_color_0) ).w * (((&kernelContext_35)->base_color_textures_0).sample(((&kernelContext_35)->base_color_sampler_0), ((_S314)).xy, uint(((_S314)).z))).w);



    if(_S315)
    {
        discard_fragment();

#line 5723
    }



    return;
}


#line 5757
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_13 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 5757
struct pixelInput_2
{
    float3 world_position_17 [[user(POSITION)]];
    float3 world_normal_3 [[user(NORMAL)]];
    float4 color_5 [[user(COLOR)]];
    [[flat]] uint material_8 [[user(TEXCOORD)]];
    float2 uv_6 [[user(TEXCOORD_1)]];
    float4 clip_position_3 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_3 [[user(TEXCOORD_3)]];
    float3 world_tangent_3 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_8 [[user(TEXCOORD_5)]];
};


#line 5800
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_2 _S316 [[stage_in]], float4 position_7 [[position]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 5800
    thread KernelContext_0 kernelContext_36;

#line 5800
    (&kernelContext_36)->draw_0 = draw_5;

#line 5800
    (&kernelContext_36)->visible_instances_0 = visible_instances_5;

#line 5800
    (&kernelContext_36)->instances_0 = instances_5;

#line 5800
    (&kernelContext_36)->meshes_0 = meshes_5;

#line 5800
    (&kernelContext_36)->frame_0 = frame_9;

#line 5800
    (&kernelContext_36)->vertices_0 = vertices_5;

#line 5800
    (&kernelContext_36)->ambient_occlusion_0 = ambient_occlusion_5;

#line 5800
    (&kernelContext_36)->materials_0 = materials_5;

#line 5800
    (&kernelContext_36)->base_color_textures_0 = base_color_textures_5;

#line 5800
    (&kernelContext_36)->base_color_sampler_0 = base_color_sampler_5;

#line 5800
    (&kernelContext_36)->normal_textures_0 = normal_textures_5;

#line 5800
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_5;

#line 5800
    (&kernelContext_36)->specular_dfg_0 = specular_dfg_5;

#line 5800
    (&kernelContext_36)->lights_0 = lights_5;

#line 5800
    (&kernelContext_36)->ltc_matrix_0 = ltc_matrix_5;

#line 5800
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_5;

#line 5800
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_5;

#line 5800
    (&kernelContext_36)->contact_shadow_0 = contact_shadow_5;

#line 5800
    (&kernelContext_36)->probes_0 = probes_5;

#line 5800
    (&kernelContext_36)->probe_visibility_0 = probe_visibility_5;

#line 5805
    float3 vertex_normal_1 = normalize(_S316.world_normal_3);

#line 5805
    thread GpuMaterial_natural_0 _S317 = materials_5[_S316.material_8];

#line 5805
    float2 uv_7;

#line 5812
    if(((&_S317)->tiling_0) == 1U)
    {

#line 5812
        uv_7 = physical_tile_uv_0(_S316.world_position_17, vertex_normal_1, (&_S317)->tile_metres_0);

#line 5812
    }
    else
    {

#line 5812
        uv_7 = _S316.uv_6;

#line 5812
    }

#line 5812
    uint _S318 = base_color_layer_0(&_S317);

#line 5817
    float3 _S319 = float3(uv_7, float(_S318));
    float4 albedo_3 = _S316.color_5 * float4((&_S317)->base_color_0)  * (((&kernelContext_36)->base_color_textures_0).sample(((&kernelContext_36)->base_color_sampler_0), ((_S319)).xy, uint(((_S319)).z)));

#line 5818
    bool _S320 = alpha_masked_0(&_S317, albedo_3.w);

#line 5824
    if(_S320)
    {
        discard_fragment();

#line 5824
    }

#line 5829
    thread RsmOutput_0 written_0;



    (&written_0)->albedo_2 = float4(albedo_3.xyz * float3((1.0f - saturate((&_S317)->metallic_0))) , 1.0f);

#line 5833
    float3 _S321 = float3(0.5f) ;
    (&written_0)->normal_13 = float4(vertex_normal_1 * _S321 + _S321, 1.0f);
    (&written_0)->world_0 = float4(_S316.world_position_17, 1.0f);
    return written_0;
}


#line 5836
struct vertexMain_Result_0
{
    float4 position_8 [[position]];
    float3 world_position_18 [[user(POSITION)]];
    float3 world_normal_4 [[user(NORMAL)]];
    float4 color_6 [[user(COLOR)]];
    uint material_9 [[user(TEXCOORD)]];
    float2 uv_8 [[user(TEXCOORD_1)]];
    float4 clip_position_4 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_4 [[user(TEXCOORD_3)]];
    float3 world_tangent_4 [[user(TEXCOORD_4)]];
    uint frame_10 [[user(TEXCOORD_5)]];
};


#line 5836
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_6 [[buffer(3)]], uint device* visible_instances_6 [[buffer(5)]], GpuInstance_natural_0 device* instances_6 [[buffer(2)]], GpuMesh_0 device* meshes_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_11 [[buffer(0)]], uint device* vertices_6 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_6 [[texture(2)]], GpuMaterial_natural_0 device* materials_6 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_6 [[texture(0)]], sampler base_color_sampler_6 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_6 [[texture(4)]], uint device* cluster_lights_6 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_6 [[texture(3)]], GpuLight_natural_0 device* lights_6 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_6 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_6 [[texture(6)]], GpuProbe_natural_0 device* probes_6 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_6 [[texture(7)]])
{

#line 5836
    thread KernelContext_0 kernelContext_37;

#line 5836
    (&kernelContext_37)->draw_0 = draw_6;

#line 5836
    (&kernelContext_37)->visible_instances_0 = visible_instances_6;

#line 5836
    (&kernelContext_37)->instances_0 = instances_6;

#line 5836
    (&kernelContext_37)->meshes_0 = meshes_6;

#line 5836
    (&kernelContext_37)->frame_0 = frame_11;

#line 5836
    (&kernelContext_37)->vertices_0 = vertices_6;

#line 5836
    (&kernelContext_37)->ambient_occlusion_0 = ambient_occlusion_6;

#line 5836
    (&kernelContext_37)->materials_0 = materials_6;

#line 5836
    (&kernelContext_37)->base_color_textures_0 = base_color_textures_6;

#line 5836
    (&kernelContext_37)->base_color_sampler_0 = base_color_sampler_6;

#line 5836
    (&kernelContext_37)->normal_textures_0 = normal_textures_6;

#line 5836
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_6;

#line 5836
    (&kernelContext_37)->specular_dfg_0 = specular_dfg_6;

#line 5836
    (&kernelContext_37)->lights_0 = lights_6;

#line 5836
    (&kernelContext_37)->ltc_matrix_0 = ltc_matrix_6;

#line 5836
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_6;

#line 5836
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_6;

#line 5836
    (&kernelContext_37)->contact_shadow_0 = contact_shadow_6;

#line 5836
    (&kernelContext_37)->probes_0 = probes_6;

#line 5836
    (&kernelContext_37)->probe_visibility_0 = probe_visibility_6;

#line 5836
    GpuInstance_natural_0 device* _S322 = instances_6+visible_instances_6[draw_6->base_0 + instance_id_1];

#line 1886
    GpuMesh_0 mesh_3 = meshes_6[draw_6->mesh_0];

#line 1894
    bool _S323 = ((_S322->flags_0) & 2U) != 0U;

#line 1894
    uint base_vertex_3;
    if(_S323)
    {

#line 1895
        base_vertex_3 = _S322->base_vertex_0;

#line 1895
    }
    else
    {

#line 1895
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1895
    }

#line 1895
    MeshVertex_0 _S324 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_37);

#line 1895
    uint previous_base_0;

#line 1908
    if(_S323)
    {

#line 1908
        previous_base_0 = _S322->previous_base_vertex_0;

#line 1908
    }
    else
    {

#line 1908
        previous_base_0 = base_vertex_3;

#line 1908
    }

#line 1908
    float3 _S325 = load_position_0(index_8 + previous_base_0, &kernelContext_37);

#line 1908
    matrix<float,int(4),int(4)>  _S326 = matrix<float,int(4),int(4)> (_S322->transform_0.data_0[int(0)][int(0)], _S322->transform_0.data_0[int(1)][int(0)], _S322->transform_0.data_0[int(2)][int(0)], _S322->transform_0.data_0[int(3)][int(0)], _S322->transform_0.data_0[int(0)][int(1)], _S322->transform_0.data_0[int(1)][int(1)], _S322->transform_0.data_0[int(2)][int(1)], _S322->transform_0.data_0[int(3)][int(1)], _S322->transform_0.data_0[int(0)][int(2)], _S322->transform_0.data_0[int(1)][int(2)], _S322->transform_0.data_0[int(2)][int(2)], _S322->transform_0.data_0[int(3)][int(2)], _S322->transform_0.data_0[int(0)][int(3)], _S322->transform_0.data_0[int(1)][int(3)], _S322->transform_0.data_0[int(2)][int(3)], _S322->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S324.position_1, 1.0f)) * (_S326)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 1922
    matrix<float,int(3),int(3)>  _S327 = matrix<float,int(3),int(3)> (_S326[int(0)].xyz, _S326[int(1)].xyz, _S326[int(2)].xyz);

#line 1922
    (&output_3)->world_normal_0 = (((_S324.basis_1.normal_0) * (normal_basis_0(_S327))));

#line 1928
    (&output_3)->world_tangent_0 = (((_S324.basis_1.tangent_1) * (_S327)));

#line 1928
    thread TangentFrame_0 _S328 = _S324.basis_1;

#line 1928
    uint _S329 = frame_word_0(mesh_3.flags_1, &_S328);
    (&output_3)->frame_3 = _S329;

#line 1929
    float4 _S330;

#line 1936
    if(((&kernelContext_37)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1936
        _S330 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1936
    }
    else
    {

#line 1936
        _S330 = _S324.color_1;

#line 1936
    }

#line 1935
    (&output_3)->color_2 = _S330;

#line 1942
    (&output_3)->material_4 = _S322->material_0;
    (&output_3)->uv_0 = _S324.uv0_0;

#line 1949
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S325, 1.0f)) * (matrix<float,int(4),int(4)> (_S322->previous_transform_0.data_0[int(0)][int(0)], _S322->previous_transform_0.data_0[int(1)][int(0)], _S322->previous_transform_0.data_0[int(2)][int(0)], _S322->previous_transform_0.data_0[int(3)][int(0)], _S322->previous_transform_0.data_0[int(0)][int(1)], _S322->previous_transform_0.data_0[int(1)][int(1)], _S322->previous_transform_0.data_0[int(2)][int(1)], _S322->previous_transform_0.data_0[int(3)][int(1)], _S322->previous_transform_0.data_0[int(0)][int(2)], _S322->previous_transform_0.data_0[int(1)][int(2)], _S322->previous_transform_0.data_0[int(2)][int(2)], _S322->previous_transform_0.data_0[int(3)][int(2)], _S322->previous_transform_0.data_0[int(0)][int(3)], _S322->previous_transform_0.data_0[int(1)][int(3)], _S322->previous_transform_0.data_0[int(2)][int(3)], _S322->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S331 = output_3;

#line 1953
    thread vertexMain_Result_0 _S332;

#line 1953
    (&_S332)->position_8 = _S331.position_3;

#line 1953
    (&_S332)->world_position_18 = _S331.world_position_1;

#line 1953
    (&_S332)->world_normal_4 = _S331.world_normal_0;

#line 1953
    (&_S332)->color_6 = _S331.color_2;

#line 1953
    (&_S332)->material_9 = _S331.material_4;

#line 1953
    (&_S332)->uv_8 = _S331.uv_0;

#line 1953
    (&_S332)->clip_position_4 = _S331.clip_position_0;

#line 1953
    (&_S332)->previous_clip_position_4 = _S331.previous_clip_position_0;

#line 1953
    (&_S332)->world_tangent_4 = _S331.world_tangent_0;

#line 1953
    (&_S332)->frame_10 = _S331.frame_3;

#line 1953
    return _S332;
}

