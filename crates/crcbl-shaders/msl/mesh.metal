#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2362 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2357
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2629
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2689
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2842
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2704
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2732
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1087
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1666
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1666
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


#line 756
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


#line 1672
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1672
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 335 "shaders/mesh.slang"
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
    float4 probe_origin_0;
    float4 probe_inv_spacing_0;
    uint4 probe_counts_0;
    float4 lod_params_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sky_sh_r_0;
    float4 sky_sh_g_0;
    float4 sky_sh_b_0;
    _MatrixStorage_float4x4_ColMajornatural_1 previous_view_proj_0;
    uint4 vertex_pool_0;
    array<float4, int(16)> shadow_atlas_rect_0;
};


#line 335
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


#line 335
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


#line 335
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 335
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
    texture2d_array<float, access::sample> normal_textures_0;
    sampler base_color_sampler_0;
    texture2d_array<float, access::sample> base_color_textures_0;
    uint device* cluster_lights_0;
    texture2d<float, access::sample> specular_dfg_0;
    GpuLight_natural_0 device* lights_0;
    texture2d<float, access::sample> ltc_matrix_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> contact_shadow_0;
    GpuProbe_natural_0 device* probes_0;
};


#line 1130
float3 load_position_0(uint at_0, KernelContext_0 thread* kernelContext_0)
{
    uint word_0 = at_0 * 3U;
    return float3((as_type<float>((kernelContext_0->vertices_0[word_0]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 1U]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 2U]))));
}


#line 178
float dequantise_snorm_0(int lane_0)
{
    return max(float(lane_0) / 32767.0f, -1.0f);
}


float4 unpack_snorm16x4_0(uint low_0, uint high_0)
{
    return float4(dequantise_snorm_0((as_type<int>((low_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((low_0))) >> 16U), dequantise_snorm_0((as_type<int>((high_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((high_0))) >> 16U));
}


#line 210
float3 rotate_by_0(float4 q_0, float3 v_0)
{
    float3 _S1 = q_0.xyz;

#line 212
    float3 t_0 = float3(2.0f)  * cross(_S1, v_0);
    return v_0 + float3(q_0.w)  * t_0 + cross(_S1, t_0);
}


#line 168
struct TangentFrame_0
{
    float3 tangent_1;
    float3 bitangent_0;
    float3 normal_0;
};


#line 224
TangentFrame_0 decode_qtangent_0(float4 lanes_0)
{
    float4 q_1 = normalize(lanes_0);
    thread TangentFrame_0 basis_0;
    float3 _S2 = rotate_by_0(q_1, float3(1.0f, 0.0f, 0.0f));

#line 228
    (&basis_0)->tangent_1 = _S2;
    float3 _S3 = rotate_by_0(q_1, float3(0.0f, 0.0f, 1.0f));

#line 229
    (&basis_0)->normal_0 = _S3;
    float3 _S4 = cross(_S3, _S2);

#line 230
    float _S5;

#line 230
    if((lanes_0.w) < 0.0f)
    {

#line 230
        _S5 = -1.0f;

#line 230
    }
    else
    {

#line 230
        _S5 = 1.0f;

#line 230
    }

#line 230
    (&basis_0)->bitangent_0 = _S4 * float3(_S5) ;
    return basis_0;
}


#line 193
float2 unpack_unorm16x2_0(uint word_1)
{
    return float2(float(word_1 & 65535U), float(word_1 >> 16U)) / float2(65535.0f) ;
}


float4 unpack_rgba8_0(uint word_2)
{
    return float4(float(word_2 & 255U), float((word_2 >> 8U) & 255U), float((word_2 >> 16U) & 255U), float(word_2 >> 24U)) / float4(255.0f) ;
}


#line 239
struct MeshVertex_0
{
    float3 position_1;
    TangentFrame_0 basis_1;
    float2 uv0_0;
    float4 color_1;
};


#line 1141
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1144
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1530
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1653
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1653
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1655
        word_4 = 1U;

#line 1655
    }
    else
    {

#line 1655
        word_4 = 0U;

#line 1655
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1659
        word_4 = word_4 | 2U;

#line 1659
    }

#line 1658
    return word_4;
}


#line 1658
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1773
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 1773
    thread KernelContext_0 kernelContext_2;

#line 1773
    (&kernelContext_2)->draw_0 = draw_1;

#line 1773
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1773
    (&kernelContext_2)->instances_0 = instances_1;

#line 1773
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1773
    (&kernelContext_2)->frame_0 = frame_1;

#line 1773
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1773
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1773
    (&kernelContext_2)->materials_0 = materials_1;

#line 1773
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1773
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1773
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1773
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1773
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1773
    (&kernelContext_2)->lights_0 = lights_1;

#line 1773
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1773
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1773
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1773
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 1773
    (&kernelContext_2)->probes_0 = probes_1;

#line 1773
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1776
    uint base_vertex_2;

#line 1782
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1782
        base_vertex_2 = _S7->base_vertex_0;

#line 1782
    }
    else
    {

#line 1782
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1782
    }

#line 1782
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1782
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1782
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 1785
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 1806
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1806
    thread KernelContext_0 kernelContext_3;

#line 1806
    (&kernelContext_3)->draw_0 = draw_2;

#line 1806
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 1806
    (&kernelContext_3)->instances_0 = instances_2;

#line 1806
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 1806
    (&kernelContext_3)->frame_0 = frame_2;

#line 1806
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 1806
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1806
    (&kernelContext_3)->materials_0 = materials_2;

#line 1806
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 1806
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 1806
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 1806
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 1806
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 1806
    (&kernelContext_3)->lights_0 = lights_2;

#line 1806
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 1806
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 1806
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 1806
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 1806
    (&kernelContext_3)->probes_0 = probes_2;

#line 1806
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 4162
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 4164
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4058
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4058
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4064
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)).x);
}


#line 3808
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 3812
    float _S16 = axis_0.y;

#line 3812
    bool _S17;

#line 3812
    if(_S15 >= _S16)
    {

#line 3812
        _S17 = _S15 >= (axis_0.z);

#line 3812
    }
    else
    {

#line 3812
        _S17 = false;

#line 3812
    }

#line 3812
    float2 planar_0;

#line 3812
    if(_S17)
    {

#line 3812
        planar_0 = world_position_0.zy;

#line 3812
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 3816
            planar_0 = world_position_0.xz;

#line 3816
        }
        else
        {

#line 3816
            planar_0 = world_position_0.xy;

#line 3816
        }

#line 3812
    }

#line 3824
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 941
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 3845
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S18 = normal_2.z;

#line 3847
    float sign_z_0;

#line 3847
    if(_S18 >= 0.0f)
    {

#line 3847
        sign_z_0 = 1.0f;

#line 3847
    }
    else
    {

#line 3847
        sign_z_0 = -1.0f;

#line 3847
    }
    float a_0 = -1.0f / (sign_z_0 + _S18);
    float _S19 = normal_2.x;

#line 3849
    float _S20 = sign_z_0 * _S19;

#line 3849
    return float3(1.0f + _S20 * _S19 * a_0, _S20 * normal_2.y * a_0, - sign_z_0 * _S19);
}


#line 3899
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S21 = duvdy_0.y;

#line 3901
    float _S22 = duvdx_0.y;

#line 3901
    float winding_0;
    if((duvdx_0.x * _S21 - duvdy_0.x * _S22) < 0.0f)
    {

#line 3902
        winding_0 = -1.0f;

#line 3902
    }
    else
    {

#line 3902
        winding_0 = 1.0f;

#line 3902
    }
    float3 tangent_2 = (float3(_S21)  * dpdx_0 - float3(_S22)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 3911
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 3912
    float3 _S23;

#line 3921
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 3921
        _S23 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 3921
    }
    else
    {

#line 3921
        _S23 = orthonormal_tangent_0(normal_3);

#line 3921
    }

#line 3921
    (&basis_4)->tangent_1 = _S23;

    (&basis_4)->bitangent_0 = cross(normal_3, _S23);
    return basis_4;
}


#line 1537
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_1;
    float3 world_normal_0;
    float4 color_2;
    [[flat]] uint material_2;
    float2 uv_0;
    float4 clip_position_0;
    float4 previous_clip_position_0;
    float3 world_tangent_0;
    [[flat]] uint frame_3;
};


#line 3981
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 3993
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 4003
    uint _S24 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4012
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4014
        float3 _S25;

#line 4019
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4019
            _S25 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4019
        }
        else
        {

#line 4019
            _S25 = orthonormal_tangent_0(normal_4);

#line 4019
        }

#line 4019
        (&basis_5)->tangent_1 = _S25;

#line 4025
        float3 _S26 = cross((&basis_5)->normal_0, _S25);

#line 4025
        float _S27;
        if((_S24 & 2U) != 0U)
        {

#line 4026
            _S27 = -1.0f;

#line 4026
        }
        else
        {

#line 4026
            _S27 = 1.0f;

#line 4026
        }

#line 4025
        (&basis_5)->bitangent_0 = _S26 * float3(_S27) ;

#line 4004
    }
    else
    {

#line 4030
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 4004
    }

#line 4034
    float3 _S28 = float3(uv_1, float(layer_0));
    float3 _S29 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S28)).xy, uint(((_S28)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4035
    thread float3 tangent_space_0 = _S29;
    tangent_space_0.xy = _S29.xy * float2(normal_scale_1) ;

#line 4041
    float3 _S30 = normalize(tangent_space_0);

#line 4041
    tangent_space_0 = _S30;
    return normalize(float3(_S30.x)  * (&basis_5)->tangent_1 + float3(_S30.y)  * (&basis_5)->bitangent_0 + float3(_S30.z)  * (&basis_5)->normal_0);
}


#line 2497
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2508
    float3 _S31;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2509
        _S31 = - facet_1;

#line 2509
    }
    else
    {

#line 2509
        _S31 = facet_1;

#line 2509
    }

#line 2509
    return _S31;
}


#line 926
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3606
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S32 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S33 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S34 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 3616
    uint _S36 = uint(pixel_0.x) / _S35;

#line 3616
    uint _S37 = min(_S36, _S32 - 1U);
    uint _S38 = uint(pixel_0.y) / _S35;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3627
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S34 - 1U))) * _S33 + min(_S38, _S33 - 1U)) * _S32 + _S37;
}


#line 1929
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 1950
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 1950
    texture2d<float, access::sample> _S39 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S39).get_width(0)),(*((&height_1)) = (_S39).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1956
    float2 _S40 = float2(1.0f) ;
    float2 _S41 = extent_1 - _S40;

#line 1957
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S41);
    float2 high_1 = min(low_1 + _S40, _S41);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 1975
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 1987
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S42 = tap_1->lo_0.x;

#line 1989
    int _S43 = tap_1->lo_0.y;

#line 1989
    int3 _S44 = int3(_S42, _S43, int(0));
    int _S45 = tap_1->hi_0.x;

#line 1990
    int3 _S46 = int3(_S45, _S43, int(0));
    float2 _S47 = float2(tap_1->weight_0.x) ;
    int _S48 = tap_1->hi_0.y;

#line 1992
    int3 _S49 = int3(_S42, _S48, int(0));
    int3 _S50 = int3(_S45, _S48, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)))), _S47), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), _S47), float2(tap_1->weight_0.y) );
}


#line 3557
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3573
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3585
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3592
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2316
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2316
    float4 _S51 = float4(light_0->tangent_0) ;

    float3 _S52 = _S51.xyz;

#line 2318
    float3 across_0 = _S52 * float3(_S51.w) ;

#line 2318
    float4 _S53 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S52, _S53.xyz) * float3(_S53.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S54 = centre_0 - across_0;

#line 2321
    (*corners_0)[int(0)] = _S54 - down_0;
    float3 _S55 = centre_0 + across_0;

#line 2322
    (*corners_0)[int(1)] = _S55 - down_0;
    (*corners_0)[int(2)] = _S55 + down_0;
    (*corners_0)[int(3)] = _S54 + down_0;
    return;
}


#line 2074
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2077
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2078
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2078
    }
    else
    {

#line 2078
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2078
    }

#line 2078
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2079
        tangent_5 = across_1 / float3(span_0) ;

#line 2079
    }
    else
    {

#line 2079
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2079
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 2055
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2145
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2145
    float3 _S56 = polygon_0->corner_0[int(0)];

#line 2145
    float3 _S57 = polygon_0->corner_0[int(1)];

#line 2145
    float3 _S58 = polygon_0->corner_0[int(2)];

#line 2145
    float3 _S59 = polygon_0->corner_0[int(3)];

#line 2151
    float3 _S60 = float3(0.0f, 0.0f, 0.0f);


    float _S61 = polygon_0->corner_0[int(0)].z;

#line 2154
    int count_1;

#line 2154
    if(_S61 > 0.0f)
    {

#line 2154
        count_1 = int(1);

#line 2154
    }
    else
    {

#line 2154
        count_1 = int(0);

#line 2154
    }
    float _S62 = _S57.z;

#line 2155
    int _S63;

#line 2155
    if(_S62 > 0.0f)
    {

#line 2155
        _S63 = int(2);

#line 2155
    }
    else
    {

#line 2155
        _S63 = int(0);

#line 2155
    }

#line 2155
    int config_0 = count_1 + _S63;
    float _S64 = _S58.z;

#line 2156
    if(_S64 > 0.0f)
    {

#line 2156
        count_1 = int(4);

#line 2156
    }
    else
    {

#line 2156
        count_1 = int(0);

#line 2156
    }

#line 2156
    int config_1 = config_0 + count_1;
    float _S65 = _S59.z;

#line 2157
    if(_S65 > 0.0f)
    {

#line 2157
        count_1 = int(8);

#line 2157
    }
    else
    {

#line 2157
        count_1 = int(0);

#line 2157
    }

#line 2157
    int config_2 = config_1 + count_1;

#line 2157
    float3 l0_0;

#line 2157
    float3 l1_0;

#line 2157
    float3 l2_0;

#line 2157
    float3 l3_0;

#line 2157
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2160
        float3 _S66 = float3(_S61) ;


        float3 _S67 = float3(- _S62)  * _S56 + _S66 * _S57;
        float3 _S68 = float3(- _S65)  * _S56 + _S66 * _S59;

#line 2164
        count_1 = int(3);

#line 2164
        l0_0 = _S56;

#line 2164
        l1_0 = _S67;

#line 2164
        l2_0 = _S68;

#line 2164
        l3_0 = _S59;

#line 2164
        l4_0 = _S60;

#line 2160
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2166
            float3 _S69 = float3(_S62) ;


            float3 _S70 = float3(- _S61)  * _S57 + _S69 * _S56;
            float3 _S71 = float3(- _S64)  * _S57 + _S69 * _S58;

#line 2170
            count_1 = int(3);

#line 2170
            l0_0 = _S70;

#line 2170
            l1_0 = _S57;

#line 2170
            l2_0 = _S71;

#line 2170
            l3_0 = _S59;

#line 2170
            l4_0 = _S60;

#line 2166
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S72 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                float3 _S73 = float3(- _S65)  * _S56 + float3(_S61)  * _S59;

#line 2176
                count_1 = int(4);

#line 2176
                l0_0 = _S56;

#line 2176
                l1_0 = _S57;

#line 2176
                l2_0 = _S72;

#line 2176
                l3_0 = _S73;

#line 2176
                l4_0 = _S60;

#line 2172
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2178
                    float3 _S74 = float3(_S64) ;


                    float3 _S75 = float3(- _S65)  * _S58 + _S74 * _S59;
                    float3 _S76 = float3(- _S62)  * _S58 + _S74 * _S57;

#line 2182
                    count_1 = int(3);

#line 2182
                    l0_0 = _S75;

#line 2182
                    l1_0 = _S76;

#line 2182
                    l2_0 = _S58;

#line 2182
                    l3_0 = _S59;

#line 2182
                    l4_0 = _S60;

#line 2178
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S77 = float3(- _S61)  * _S57 + float3(_S62)  * _S56;
                        float3 _S78 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;

#line 2188
                        count_1 = int(4);

#line 2188
                        l0_0 = _S77;

#line 2188
                        l1_0 = _S57;

#line 2188
                        l2_0 = _S58;

#line 2188
                        l3_0 = _S78;

#line 2188
                        l4_0 = _S60;

#line 2184
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2190
                            float3 _S79 = float3(- _S65) ;


                            float3 _S80 = _S79 * _S56 + float3(_S61)  * _S59;
                            float3 _S81 = _S79 * _S58 + float3(_S64)  * _S59;

#line 2194
                            count_1 = int(5);

#line 2194
                            l0_0 = _S56;

#line 2194
                            l1_0 = _S57;

#line 2194
                            l2_0 = _S58;

#line 2194
                            l3_0 = _S81;

#line 2194
                            l4_0 = _S80;

#line 2190
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2196
                                float3 _S82 = float3(_S65) ;


                                float3 _S83 = float3(- _S61)  * _S59 + _S82 * _S56;
                                float3 _S84 = float3(- _S64)  * _S59 + _S82 * _S58;

#line 2200
                                count_1 = int(3);

#line 2200
                                l0_0 = _S83;

#line 2200
                                l1_0 = _S84;

#line 2200
                                l2_0 = _S59;

#line 2200
                                l3_0 = _S59;

#line 2200
                                l4_0 = _S60;

#line 2196
                            }
                            else
                            {

#line 2203
                                if(config_2 == int(9))
                                {

                                    float3 _S85 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;
                                    float3 _S86 = float3(- _S64)  * _S59 + float3(_S65)  * _S58;

#line 2207
                                    count_1 = int(4);

#line 2207
                                    l0_0 = _S56;

#line 2207
                                    l1_0 = _S85;

#line 2207
                                    l2_0 = _S86;

#line 2207
                                    l3_0 = _S59;

#line 2207
                                    l4_0 = _S60;

#line 2203
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S87 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;
                                        float3 _S88 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;

#line 2214
                                        count_1 = int(5);

#line 2214
                                        l0_0 = _S56;

#line 2214
                                        l1_0 = _S57;

#line 2214
                                        l2_0 = _S88;

#line 2214
                                        l3_0 = _S87;

#line 2214
                                        l4_0 = _S59;

#line 2209
                                    }
                                    else
                                    {

#line 2216
                                        if(config_2 == int(12))
                                        {

                                            float3 _S89 = float3(- _S62)  * _S58 + float3(_S64)  * _S57;
                                            float3 _S90 = float3(- _S61)  * _S59 + float3(_S65)  * _S56;

#line 2220
                                            count_1 = int(4);

#line 2220
                                            l0_0 = _S90;

#line 2220
                                            l1_0 = _S89;

#line 2220
                                            l2_0 = _S58;

#line 2220
                                            l3_0 = _S59;

#line 2220
                                            l4_0 = _S60;

#line 2216
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S91 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                                                float3 _S92 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;

#line 2228
                                                count_1 = int(5);

#line 2228
                                                l0_0 = _S56;

#line 2228
                                                l1_0 = _S92;

#line 2228
                                                l2_0 = _S91;

#line 2228
                                                l3_0 = _S58;

#line 2228
                                                l4_0 = _S59;

#line 2222
                                            }
                                            else
                                            {

#line 2230
                                                if(config_2 == int(14))
                                                {

#line 2230
                                                    float3 _S93 = float3(- _S61) ;


                                                    float3 _S94 = _S93 * _S59 + float3(_S65)  * _S56;
                                                    float3 _S95 = _S93 * _S57 + float3(_S62)  * _S56;

#line 2234
                                                    count_1 = int(5);

#line 2234
                                                    l0_0 = _S95;

#line 2234
                                                    l1_0 = _S94;

#line 2230
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2236
                                                        count_1 = int(4);

#line 2236
                                                    }
                                                    else
                                                    {

#line 2236
                                                        count_1 = int(0);

#line 2236
                                                    }

#line 2236
                                                    l0_0 = _S56;

#line 2236
                                                    l1_0 = _S60;

#line 2230
                                                }

#line 2151
                                                float3 _S96 = l1_0;

#line 2151
                                                l1_0 = _S57;

#line 2151
                                                l2_0 = _S58;

#line 2151
                                                l3_0 = _S59;

#line 2151
                                                l4_0 = _S96;

#line 2222
                                            }

#line 2216
                                        }

#line 2209
                                    }

#line 2203
                                }

#line 2196
                            }

#line 2190
                        }

#line 2184
                    }

#line 2178
                }

#line 2172
            }

#line 2166
        }

#line 2160
    }

#line 2244
    if(count_1 <= int(3))
    {

#line 2244
        l3_0 = l0_0;

#line 2244
        l4_0 = l0_0;

#line 2244
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2249
            l4_0 = l0_0;

#line 2249
        }

#line 2244
    }

#line 2254
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2117
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2123
    float weight_1;

#line 2128
    if(cosine_0 > 0.0f)
    {

#line 2128
        weight_1 = fit_0;

#line 2128
    }
    else
    {

#line 2128
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2128
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2274
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2276
    int corner_1 = int(0);
    for(;;)
    {

#line 2277
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2277
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2277
        corner_1 = corner_1 + int(1);

#line 2277
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2282
    thread LtcPolygon_0 _S97 = polygon_1;

#line 2282
    LtcPolygon_0 _S98 = ltc_clip_0(&_S97);
    polygon_1 = _S98;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2286
    int at_2 = int(0);

    for(;;)
    {

#line 2288
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2288
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2288
        at_2 = at_2 + int(1);

#line 2288
    }

#line 2295
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2295
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2296
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2296
    }
    else
    {

#line 2296
        sum_1 = sum_0;

#line 2296
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2300
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2300
    }

#line 2307
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2003
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S99 = tap_2->lo_0.x;

#line 2005
    int _S100 = tap_2->lo_0.y;

#line 2005
    int3 _S101 = int3(_S99, _S100, int(0));
    int _S102 = tap_2->hi_0.x;

#line 2006
    int3 _S103 = int3(_S102, _S100, int(0));
    float4 _S104 = float4(tap_2->weight_0.x) ;
    int _S105 = tap_2->hi_0.y;

#line 2008
    int3 _S106 = int3(_S99, _S105, int(0));
    int3 _S107 = int3(_S102, _S105, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S101)).xy), uint(((_S101)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S103)).xy), uint(((_S103)).z))), _S104), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S107)).xy), uint(((_S107)).z))), _S104), float4(tap_2->weight_0.y) );
}


#line 2090
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 1885
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 1892
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1899
    float _S108 = 1.0f - alpha2_0;

#line 1904
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S108 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S108 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2877
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 2877
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 2937
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 2909
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2548
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 2864
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 2889
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 2889
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 321
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3059
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_15)
{
    float2 texel_1 = kernelContext_15->frame_0->shadow_params_0.xy;

#line 3061
    float4 _S109 = atlas_rect_0(cascade_0, kernelContext_15);

#line 3061
    float2 _S110 = atlas_step_0(_S109, kernelContext_15);


    float2 _S111 = float2(0.5f, 0.5f) * _S110;


    float2 _S112 = float2(1.0f, 1.0f);

#line 3067
    float2 _S113 = _S112 / texel_1;

#line 3067
    uint index_2 = 0U;

#line 3067
    float sum_2 = 0.0f;

#line 3067
    float found_0 = 0.0f;



    for(;;)
    {

#line 3071
        if(index_2 < 16U)
        {
        }
        else
        {

#line 3071
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_2] * float2(8.0f) ;
        float _S114 = spoke_0.x;

#line 3074
        float _S115 = rotation_0.x;

#line 3074
        float _S116 = spoke_0.y;

#line 3074
        float _S117 = rotation_0.y;

#line 3082
        int3 _S118 = int3(int2(min(atlas_uv_0(_S109, clamp(tile_uv_1 + float2(_S114 * _S115 - _S116 * _S117, _S114 * _S117 + _S116 * _S115) * _S110, _S111, float2(1.0f)  - _S111)) * _S113, _S113 - _S112)), int(0));

#line 3082
        float depth_1 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 3086
            sum_2 = sum_2 + depth_1;

#line 3086
            found_0 = found_1;

#line 3083
        }

#line 3071
        index_2 = index_2 + 1U;

#line 3071
    }

#line 3090
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3101
    float _S119 = 2.0f * kernelContext_15->frame_0->cascade_far_0[cascade_0];

#line 3101
    float separation_0 = (sum_2 / found_0 - reference_0) * (_S119 + 40.0f);

#line 3101
    float _S120 = tile_texels_0(_S109, kernelContext_15);

    return clamp(separation_0 * 0.01999999955296516f / (_S119 / _S120), 2.0f, 8.0f);
}


#line 2959
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S121 = spoke_1.x;

#line 2964
    float _S122 = rotation_1.x;

#line 2964
    float _S123 = spoke_1.y;

#line 2964
    float _S124 = rotation_1.y;


    float _S125 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_2 + float2(_S121 * _S122 - _S123 * _S124, _S121 * _S124 + _S123 * _S122) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2967
    return _S125;
}


#line 2989
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_2, KernelContext_0 thread* kernelContext_17)
{
    float2 _S126 = shadow_rotation_0(pixel_2);

#line 2991
    float4 _S127 = atlas_rect_1(tile_2, kernelContext_17);

    if(atlas_rect_is_empty_0(_S127))
    {
        return 1.0f;
    }

#line 2995
    float2 _S128 = atlas_step_1(_S127, kernelContext_17);

#line 2995
    uint spot_0 = 0U;

#line 2995
    float probe_0 = 0.0f;

#line 3000
    for(;;)
    {

#line 3000
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3000
            break;
        }

#line 3000
        float _S129 = tile_tap_0(_S127, _S128, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S126, reference_2, kernelContext_17);

        float probe_1 = probe_0 + _S129;

#line 3000
        spot_0 = spot_0 + 1U;

#line 3000
        probe_0 = probe_1;

#line 3000
    }

#line 3009
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3015
    uint index_3 = 0U;

#line 3015
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 3019
        if(index_3 < 32U)
        {
        }
        else
        {

#line 3019
            break;
        }

#line 3019
        float _S130 = tile_tap_0(_S127, _S128, tile_uv_3, SHADOW_DISC_0[index_3] * float2(radius_2) , _S126, reference_2, kernelContext_17);

        float visibility_1 = visibility_0 + _S130;

#line 3019
        index_3 = index_3 + 1U;

#line 3019
        visibility_0 = visibility_1;

#line 3019
    }

#line 3024
    return visibility_0 / 32.0f;
}


#line 3155
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_18)
{

#line 3156
    float4 _S131 = atlas_rect_0(cascade_1, kernelContext_18);

#line 3190
    if(atlas_rect_is_empty_0(_S131))
    {


        return 1.0f;
    }
    float _S132 = 2.0f * kernelContext_18->frame_0->cascade_far_0[cascade_1];

#line 3196
    float _S133 = tile_texels_0(_S131, kernelContext_18);

#line 3196
    float texel_world_0 = _S132 / _S133;

#line 3203
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_18->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_18->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3207
    bool _S134;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3208
        _S134 = true;

#line 3208
    }
    else
    {

#line 3208
        _S134 = (ndc_0.z) <= 0.0f;

#line 3208
    }

#line 3208
    if(_S134)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3235
    float _S135 = ndc_0.z;

#line 3235
    float _S136 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S135, shadow_rotation_0(pixel_3), kernelContext_18);

#line 3235
    float _S137 = tile_pcf_0(cascade_1, tile_uv_4, _S135, pixel_3, _S136, kernelContext_18);
    return _S137;
}


#line 3252
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_19)
{

#line 3253
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3265
    float eye_distance_0 = length(world_position_5 - kernelContext_19->frame_0->camera_position_0.xyz);

#line 3265
    uint index_4 = 0U;

    for(;;)
    {

#line 3267
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3267
            cascade_2 = 1U;

#line 3267
            break;
        }
        if(eye_distance_0 < kernelContext_19->frame_0->cascade_far_0[index_4])
        {

#line 3269
            cascade_2 = index_4;


            break;
        }

#line 3267
        index_4 = index_4 + 1U;

#line 3267
    }

#line 3267
    float _S138 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_19);

#line 3278
    uint _S139 = cascade_2 + 1U;

#line 3278
    if(_S139 >= 2U)
    {



        return _S138;
    }

#line 3291
    float band_0 = kernelContext_19->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_19->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S138;
    }

#line 3295
    float _S140 = cascade_visibility_0(_S139, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_19);

#line 3306
    return mix(_S138, _S140, blend_0);
}


#line 4074
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_20)
{

#line 4074
    texture2d<float, access::sample> _S141 = kernelContext_20->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S141).get_width(0)),(*((&height_2)) = (_S141).get_height(0));

    int3 _S142 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 4080
    return ((kernelContext_20->contact_shadow_0).read(vec<uint,2>(((_S142)).xy), uint(((_S142)).z)).x);
}


#line 3509
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S143 = axis_2.x;

#line 3512
    float _S144 = axis_2.y;

#line 3512
    bool _S145;

#line 3512
    if(_S143 >= _S144)
    {

#line 3512
        _S145 = _S143 >= (axis_2.z);

#line 3512
    }
    else
    {

#line 3512
        _S145 = false;

#line 3512
    }

#line 3512
    uint _S146;

#line 3512
    if(_S145)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3514
            _S146 = 0U;

#line 3514
        }
        else
        {

#line 3514
            _S146 = 1U;

#line 3514
        }

#line 3514
        return _S146;
    }
    if(_S144 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3518
            _S146 = 2U;

#line 3518
        }
        else
        {

#line 3518
            _S146 = 3U;

#line 3518
        }

#line 3518
        return _S146;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3520
        _S146 = 4U;

#line 3520
    }
    else
    {

#line 3520
        _S146 = 5U;

#line 3520
    }

#line 3520
    return _S146;
}


#line 308
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 3413
float punctual_visibility_0(uint tile_4, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_21)
{

    uint atlas_0 = light_tile_0(tile_4);

#line 3416
    float4 _S147 = atlas_rect_0(atlas_0, kernelContext_21);

    if(atlas_rect_is_empty_0(_S147))
    {


        return 1.0f;
    }

#line 3422
    float _S148 = tile_texels_0(_S147, kernelContext_21);

    float texel_world_1 = map_world_0 / _S148;

#line 3434
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 3441
    float _S149 = clip_1.w;

#line 3441
    if(_S149 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S149) ;

#line 3445
    bool _S150;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3446
        _S150 = true;

#line 3446
    }
    else
    {

#line 3446
        _S150 = (ndc_1.z) <= 0.0f;

#line 3446
    }

#line 3446
    if(_S150)
    {

#line 3446
        _S150 = true;

#line 3446
    }
    else
    {

#line 3446
        _S150 = (ndc_1.z) > 1.0f;

#line 3446
    }

#line 3446
    if(_S150)
    {

#line 3453
        return 1.0f;
    }

#line 3453
    float _S151 = tile_pcf_0(atlas_0, float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_21);

#line 3463
    return _S151;
}


#line 3528
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_22)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3536
    float _S152 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_6, kernelContext_22);

#line 3542
    return _S152;
}


#line 3470
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_5, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_23)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3477
    float4 _S153 = float4(light_2->direction_0) ;

#line 3484
    float cos_outer_1 = _S153.w;

#line 3484
    float _S154 = punctual_visibility_0(tile_5, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S153.xyz)), 0.0f), geometric_normal_5, pixel_7, kernelContext_23);

#line 3491
    return _S154;
}


#line 2031
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 3735
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_24)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_24->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_24->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_24->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 978
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 3637
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_25)
{

    GpuProbe_natural_0 _S155 = kernelContext_25->probes_0[min((cell_1.z * kernelContext_25->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_25->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_25->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 3640
    GpuProbe_0 _S156 = { float4(_S155.sh_r_0) , float4(_S155.sh_g_0) , float4(_S155.sh_b_0)  };

#line 3640
    return _S156;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_1, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S157 = float4(t_1) ;

#line 3648
    (&blended_0)->sh_r_0 = mix(a_1->sh_r_0, b_0->sh_r_0, _S157);
    (&blended_0)->sh_g_0 = mix(a_1->sh_g_0, b_0->sh_g_0, _S157);
    (&blended_0)->sh_b_0 = mix(a_1->sh_b_0, b_0->sh_b_0, _S157);
    return blended_0;
}


#line 3688
float3 probe_irradiance_0(float3 world_position_9, float3 normal_7, KernelContext_0 thread* kernelContext_26)
{

#line 3688
    float3 _S158 = float3(1.0f) ;

#line 3693
    float3 _S159 = float3(0.0f, 0.0f, 0.0f);

#line 3693
    float3 last_0 = max(float3(kernelContext_26->frame_0->probe_counts_0.xyz) - _S158, _S159);
    float3 grid_0 = clamp((world_position_9 - kernelContext_26->frame_0->probe_origin_0.xyz) * kernelContext_26->frame_0->probe_inv_spacing_0.xyz, _S159, last_0);

    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S160 = uint3(base_2);



    uint3 _S161 = uint3(min(base_2 + _S158, last_0));

#line 3710
    uint _S162 = _S160.x;

#line 3710
    uint _S163 = _S160.y;

#line 3710
    uint _S164 = _S160.z;

#line 3710
    GpuProbe_0 _S165 = probe_at_0(uint3(_S162, _S163, _S164), kernelContext_26);

#line 3710
    uint _S166 = _S161.x;

#line 3710
    GpuProbe_0 _S167 = probe_at_0(uint3(_S166, _S163, _S164), kernelContext_26);

#line 3710
    float _S168 = f_0.x;

#line 3710
    thread GpuProbe_0 _S169 = _S165;

#line 3710
    thread GpuProbe_0 _S170 = _S167;

#line 3710
    GpuProbe_0 _S171 = lerp_probe_0(&_S169, &_S170, _S168);
    uint _S172 = _S161.y;

#line 3711
    GpuProbe_0 _S173 = probe_at_0(uint3(_S162, _S172, _S164), kernelContext_26);

#line 3711
    GpuProbe_0 _S174 = probe_at_0(uint3(_S166, _S172, _S164), kernelContext_26);

#line 3711
    thread GpuProbe_0 _S175 = _S173;

#line 3711
    thread GpuProbe_0 _S176 = _S174;

#line 3711
    GpuProbe_0 _S177 = lerp_probe_0(&_S175, &_S176, _S168);
    uint _S178 = _S161.z;

#line 3712
    GpuProbe_0 _S179 = probe_at_0(uint3(_S162, _S163, _S178), kernelContext_26);

#line 3712
    GpuProbe_0 _S180 = probe_at_0(uint3(_S166, _S163, _S178), kernelContext_26);

#line 3712
    thread GpuProbe_0 _S181 = _S179;

#line 3712
    thread GpuProbe_0 _S182 = _S180;

#line 3712
    GpuProbe_0 _S183 = lerp_probe_0(&_S181, &_S182, _S168);

#line 3712
    GpuProbe_0 _S184 = probe_at_0(uint3(_S162, _S172, _S178), kernelContext_26);

#line 3712
    GpuProbe_0 _S185 = probe_at_0(uint3(_S166, _S172, _S178), kernelContext_26);

#line 3712
    thread GpuProbe_0 _S186 = _S184;

#line 3712
    thread GpuProbe_0 _S187 = _S185;

#line 3712
    GpuProbe_0 _S188 = lerp_probe_0(&_S186, &_S187, _S168);

    float _S189 = f_0.y;

#line 3714
    thread GpuProbe_0 _S190 = _S171;

#line 3714
    thread GpuProbe_0 _S191 = _S177;

#line 3714
    GpuProbe_0 _S192 = lerp_probe_0(&_S190, &_S191, _S189);

#line 3714
    thread GpuProbe_0 _S193 = _S183;

#line 3714
    thread GpuProbe_0 _S194 = _S188;

#line 3714
    GpuProbe_0 _S195 = lerp_probe_0(&_S193, &_S194, _S189);

    float _S196 = f_0.z;

#line 3716
    thread GpuProbe_0 _S197 = _S192;

#line 3716
    thread GpuProbe_0 _S198 = _S195;

#line 3716
    GpuProbe_0 _S199 = lerp_probe_0(&_S197, &_S198, _S196);

    float4 basis_7 = float4(normal_7, 1.0f);
    return max(float3(dot(_S199.sh_r_0, basis_7), dot(_S199.sh_g_0, basis_7), dot(_S199.sh_b_0, basis_7)), _S159);
}


#line 4130
float3 multi_bounce_occlusion_0(float visibility_2, float3 albedo_0)
{

#line 4130
    float3 _S200 = float3(visibility_2) ;

#line 4136
    return min(float3(1.0f) , max(_S200, ((_S200 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S200 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S200));
}


#line 951
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2382
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S201 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2390
    float kernel_0 = 0.0001984127011383f;

#line 2390
    int term_0 = int(6);

    for(;;)
    {

#line 2392
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2392
            break;
        }
        float _S202 = kernel_0 * _S201 + FOG_KERNEL_0[term_0];

#line 2392
        int term_1 = term_0 - int(1);

#line 2392
        kernel_0 = _S202;

#line 2392
        term_0 = term_1;

#line 2392
    }

#line 2399
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2409
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S203 = - d_0;

#line 2413
        float series_0 = 0.00833333376795053f;

#line 2413
        int term_2 = int(3);

        for(;;)
        {

#line 2415
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2415
                break;
            }
            float _S204 = series_0 * _S203 + FOG_RATIO_KERNEL_0[term_2];

#line 2415
            int term_3 = term_2 - int(1);

#line 2415
            series_0 = _S204;

#line 2415
            term_2 = term_3;

#line 2415
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2443
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2454
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2462
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 3761
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 3761
struct pixelInput_0
{
    float3 world_position_10 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    [[flat]] uint material_5 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
    float3 world_tangent_1 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_4 [[user(TEXCOORD_5)]];
};


#line 4172
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S205 [[stage_in]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]])
{

#line 4172
    thread KernelContext_0 kernelContext_27;

#line 4172
    (&kernelContext_27)->draw_0 = draw_3;

#line 4172
    (&kernelContext_27)->visible_instances_0 = visible_instances_3;

#line 4172
    (&kernelContext_27)->instances_0 = instances_3;

#line 4172
    (&kernelContext_27)->meshes_0 = meshes_3;

#line 4172
    (&kernelContext_27)->frame_0 = frame_5;

#line 4172
    (&kernelContext_27)->vertices_0 = vertices_3;

#line 4172
    (&kernelContext_27)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4172
    (&kernelContext_27)->materials_0 = materials_3;

#line 4172
    (&kernelContext_27)->normal_textures_0 = normal_textures_3;

#line 4172
    (&kernelContext_27)->base_color_sampler_0 = base_color_sampler_3;

#line 4172
    (&kernelContext_27)->base_color_textures_0 = base_color_textures_3;

#line 4172
    (&kernelContext_27)->cluster_lights_0 = cluster_lights_3;

#line 4172
    (&kernelContext_27)->specular_dfg_0 = specular_dfg_3;

#line 4172
    (&kernelContext_27)->lights_0 = lights_3;

#line 4172
    (&kernelContext_27)->ltc_matrix_0 = ltc_matrix_3;

#line 4172
    (&kernelContext_27)->shadow_atlas_0 = shadow_atlas_3;

#line 4172
    (&kernelContext_27)->shadow_sampler_0 = shadow_sampler_3;

#line 4172
    (&kernelContext_27)->contact_shadow_0 = contact_shadow_3;

#line 4172
    (&kernelContext_27)->probes_0 = probes_3;

#line 4184
    float3 vertex_normal_0 = normalize(_S205.world_normal_1);

#line 4189
    float2 motion_1 = motion_vector_0(_S205.clip_position_1, _S205.previous_clip_position_1);

#line 4198
    if((frame_5->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 4240
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 4240
        float _S206 = occlusion_at_0(position_5.xy, &kernelContext_27);

        thread FragmentOutput_0 occlusion_0;

#line 4251
        (&occlusion_0)->lit_0 = float4(_S206, _S206, _S206, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S205.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 4268
    thread GpuMaterial_natural_0 _S207 = (&kernelContext_27)->materials_0[_S205.material_5];

#line 4268
    float2 uv_3;

#line 4293
    if(((&_S207)->tiling_0) == 1U)
    {

#line 4293
        uv_3 = physical_tile_uv_0(_S205.world_position_10, vertex_normal_0, (&_S207)->tile_metres_0);

#line 4293
    }
    else
    {

#line 4293
        uv_3 = _S205.uv_2;

#line 4293
    }

#line 4293
    uint _S208 = normal_layer_0(&_S207);

#line 4293
    thread VertexOutput_0 _S209;

#line 4293
    (&_S209)->position_3 = position_5;

#line 4293
    (&_S209)->world_position_1 = _S205.world_position_10;

#line 4293
    (&_S209)->world_normal_0 = _S205.world_normal_1;

#line 4293
    (&_S209)->color_2 = _S205.color_3;

#line 4293
    (&_S209)->material_2 = _S205.material_5;

#line 4293
    (&_S209)->uv_0 = _S205.uv_2;

#line 4293
    (&_S209)->clip_position_0 = _S205.clip_position_1;

#line 4293
    (&_S209)->previous_clip_position_0 = _S205.previous_clip_position_1;

#line 4293
    (&_S209)->world_tangent_0 = _S205.world_tangent_1;

#line 4293
    (&_S209)->frame_3 = _S205.frame_4;

#line 4293
    float3 _S210 = shading_normal_of_0(_S208, (&_S207)->normal_scale_0, &_S209, vertex_normal_0, uv_3, &kernelContext_27);

#line 4300
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 4302
        float3 _S211 = float3(0.5f) ;

#line 4314
        (&normals_0)->lit_0 = float4(_S210 * _S211 + _S211, 1.0f);

#line 4320
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_27)->frame_0->camera_position_0.xyz - _S205.world_position_10);



    float3 _S212 = geometric_normal_of_0(_S205.world_position_10, vertex_normal_0);

#line 4329
    uint _S213 = base_color_layer_0(&_S207);

#line 4344
    float3 _S214 = float3(uv_3, float(_S213));
    float4 albedo_1 = _S205.color_3 * float4((&_S207)->base_color_0)  * (((&kernelContext_27)->base_color_textures_0).sample(((&kernelContext_27)->base_color_sampler_0), ((_S214)).xy, uint(((_S214)).z)));

#line 4351
    float metallic_1 = saturate((&_S207)->metallic_0);
    float roughness_2 = clamp((&_S207)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S215 = alpha_0 * alpha_0;

#line 4360
    float3 _S216 = albedo_1.xyz;

#line 4360
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S216, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S216 * float3((1.0f - metallic_1)) ;

#line 4367
    float _S217 = max(dot(_S210, to_eye_1), 0.00009999999747379f);

#line 4377
    float2 _S218 = position_5.xy;

#line 4377
    uint _S219 = froxel_of_0(_S218, (((float4(_S205.world_position_10, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_27);

#line 4377
    uint base_3 = _S219 * 17U;

#line 4382
    uint _S220 = min((&kernelContext_27)->cluster_lights_0[base_3], 16U);

#line 4382
    TableTap_0 _S221 = table_tap_0(_S217, roughness_2, &kernelContext_27);

#line 4382
    thread TableTap_0 _S222 = _S221;

#line 4382
    float2 _S223 = dfg_at_0(&_S222, &kernelContext_27);

#line 4391
    float _S224 = _S223.x;

#line 4391
    float _S225 = _S223.y;

#line 4391
    float3 _S226 = f0_2 * float3(_S224)  + float3(_S225) ;

#line 4397
    float3 _S227 = float3(0.0f, 0.0f, 0.0f);

#line 4397
    uint slot_0 = 0U;

#line 4397
    float3 direct_0 = _S227;

#line 4397
    float3 gloss_0 = _S227;

    for(;;)
    {

#line 4399
        if(slot_0 < _S220)
        {
        }
        else
        {

#line 4399
            break;
        }

#line 4399
        thread GpuLight_natural_0 _S228 = (&kernelContext_27)->lights_0[(&kernelContext_27)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 4399
        uint _S229 = (&_S228)->kind_0;

#line 4408
        bool _S230 = ((&_S228)->kind_0) == 0U;

#line 4408
        float3 to_light_7;

#line 4408
        float reach_0;

#line 4408
        if(_S230)
        {

#line 4408
            to_light_7 = normalize((float4((&_S228)->direction_0) ).xyz);

#line 4408
            reach_0 = 1.0f;

#line 4408
        }
        else
        {


            if(_S229 == 3U)
            {

#line 4413
                float4 _S231 = float4((&_S228)->position_0) ;

#line 4421
                float3 offset_0 = _S231.xyz - _S205.world_position_10;
                float distance_3 = length(offset_0);

                float _S232 = range_window_0(distance_3, _S231.w);

#line 4424
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 4424
                reach_0 = _S232;

#line 4413
            }
            else
            {

#line 4413
                float4 _S233 = float4((&_S228)->position_0) ;

#line 4428
                float3 offset_1 = _S233.xyz - _S205.world_position_10;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_1 = punctual_falloff_0(distance_4, _S233.w);
                if(_S229 == 2U)
                {

#line 4432
                    float4 _S234 = float4((&_S228)->direction_0) ;

#line 4432
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S234.xyz, _S234.w, (&_S228)->cos_inner_0);

#line 4432
                }
                else
                {

#line 4432
                    reach_0 = reach_1;

#line 4432
                }

#line 4432
                to_light_7 = to_light_8;

#line 4413
            }

#line 4408
        }

#line 4441
        float n_dot_l_5 = dot(_S210, to_light_7);

#line 4441
        float3 specular_0;

#line 4441
        float diffuse_0;


        if(_S229 == 3U)
        {

#line 4454
            thread array<float3, int(4)> corners_2;

#line 4454
            rect_corners_0(&_S228, _S205.world_position_10, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S210, to_eye_1, _S217);

#line 4456
            thread array<float3, int(4)> _S235 = corners_2;

#line 4456
            float _S236 = ltc_irradiance_0(to_local_0, &_S235);

#line 4456
            thread TableTap_0 _S237 = _S221;

#line 4456
            float4 _S238 = ltc_at_0(&_S237, &kernelContext_27);

            matrix<float,int(3),int(3)>  _S239 = (((to_local_0) * (ltc_transform_0(_S238))));

#line 4458
            thread array<float3, int(4)> _S240 = corners_2;

#line 4458
            float _S241 = ltc_irradiance_0(_S239, &_S240);
            float3 _S242 = float3(_S241)  * _S226;

#line 4459
            diffuse_0 = _S236;

#line 4459
            specular_0 = _S242;

#line 4444
        }
        else
        {

#line 4464
            float _S243 = max(n_dot_l_5, 0.0f);

#line 4471
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 4479
            float3 specular_1 = ggx_lobe_0(_S215, f0_2, _S243, _S217, max(dot(_S210, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S243) ;

#line 4479
            diffuse_0 = _S243;

#line 4479
            specular_0 = specular_1;

#line 4444
        }

#line 4444
        float3 specular_2;

#line 4487
        if((((&_S228)->flags_3) & 1U) != 0U)
        {

#line 4487
            specular_2 = _S227;

#line 4487
        }
        else
        {

#line 4487
            specular_2 = specular_0;

#line 4487
        }

#line 4487
        float reach_2;

#line 4505
        if(_S230)
        {

#line 4505
            float _S244 = sun_visibility_0(_S205.world_position_10, to_light_7, n_dot_l_5, _S212, _S218, &kernelContext_27);

#line 4505
            float _S245 = contact_at_0(_S218, &kernelContext_27);

#line 4505
            reach_2 = _S244 * _S245;

#line 4505
        }
        else
        {

#line 4517
            if(_S229 == 1U)
            {

#line 4517
                uint _S246 = (&_S228)->shadow_tile_0;

#line 4529
                if(((&_S228)->shadow_tile_0) <= 8U)
                {

#line 4529
                    float _S247 = point_visibility_0(&_S228, _S246, _S205.world_position_10, to_light_7, n_dot_l_5, _S212, _S218, &kernelContext_27);

#line 4529
                    reach_2 = reach_0 * _S247;

#line 4529
                }
                else
                {

#line 4529
                    reach_2 = reach_0;

#line 4529
                }

#line 4517
            }
            else
            {

#line 4517
                uint _S248 = (&_S228)->shadow_tile_0;

#line 4535
                if(((&_S228)->shadow_tile_0) < 14U)
                {

#line 4535
                    float _S249 = spot_visibility_0(&_S228, _S248, _S205.world_position_10, to_light_7, n_dot_l_5, _S212, _S218, &kernelContext_27);

#line 4535
                    reach_2 = reach_0 * _S249;

#line 4535
                }
                else
                {

#line 4535
                    reach_2 = reach_0;

#line 4535
                }

#line 4517
            }

#line 4505
        }

#line 4543
        float3 _S250 = (float4((&_S228)->color_0) ).xyz;

#line 4543
        float3 direct_1 = direct_0 + _S250 * float3((diffuse_0 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S250 * (specular_2 * float3(reach_2) );

#line 4399
        slot_0 = slot_0 + 1U;

#line 4399
        direct_0 = direct_1;

#line 4399
        gloss_0 = gloss_1;

#line 4399
    }

#line 4558
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S224 + _S225);

#line 4558
    float _S251 = occlusion_at_0(_S218, &kernelContext_27);

#line 4594
    float3 _S252 = frame_5->ambient_0.xyz;

#line 4594
    float3 _S253 = sky_irradiance_0(_S210, &kernelContext_27);

#line 4594
    float3 _S254 = _S252 + _S253;

#line 4594
    float3 _S255 = probe_irradiance_0(_S205.world_position_10, _S210, &kernelContext_27);

#line 4630
    float3 lit_1 = diffuse_albedo_0 * ((_S254 + _S255) * multi_bounce_occlusion_0(_S251, diffuse_albedo_0) + direct_0) + gloss_2;

#line 4630
    float3 _S256 = emissive_of_0(&_S207);

#line 4666
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_27)->frame_0->fog_params_0.x, (&kernelContext_27)->frame_0->fog_params_0.y, (&kernelContext_27)->frame_0->camera_position_0.y - (&kernelContext_27)->frame_0->fog_params_0.z, _S205.world_position_10.y - (&kernelContext_27)->frame_0->fog_params_0.z, length((&kernelContext_27)->frame_0->camera_position_0.xyz - _S205.world_position_10)));


    thread FragmentOutput_0 output_2;



    (&output_2)->lit_0 = float4((lit_1 + _S256) * float3(fog_survives_0)  + (&kernelContext_27)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_1.w);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;
    return output_2;
}


#line 4679
struct vertexMain_Result_0
{
    float4 position_6 [[position]];
    float3 world_position_11 [[user(POSITION)]];
    float3 world_normal_2 [[user(NORMAL)]];
    float4 color_4 [[user(COLOR)]];
    uint material_6 [[user(TEXCOORD)]];
    float2 uv_4 [[user(TEXCOORD_1)]];
    float4 clip_position_2 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_2 [[user(TEXCOORD_3)]];
    float3 world_tangent_2 [[user(TEXCOORD_4)]];
    uint frame_6 [[user(TEXCOORD_5)]];
};


#line 4679
[[vertex]] vertexMain_Result_0 vertexMain(uint index_5 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]])
{

#line 4679
    thread KernelContext_0 kernelContext_28;

#line 4679
    (&kernelContext_28)->draw_0 = draw_4;

#line 4679
    (&kernelContext_28)->visible_instances_0 = visible_instances_4;

#line 4679
    (&kernelContext_28)->instances_0 = instances_4;

#line 4679
    (&kernelContext_28)->meshes_0 = meshes_4;

#line 4679
    (&kernelContext_28)->frame_0 = frame_7;

#line 4679
    (&kernelContext_28)->vertices_0 = vertices_4;

#line 4679
    (&kernelContext_28)->ambient_occlusion_0 = ambient_occlusion_4;

#line 4679
    (&kernelContext_28)->materials_0 = materials_4;

#line 4679
    (&kernelContext_28)->normal_textures_0 = normal_textures_4;

#line 4679
    (&kernelContext_28)->base_color_sampler_0 = base_color_sampler_4;

#line 4679
    (&kernelContext_28)->base_color_textures_0 = base_color_textures_4;

#line 4679
    (&kernelContext_28)->cluster_lights_0 = cluster_lights_4;

#line 4679
    (&kernelContext_28)->specular_dfg_0 = specular_dfg_4;

#line 4679
    (&kernelContext_28)->lights_0 = lights_4;

#line 4679
    (&kernelContext_28)->ltc_matrix_0 = ltc_matrix_4;

#line 4679
    (&kernelContext_28)->shadow_atlas_0 = shadow_atlas_4;

#line 4679
    (&kernelContext_28)->shadow_sampler_0 = shadow_sampler_4;

#line 4679
    (&kernelContext_28)->contact_shadow_0 = contact_shadow_4;

#line 4679
    (&kernelContext_28)->probes_0 = probes_4;

#line 4679
    GpuInstance_natural_0 device* _S257 = instances_4+visible_instances_4[draw_4->base_0 + instance_id_1];

#line 1672
    GpuMesh_0 mesh_3 = meshes_4[draw_4->mesh_0];

#line 1680
    bool _S258 = ((_S257->flags_0) & 2U) != 0U;

#line 1680
    uint base_vertex_3;
    if(_S258)
    {

#line 1681
        base_vertex_3 = _S257->base_vertex_0;

#line 1681
    }
    else
    {

#line 1681
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1681
    }

#line 1681
    MeshVertex_0 _S259 = load_vertex_0(index_5 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_28);

#line 1681
    uint previous_base_0;

#line 1694
    if(_S258)
    {

#line 1694
        previous_base_0 = _S257->previous_base_vertex_0;

#line 1694
    }
    else
    {

#line 1694
        previous_base_0 = base_vertex_3;

#line 1694
    }

#line 1694
    float3 _S260 = load_position_0(index_5 + previous_base_0, &kernelContext_28);

#line 1694
    matrix<float,int(4),int(4)>  _S261 = matrix<float,int(4),int(4)> (_S257->transform_0.data_0[int(0)][int(0)], _S257->transform_0.data_0[int(1)][int(0)], _S257->transform_0.data_0[int(2)][int(0)], _S257->transform_0.data_0[int(3)][int(0)], _S257->transform_0.data_0[int(0)][int(1)], _S257->transform_0.data_0[int(1)][int(1)], _S257->transform_0.data_0[int(2)][int(1)], _S257->transform_0.data_0[int(3)][int(1)], _S257->transform_0.data_0[int(0)][int(2)], _S257->transform_0.data_0[int(1)][int(2)], _S257->transform_0.data_0[int(2)][int(2)], _S257->transform_0.data_0[int(3)][int(2)], _S257->transform_0.data_0[int(0)][int(3)], _S257->transform_0.data_0[int(1)][int(3)], _S257->transform_0.data_0[int(2)][int(3)], _S257->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S259.position_1, 1.0f)) * (_S261)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_28)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_28)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_0.xyz;

#line 1708
    matrix<float,int(3),int(3)>  _S262 = matrix<float,int(3),int(3)> (_S261[int(0)].xyz, _S261[int(1)].xyz, _S261[int(2)].xyz);

#line 1708
    (&output_3)->world_normal_0 = (((_S259.basis_1.normal_0) * (normal_basis_0(_S262))));

#line 1714
    (&output_3)->world_tangent_0 = (((_S259.basis_1.tangent_1) * (_S262)));

#line 1714
    thread TangentFrame_0 _S263 = _S259.basis_1;

#line 1714
    uint _S264 = frame_word_0(mesh_3.flags_1, &_S263);
    (&output_3)->frame_3 = _S264;

#line 1715
    float4 _S265;

#line 1722
    if(((&kernelContext_28)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1722
        _S265 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1722
    }
    else
    {

#line 1722
        _S265 = _S259.color_1;

#line 1722
    }

#line 1721
    (&output_3)->color_2 = _S265;

#line 1728
    (&output_3)->material_2 = _S257->material_0;
    (&output_3)->uv_0 = _S259.uv0_0;

#line 1735
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S260, 1.0f)) * (matrix<float,int(4),int(4)> (_S257->previous_transform_0.data_0[int(0)][int(0)], _S257->previous_transform_0.data_0[int(1)][int(0)], _S257->previous_transform_0.data_0[int(2)][int(0)], _S257->previous_transform_0.data_0[int(3)][int(0)], _S257->previous_transform_0.data_0[int(0)][int(1)], _S257->previous_transform_0.data_0[int(1)][int(1)], _S257->previous_transform_0.data_0[int(2)][int(1)], _S257->previous_transform_0.data_0[int(3)][int(1)], _S257->previous_transform_0.data_0[int(0)][int(2)], _S257->previous_transform_0.data_0[int(1)][int(2)], _S257->previous_transform_0.data_0[int(2)][int(2)], _S257->previous_transform_0.data_0[int(3)][int(2)], _S257->previous_transform_0.data_0[int(0)][int(3)], _S257->previous_transform_0.data_0[int(1)][int(3)], _S257->previous_transform_0.data_0[int(2)][int(3)], _S257->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_28)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S266 = output_3;

#line 1739
    thread vertexMain_Result_0 _S267;

#line 1739
    (&_S267)->position_6 = _S266.position_3;

#line 1739
    (&_S267)->world_position_11 = _S266.world_position_1;

#line 1739
    (&_S267)->world_normal_2 = _S266.world_normal_0;

#line 1739
    (&_S267)->color_4 = _S266.color_2;

#line 1739
    (&_S267)->material_6 = _S266.material_2;

#line 1739
    (&_S267)->uv_4 = _S266.uv_0;

#line 1739
    (&_S267)->clip_position_2 = _S266.clip_position_0;

#line 1739
    (&_S267)->previous_clip_position_2 = _S266.previous_clip_position_0;

#line 1739
    (&_S267)->world_tangent_2 = _S266.world_tangent_0;

#line 1739
    (&_S267)->frame_6 = _S266.frame_3;

#line 1739
    return _S267;
}

