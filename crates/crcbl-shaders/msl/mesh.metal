#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2510 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2505
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2990
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2777
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2837
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2852
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2880
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1170
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1814
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1814
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


#line 839
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


#line 1820
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1820
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
    texture2d_array<float, access::sample> probe_visibility_0;
};


#line 1213
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


#line 1224
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1227
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1678
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1801
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1801
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1803
        word_4 = 1U;

#line 1803
    }
    else
    {

#line 1803
        word_4 = 0U;

#line 1803
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1807
        word_4 = word_4 | 2U;

#line 1807
    }

#line 1806
    return word_4;
}


#line 1806
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1921
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 1921
    thread KernelContext_0 kernelContext_2;

#line 1921
    (&kernelContext_2)->draw_0 = draw_1;

#line 1921
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1921
    (&kernelContext_2)->instances_0 = instances_1;

#line 1921
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1921
    (&kernelContext_2)->frame_0 = frame_1;

#line 1921
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1921
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1921
    (&kernelContext_2)->materials_0 = materials_1;

#line 1921
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1921
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1921
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1921
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1921
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1921
    (&kernelContext_2)->lights_0 = lights_1;

#line 1921
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1921
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1921
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1921
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 1921
    (&kernelContext_2)->probes_0 = probes_1;

#line 1921
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 1921
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1924
    uint base_vertex_2;

#line 1930
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1930
        base_vertex_2 = _S7->base_vertex_0;

#line 1930
    }
    else
    {

#line 1930
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1930
    }

#line 1930
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1930
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1930
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 1933
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 1954
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 1954
    thread KernelContext_0 kernelContext_3;

#line 1954
    (&kernelContext_3)->draw_0 = draw_2;

#line 1954
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 1954
    (&kernelContext_3)->instances_0 = instances_2;

#line 1954
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 1954
    (&kernelContext_3)->frame_0 = frame_2;

#line 1954
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 1954
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1954
    (&kernelContext_3)->materials_0 = materials_2;

#line 1954
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 1954
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 1954
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 1954
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 1954
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 1954
    (&kernelContext_3)->lights_0 = lights_2;

#line 1954
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 1954
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 1954
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 1954
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 1954
    (&kernelContext_3)->probes_0 = probes_2;

#line 1954
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 1954
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 4795
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 4797
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4671
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4671
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4677
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4405
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4409
    float _S16 = axis_0.y;

#line 4409
    bool _S17;

#line 4409
    if(_S15 >= _S16)
    {

#line 4409
        _S17 = _S15 >= (axis_0.z);

#line 4409
    }
    else
    {

#line 4409
        _S17 = false;

#line 4409
    }

#line 4409
    float2 planar_0;

#line 4409
    if(_S17)
    {

#line 4409
        planar_0 = world_position_0.zy;

#line 4409
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4413
            planar_0 = world_position_0.xz;

#line 4413
        }
        else
        {

#line 4413
            planar_0 = world_position_0.xy;

#line 4413
        }

#line 4409
    }

#line 4421
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1024
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 4442
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S18 = normal_2.z;

#line 4444
    float sign_z_0;

#line 4444
    if(_S18 >= 0.0f)
    {

#line 4444
        sign_z_0 = 1.0f;

#line 4444
    }
    else
    {

#line 4444
        sign_z_0 = -1.0f;

#line 4444
    }
    float a_0 = -1.0f / (sign_z_0 + _S18);
    float _S19 = normal_2.x;

#line 4446
    float _S20 = sign_z_0 * _S19;

#line 4446
    return float3(1.0f + _S20 * _S19 * a_0, _S20 * normal_2.y * a_0, - sign_z_0 * _S19);
}


#line 4496
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S21 = duvdy_0.y;

#line 4498
    float _S22 = duvdx_0.y;

#line 4498
    float winding_0;
    if((duvdx_0.x * _S21 - duvdy_0.x * _S22) < 0.0f)
    {

#line 4499
        winding_0 = -1.0f;

#line 4499
    }
    else
    {

#line 4499
        winding_0 = 1.0f;

#line 4499
    }
    float3 tangent_2 = (float3(_S21)  * dpdx_0 - float3(_S22)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 4508
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 4509
    float3 _S23;

#line 4518
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 4518
        _S23 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 4518
    }
    else
    {

#line 4518
        _S23 = orthonormal_tangent_0(normal_3);

#line 4518
    }

#line 4518
    (&basis_4)->tangent_1 = _S23;

    (&basis_4)->bitangent_0 = cross(normal_3, _S23);
    return basis_4;
}


#line 1685
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


#line 4578
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 4590
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 4600
    uint _S24 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4609
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4611
        float3 _S25;

#line 4616
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4616
            _S25 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4616
        }
        else
        {

#line 4616
            _S25 = orthonormal_tangent_0(normal_4);

#line 4616
        }

#line 4616
        (&basis_5)->tangent_1 = _S25;

#line 4622
        float3 _S26 = cross((&basis_5)->normal_0, _S25);

#line 4622
        float _S27;
        if((_S24 & 2U) != 0U)
        {

#line 4623
            _S27 = -1.0f;

#line 4623
        }
        else
        {

#line 4623
            _S27 = 1.0f;

#line 4623
        }

#line 4622
        (&basis_5)->bitangent_0 = _S26 * float3(_S27) ;

#line 4601
    }
    else
    {

#line 4627
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 4601
    }

#line 4631
    float3 _S28 = float3(uv_1, float(layer_0));
    float3 _S29 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S28)).xy, uint(((_S28)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4632
    thread float3 tangent_space_0 = _S29;
    tangent_space_0.xy = _S29.xy * float2(normal_scale_1) ;

#line 4638
    float3 _S30 = normalize(tangent_space_0);

#line 4638
    tangent_space_0 = _S30;
    return normalize(float3(_S30.x)  * (&basis_5)->tangent_1 + float3(_S30.y)  * (&basis_5)->bitangent_0 + float3(_S30.z)  * (&basis_5)->normal_0);
}


#line 2645
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2656
    float3 _S31;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2657
        _S31 = - facet_1;

#line 2657
    }
    else
    {

#line 2657
        _S31 = facet_1;

#line 2657
    }

#line 2657
    return _S31;
}


#line 1009
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3856
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S32 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S33 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S34 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 3866
    uint _S36 = uint(pixel_0.x) / _S35;

#line 3866
    uint _S37 = min(_S36, _S32 - 1U);
    uint _S38 = uint(pixel_0.y) / _S35;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3877
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S34 - 1U))) * _S33 + min(_S38, _S33 - 1U)) * _S32 + _S37;
}


#line 2077
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2098
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 2098
    texture2d<float, access::sample> _S39 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S39).get_width(0)),(*((&height_1)) = (_S39).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2104
    float2 _S40 = float2(1.0f) ;
    float2 _S41 = extent_1 - _S40;

#line 2105
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S41);
    float2 high_1 = min(low_1 + _S40, _S41);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2123
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 2135
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S42 = tap_1->lo_0.x;

#line 2137
    int _S43 = tap_1->lo_0.y;

#line 2137
    int3 _S44 = int3(_S42, _S43, int(0));
    int _S45 = tap_1->hi_0.x;

#line 2138
    int3 _S46 = int3(_S45, _S43, int(0));
    float2 _S47 = float2(tap_1->weight_0.x) ;
    int _S48 = tap_1->hi_0.y;

#line 2140
    int3 _S49 = int3(_S42, _S48, int(0));
    int3 _S50 = int3(_S45, _S48, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)))), _S47), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), _S47), float2(tap_1->weight_0.y) );
}


#line 3807
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3823
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3835
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3842
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2464
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2464
    float4 _S51 = float4(light_0->tangent_0) ;

    float3 _S52 = _S51.xyz;

#line 2466
    float3 across_0 = _S52 * float3(_S51.w) ;

#line 2466
    float4 _S53 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S52, _S53.xyz) * float3(_S53.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S54 = centre_0 - across_0;

#line 2469
    (*corners_0)[int(0)] = _S54 - down_0;
    float3 _S55 = centre_0 + across_0;

#line 2470
    (*corners_0)[int(1)] = _S55 - down_0;
    (*corners_0)[int(2)] = _S55 + down_0;
    (*corners_0)[int(3)] = _S54 + down_0;
    return;
}


#line 2222
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2225
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2226
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2226
    }
    else
    {

#line 2226
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2226
    }

#line 2226
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2227
        tangent_5 = across_1 / float3(span_0) ;

#line 2227
    }
    else
    {

#line 2227
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2227
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 2203
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2293
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2293
    float3 _S56 = polygon_0->corner_0[int(0)];

#line 2293
    float3 _S57 = polygon_0->corner_0[int(1)];

#line 2293
    float3 _S58 = polygon_0->corner_0[int(2)];

#line 2293
    float3 _S59 = polygon_0->corner_0[int(3)];

#line 2299
    float3 _S60 = float3(0.0f, 0.0f, 0.0f);


    float _S61 = polygon_0->corner_0[int(0)].z;

#line 2302
    int count_1;

#line 2302
    if(_S61 > 0.0f)
    {

#line 2302
        count_1 = int(1);

#line 2302
    }
    else
    {

#line 2302
        count_1 = int(0);

#line 2302
    }
    float _S62 = _S57.z;

#line 2303
    int _S63;

#line 2303
    if(_S62 > 0.0f)
    {

#line 2303
        _S63 = int(2);

#line 2303
    }
    else
    {

#line 2303
        _S63 = int(0);

#line 2303
    }

#line 2303
    int config_0 = count_1 + _S63;
    float _S64 = _S58.z;

#line 2304
    if(_S64 > 0.0f)
    {

#line 2304
        count_1 = int(4);

#line 2304
    }
    else
    {

#line 2304
        count_1 = int(0);

#line 2304
    }

#line 2304
    int config_1 = config_0 + count_1;
    float _S65 = _S59.z;

#line 2305
    if(_S65 > 0.0f)
    {

#line 2305
        count_1 = int(8);

#line 2305
    }
    else
    {

#line 2305
        count_1 = int(0);

#line 2305
    }

#line 2305
    int config_2 = config_1 + count_1;

#line 2305
    float3 l0_0;

#line 2305
    float3 l1_0;

#line 2305
    float3 l2_0;

#line 2305
    float3 l3_0;

#line 2305
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2308
        float3 _S66 = float3(_S61) ;


        float3 _S67 = float3(- _S62)  * _S56 + _S66 * _S57;
        float3 _S68 = float3(- _S65)  * _S56 + _S66 * _S59;

#line 2312
        count_1 = int(3);

#line 2312
        l0_0 = _S56;

#line 2312
        l1_0 = _S67;

#line 2312
        l2_0 = _S68;

#line 2312
        l3_0 = _S59;

#line 2312
        l4_0 = _S60;

#line 2308
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2314
            float3 _S69 = float3(_S62) ;


            float3 _S70 = float3(- _S61)  * _S57 + _S69 * _S56;
            float3 _S71 = float3(- _S64)  * _S57 + _S69 * _S58;

#line 2318
            count_1 = int(3);

#line 2318
            l0_0 = _S70;

#line 2318
            l1_0 = _S57;

#line 2318
            l2_0 = _S71;

#line 2318
            l3_0 = _S59;

#line 2318
            l4_0 = _S60;

#line 2314
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S72 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                float3 _S73 = float3(- _S65)  * _S56 + float3(_S61)  * _S59;

#line 2324
                count_1 = int(4);

#line 2324
                l0_0 = _S56;

#line 2324
                l1_0 = _S57;

#line 2324
                l2_0 = _S72;

#line 2324
                l3_0 = _S73;

#line 2324
                l4_0 = _S60;

#line 2320
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2326
                    float3 _S74 = float3(_S64) ;


                    float3 _S75 = float3(- _S65)  * _S58 + _S74 * _S59;
                    float3 _S76 = float3(- _S62)  * _S58 + _S74 * _S57;

#line 2330
                    count_1 = int(3);

#line 2330
                    l0_0 = _S75;

#line 2330
                    l1_0 = _S76;

#line 2330
                    l2_0 = _S58;

#line 2330
                    l3_0 = _S59;

#line 2330
                    l4_0 = _S60;

#line 2326
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S77 = float3(- _S61)  * _S57 + float3(_S62)  * _S56;
                        float3 _S78 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;

#line 2336
                        count_1 = int(4);

#line 2336
                        l0_0 = _S77;

#line 2336
                        l1_0 = _S57;

#line 2336
                        l2_0 = _S58;

#line 2336
                        l3_0 = _S78;

#line 2336
                        l4_0 = _S60;

#line 2332
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2338
                            float3 _S79 = float3(- _S65) ;


                            float3 _S80 = _S79 * _S56 + float3(_S61)  * _S59;
                            float3 _S81 = _S79 * _S58 + float3(_S64)  * _S59;

#line 2342
                            count_1 = int(5);

#line 2342
                            l0_0 = _S56;

#line 2342
                            l1_0 = _S57;

#line 2342
                            l2_0 = _S58;

#line 2342
                            l3_0 = _S81;

#line 2342
                            l4_0 = _S80;

#line 2338
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2344
                                float3 _S82 = float3(_S65) ;


                                float3 _S83 = float3(- _S61)  * _S59 + _S82 * _S56;
                                float3 _S84 = float3(- _S64)  * _S59 + _S82 * _S58;

#line 2348
                                count_1 = int(3);

#line 2348
                                l0_0 = _S83;

#line 2348
                                l1_0 = _S84;

#line 2348
                                l2_0 = _S59;

#line 2348
                                l3_0 = _S59;

#line 2348
                                l4_0 = _S60;

#line 2344
                            }
                            else
                            {

#line 2351
                                if(config_2 == int(9))
                                {

                                    float3 _S85 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;
                                    float3 _S86 = float3(- _S64)  * _S59 + float3(_S65)  * _S58;

#line 2355
                                    count_1 = int(4);

#line 2355
                                    l0_0 = _S56;

#line 2355
                                    l1_0 = _S85;

#line 2355
                                    l2_0 = _S86;

#line 2355
                                    l3_0 = _S59;

#line 2355
                                    l4_0 = _S60;

#line 2351
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S87 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;
                                        float3 _S88 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;

#line 2362
                                        count_1 = int(5);

#line 2362
                                        l0_0 = _S56;

#line 2362
                                        l1_0 = _S57;

#line 2362
                                        l2_0 = _S88;

#line 2362
                                        l3_0 = _S87;

#line 2362
                                        l4_0 = _S59;

#line 2357
                                    }
                                    else
                                    {

#line 2364
                                        if(config_2 == int(12))
                                        {

                                            float3 _S89 = float3(- _S62)  * _S58 + float3(_S64)  * _S57;
                                            float3 _S90 = float3(- _S61)  * _S59 + float3(_S65)  * _S56;

#line 2368
                                            count_1 = int(4);

#line 2368
                                            l0_0 = _S90;

#line 2368
                                            l1_0 = _S89;

#line 2368
                                            l2_0 = _S58;

#line 2368
                                            l3_0 = _S59;

#line 2368
                                            l4_0 = _S60;

#line 2364
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S91 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                                                float3 _S92 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;

#line 2376
                                                count_1 = int(5);

#line 2376
                                                l0_0 = _S56;

#line 2376
                                                l1_0 = _S92;

#line 2376
                                                l2_0 = _S91;

#line 2376
                                                l3_0 = _S58;

#line 2376
                                                l4_0 = _S59;

#line 2370
                                            }
                                            else
                                            {

#line 2378
                                                if(config_2 == int(14))
                                                {

#line 2378
                                                    float3 _S93 = float3(- _S61) ;


                                                    float3 _S94 = _S93 * _S59 + float3(_S65)  * _S56;
                                                    float3 _S95 = _S93 * _S57 + float3(_S62)  * _S56;

#line 2382
                                                    count_1 = int(5);

#line 2382
                                                    l0_0 = _S95;

#line 2382
                                                    l1_0 = _S94;

#line 2378
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2384
                                                        count_1 = int(4);

#line 2384
                                                    }
                                                    else
                                                    {

#line 2384
                                                        count_1 = int(0);

#line 2384
                                                    }

#line 2384
                                                    l0_0 = _S56;

#line 2384
                                                    l1_0 = _S60;

#line 2378
                                                }

#line 2299
                                                float3 _S96 = l1_0;

#line 2299
                                                l1_0 = _S57;

#line 2299
                                                l2_0 = _S58;

#line 2299
                                                l3_0 = _S59;

#line 2299
                                                l4_0 = _S96;

#line 2370
                                            }

#line 2364
                                        }

#line 2357
                                    }

#line 2351
                                }

#line 2344
                            }

#line 2338
                        }

#line 2332
                    }

#line 2326
                }

#line 2320
            }

#line 2314
        }

#line 2308
    }

#line 2392
    if(count_1 <= int(3))
    {

#line 2392
        l3_0 = l0_0;

#line 2392
        l4_0 = l0_0;

#line 2392
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2397
            l4_0 = l0_0;

#line 2397
        }

#line 2392
    }

#line 2402
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2265
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2271
    float weight_1;

#line 2276
    if(cosine_0 > 0.0f)
    {

#line 2276
        weight_1 = fit_0;

#line 2276
    }
    else
    {

#line 2276
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2276
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2422
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2424
    int corner_1 = int(0);
    for(;;)
    {

#line 2425
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2425
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2425
        corner_1 = corner_1 + int(1);

#line 2425
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2430
    thread LtcPolygon_0 _S97 = polygon_1;

#line 2430
    LtcPolygon_0 _S98 = ltc_clip_0(&_S97);
    polygon_1 = _S98;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2434
    int at_2 = int(0);

    for(;;)
    {

#line 2436
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2436
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2436
        at_2 = at_2 + int(1);

#line 2436
    }

#line 2443
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2443
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2444
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2444
    }
    else
    {

#line 2444
        sum_1 = sum_0;

#line 2444
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2448
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2448
    }

#line 2455
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2151
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S99 = tap_2->lo_0.x;

#line 2153
    int _S100 = tap_2->lo_0.y;

#line 2153
    int3 _S101 = int3(_S99, _S100, int(0));
    int _S102 = tap_2->hi_0.x;

#line 2154
    int3 _S103 = int3(_S102, _S100, int(0));
    float4 _S104 = float4(tap_2->weight_0.x) ;
    int _S105 = tap_2->hi_0.y;

#line 2156
    int3 _S106 = int3(_S99, _S105, int(0));
    int3 _S107 = int3(_S102, _S105, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S101)).xy), uint(((_S101)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S103)).xy), uint(((_S103)).z))), _S104), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S107)).xy), uint(((_S107)).z))), _S104), float4(tap_2->weight_0.y) );
}


#line 2238
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2033
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2040
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2047
    float _S108 = 1.0f - alpha2_0;

#line 2052
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S108 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S108 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3067
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3067
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3127
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3099
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2696
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3054
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_13)
{

#line 3054
    uint _S109;

    if(uint(pixel_1.x) < (kernelContext_13->frame_0->shadow_filter_0.z))
    {

#line 3056
        _S109 = kernelContext_13->frame_0->shadow_filter_0.x;

#line 3056
    }
    else
    {

#line 3056
        _S109 = kernelContext_13->frame_0->shadow_filter_0.y;

#line 3056
    }

#line 3056
    return _S109;
}


#line 3079
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3079
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 349
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3149
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S110 = spoke_0.x;

#line 3154
    float _S111 = rotation_0.x;

#line 3154
    float _S112 = spoke_0.y;

#line 3154
    float _S113 = rotation_0.y;


    float _S114 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S110 * _S111 - _S112 * _S113, _S110 * _S113 + _S112 * _S111) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3157
    return _S114;
}


#line 3237
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_17)
{

#line 3237
    float4 _S115 = atlas_rect_1(tile_2, kernelContext_17);


    if(atlas_rect_is_empty_0(_S115))
    {
        return 1.0f;
    }

#line 3242
    float2 _S116 = atlas_step_1(_S115, kernelContext_17);

#line 3242
    int y_1 = int(-1);

#line 3242
    float visibility_0 = 0.0f;

#line 3247
    for(;;)
    {

#line 3247
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3247
            break;
        }

#line 3247
        int x_0 = int(-1);

        for(;;)
        {

#line 3249
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3249
                break;
            }

#line 3249
            float _S117 = tile_tap_0(_S115, _S116, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_17);

            float visibility_1 = visibility_0 + _S117;

#line 3249
            x_0 = x_0 + int(1);

#line 3249
            visibility_0 = visibility_1;

#line 3249
        }

#line 3247
        y_1 = y_1 + int(1);

#line 3247
    }

#line 3255
    return visibility_0 / 9.0f;
}


#line 3012
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3179
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_18)
{
    float2 _S118 = shadow_rotation_0(pixel_3);

#line 3181
    float4 _S119 = atlas_rect_1(tile_3, kernelContext_18);

    if(atlas_rect_is_empty_0(_S119))
    {
        return 1.0f;
    }

#line 3185
    float2 _S120 = atlas_step_1(_S119, kernelContext_18);

#line 3185
    uint spot_0 = 0U;

#line 3185
    float probe_0 = 0.0f;

#line 3190
    for(;;)
    {

#line 3190
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3190
            break;
        }

#line 3190
        float _S121 = tile_tap_0(_S119, _S120, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S118, reference_2, kernelContext_18);

        float probe_1 = probe_0 + _S121;

#line 3190
        spot_0 = spot_0 + 1U;

#line 3190
        probe_0 = probe_1;

#line 3190
    }

#line 3199
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3205
    uint index_2 = 0U;

#line 3205
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3209
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3209
            break;
        }

#line 3209
        float _S122 = tile_tap_0(_S119, _S120, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S118, reference_2, kernelContext_18);

        float visibility_3 = visibility_2 + _S122;

#line 3209
        index_2 = index_2 + 1U;

#line 3209
        visibility_2 = visibility_3;

#line 3209
    }

#line 3214
    return visibility_2 / 32.0f;
}


#line 3290
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_19)
{
    float2 texel_1 = kernelContext_19->frame_0->shadow_params_0.xy;

#line 3292
    float4 _S123 = atlas_rect_0(cascade_0, kernelContext_19);

#line 3292
    float2 _S124 = atlas_step_0(_S123, kernelContext_19);


    float2 _S125 = float2(0.5f, 0.5f) * _S124;


    float2 _S126 = float2(1.0f, 1.0f);

#line 3298
    float2 _S127 = _S126 / texel_1;

#line 3298
    uint index_3 = 0U;

#line 3298
    float sum_2 = 0.0f;

#line 3298
    float found_0 = 0.0f;



    for(;;)
    {

#line 3302
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3302
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S128 = spoke_1.x;

#line 3305
        float _S129 = rotation_1.x;

#line 3305
        float _S130 = spoke_1.y;

#line 3305
        float _S131 = rotation_1.y;

#line 3313
        int3 _S132 = int3(int2(min(atlas_uv_0(_S123, clamp(tile_uv_4 + float2(_S128 * _S129 - _S130 * _S131, _S128 * _S131 + _S130 * _S129) * _S124, _S125, float2(1.0f)  - _S125)) * _S127, _S127 - _S126)), int(0));

#line 3313
        float depth_1 = ((kernelContext_19->shadow_atlas_0).read(vec<uint,2>(((_S132)).xy), uint(((_S132)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3317
            sum_2 = sum_2 + depth_1;

#line 3317
            found_0 = found_1;

#line 3314
        }

#line 3302
        index_3 = index_3 + 1U;

#line 3302
    }

#line 3321
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3332
    float _S133 = 2.0f * kernelContext_19->frame_0->cascade_far_0[cascade_0];

#line 3332
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S133 + 40.0f);

#line 3332
    float _S134 = tile_texels_0(_S123, kernelContext_19);

    return clamp(separation_0 * 0.01999999955296516f / (_S133 / _S134), 2.0f, 8.0f);
}


#line 3386
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_20)
{

#line 3387
    float4 _S135 = atlas_rect_0(cascade_1, kernelContext_20);

#line 3421
    if(atlas_rect_is_empty_0(_S135))
    {


        return 1.0f;
    }
    float _S136 = 2.0f * kernelContext_20->frame_0->cascade_far_0[cascade_1];

#line 3427
    float _S137 = tile_texels_0(_S135, kernelContext_20);

#line 3427
    float texel_world_0 = _S136 / _S137;

#line 3434
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3438
    bool _S138;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3439
        _S138 = true;

#line 3439
    }
    else
    {

#line 3439
        _S138 = (ndc_0.z) <= 0.0f;

#line 3439
    }

#line 3439
    if(_S138)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3449
    uint _S139 = shadow_filter_mode_0(pixel_4, kernelContext_20);

#line 3466
    if(_S139 == 2U)
    {

#line 3466
        float _S140 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_20);

        return _S140;
    }
    if(_S139 == 1U)
    {

#line 3470
        float _S141 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_20);



        return _S141;
    }

    float _S142 = ndc_0.z;

#line 3477
    float _S143 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S142, shadow_rotation_0(pixel_4), kernelContext_20);

#line 3477
    float _S144 = tile_pcf_0(cascade_1, tile_uv_5, _S142, pixel_4, _S143, kernelContext_20);
    return _S144;
}


#line 3494
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, KernelContext_0 thread* kernelContext_21)
{

#line 3495
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3507
    float eye_distance_0 = length(world_position_5 - kernelContext_21->frame_0->camera_position_0.xyz);

#line 3507
    uint index_4 = 0U;

    for(;;)
    {

#line 3509
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3509
            cascade_2 = 1U;

#line 3509
            break;
        }
        if(eye_distance_0 < kernelContext_21->frame_0->cascade_far_0[index_4])
        {

#line 3511
            cascade_2 = index_4;


            break;
        }

#line 3509
        index_4 = index_4 + 1U;

#line 3509
    }

#line 3509
    float _S145 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3520
    uint _S146 = cascade_2 + 1U;

#line 3520
    if(_S146 >= 2U)
    {



        return _S145;
    }

#line 3533
    float band_0 = kernelContext_21->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_21->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S145;
    }

#line 3537
    float _S147 = cascade_visibility_0(_S146, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3548
    return mix(_S145, _S147, blend_0);
}


#line 4707
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_22)
{

#line 4707
    texture2d<float, access::sample> _S148 = kernelContext_22->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S148).get_width(0)),(*((&height_2)) = (_S148).get_height(0));

    int3 _S149 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 4713
    return ((kernelContext_22->contact_shadow_0).read(vec<uint,2>(((_S149)).xy), uint(((_S149)).z)).x);
}


#line 3759
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S150 = axis_2.x;

#line 3762
    float _S151 = axis_2.y;

#line 3762
    bool _S152;

#line 3762
    if(_S150 >= _S151)
    {

#line 3762
        _S152 = _S150 >= (axis_2.z);

#line 3762
    }
    else
    {

#line 3762
        _S152 = false;

#line 3762
    }

#line 3762
    uint _S153;

#line 3762
    if(_S152)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3764
            _S153 = 0U;

#line 3764
        }
        else
        {

#line 3764
            _S153 = 1U;

#line 3764
        }

#line 3764
        return _S153;
    }
    if(_S151 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3768
            _S153 = 2U;

#line 3768
        }
        else
        {

#line 3768
            _S153 = 3U;

#line 3768
        }

#line 3768
        return _S153;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3770
        _S153 = 4U;

#line 3770
    }
    else
    {

#line 3770
        _S153 = 5U;

#line 3770
    }

#line 3770
    return _S153;
}


#line 336
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 3655
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_23)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 3658
    float4 _S154 = atlas_rect_0(atlas_0, kernelContext_23);

    if(atlas_rect_is_empty_0(_S154))
    {


        return 1.0f;
    }

#line 3664
    float _S155 = tile_texels_0(_S154, kernelContext_23);

    float texel_world_1 = map_world_0 / _S155;

#line 3676
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 3683
    float _S156 = clip_1.w;

#line 3683
    if(_S156 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S156) ;

#line 3687
    bool _S157;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3688
        _S157 = true;

#line 3688
    }
    else
    {

#line 3688
        _S157 = (ndc_1.z) <= 0.0f;

#line 3688
    }

#line 3688
    if(_S157)
    {

#line 3688
        _S157 = true;

#line 3688
    }
    else
    {

#line 3688
        _S157 = (ndc_1.z) > 1.0f;

#line 3688
    }

#line 3688
    if(_S157)
    {

#line 3695
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 3700
    uint _S158 = shadow_filter_mode_0(pixel_6, kernelContext_23);

#line 3709
    if(_S158 == 2U)
    {

#line 3709
        float _S159 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_23);

        return _S159;
    }

#line 3711
    float _S160 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_23);

    return _S160;
}


#line 3778
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_24)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3786
    float _S161 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_24);

#line 3792
    return _S161;
}


#line 3720
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_25)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3727
    float4 _S162 = float4(light_2->direction_0) ;

#line 3734
    float cos_outer_1 = _S162.w;

#line 3734
    float _S163 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S162.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_25);

#line 3741
    return _S163;
}


#line 2179
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 4694
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 4696
    float3 _S164;
    if((length(decoded_0)) < 0.5f)
    {

#line 4697
        _S164 = shading_normal_1;

#line 4697
    }
    else
    {

#line 4697
        _S164 = normalize(decoded_0);

#line 4697
    }

#line 4697
    return _S164;
}


#line 4332
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_26)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_26->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4236
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4236
    float reach_0 = 0.0f;

#line 4236
    uint axis_3 = 0U;


    for(;;)
    {

#line 4239
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4239
            break;
        }

#line 4239
        uint _S165 = axis_3;

#line 4239
        bool _S166;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4241
            _S166 = true;

#line 4241
        }
        else
        {

#line 4241
            _S166 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4241
        }

#line 4241
        if(_S166)
        {

#line 4242
            axis_3 = axis_3 + 1U;

#line 4239
            continue;
        }

#line 4239
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S165] - 1.0f));

#line 4239
        axis_3 = axis_3 + 1U;

#line 4239
    }

#line 4246
    return reach_0;
}


#line 4266
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4266
    uint level_0 = 0U;

    for(;;)
    {

#line 4268
        uint _S167 = level_0 + 1U;

#line 4268
        if(_S167 < levels_0)
        {
        }
        else
        {

#line 4268
            break;
        }
        float _S168 = float(level_0);

#line 4270
        float at_3 = reach_1 * exp2(- _S168);
        if(at_3 < 1.0f)
        {

#line 4272
            return float2(_S168, saturate((1.0f - at_3) / 0.25f));
        }

#line 4268
        level_0 = _S167;

#line 4268
    }

#line 4274
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4054
uint probe_row_0(uint level_1, uint3 cell_1, KernelContext_0 thread* kernelContext_27)
{


    return min(kernelContext_27->frame_0->probe_levels_0.y * level_1 + (cell_1.z * kernelContext_27->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_27->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_27->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 3919
float sign_not_zero_0(float value_0)
{

#line 3919
    float _S169;

    if(value_0 >= 0.0f)
    {

#line 3921
        _S169 = 1.0f;

#line 3921
    }
    else
    {

#line 3921
        _S169 = -1.0f;

#line 3921
    }

#line 3921
    return _S169;
}


#line 3938
float2 oct_encode_0(float3 direction_1)
{
    float _S170 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S170) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 3941
    float2 p_1;
    if(_S170 < 0.0f)
    {
        float _S171 = p_0.y;

#line 3944
        float _S172 = p_0.x;

#line 3944
        p_1 = float2((1.0f - abs(_S171)) * sign_not_zero_0(_S172), (1.0f - abs(_S172)) * sign_not_zero_0(_S171));

#line 3942
    }
    else
    {

#line 3942
        p_1 = p_0;

#line 3942
    }

#line 3947
    return p_1;
}


#line 3967
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_28)
{

#line 3967
    texture2d_array<float, access::sample> _S173 = kernelContext_28->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S173).get_width(0)),(*((&height_3)) = (_S173).get_height(0)),(*((&layers_0)) = (_S173).get_array_size());

#line 3972
    float2 _S174 = float2(0.5f) ;

#line 3972
    float2 _S175 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S174 + _S174) * float2(16.0f)  + _S175 - _S174;
    float2 _S176 = float2(float(width_3), float(height_3)) - _S175;

#line 3976
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S176);
    float2 high_2 = min(low_2 + _S175, _S176);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S177 = int(low_2.x);

#line 3981
    int _S178 = int(low_2.y);

#line 3981
    int4 _S179 = int4(_S177, _S178, layer_1, int(0));
    int _S180 = int(high_2.x);

#line 3982
    int4 _S181 = int4(_S180, _S178, layer_1, int(0));
    int _S182 = int(high_2.y);

#line 3983
    int4 _S183 = int4(_S177, _S182, layer_1, int(0));
    int4 _S184 = int4(_S180, _S182, layer_1, int(0));
    float2 _S185 = float2(weight_2.x) ;

#line 3985
    return mix(mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S179)).xy), uint(((_S179)).z), uint(((_S179)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S181)).xy), uint(((_S181)).z), uint(((_S181)).w))).xy, _S185), mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S183)).xy), uint(((_S183)).z), uint(((_S183)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S184)).xy), uint(((_S184)).z), uint(((_S184)).w))).xy, _S185), float2(weight_2.y) );
}


#line 4013
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_7, KernelContext_0 thread* kernelContext_29)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_7 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4016
    float2 _S186 = probe_moments_0(index_6, - to_probe_0, kernelContext_29);

#line 4022
    float _S187 = _S186.x;

#line 4022
    float _S188 = max(_S186.y - _S187 * _S187, 0.0f);
    float behind_0 = to_surface_0 - _S187;
    float bound_0 = _S188 / (_S188 + behind_0 * behind_0);

#line 4024
    float _S189;
    if(to_surface_0 <= _S187)
    {

#line 4025
        _S189 = 1.0f;

#line 4025
    }
    else
    {

#line 4025
        _S189 = bound_0 * bound_0 * bound_0;

#line 4025
    }

#line 4025
    return _S189;
}


#line 4035
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_8, KernelContext_0 thread* kernelContext_30)
{

#line 4035
    float _S190 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_8, kernelContext_30);

    return max(_S190, 0.00009999999747379f);
}


#line 1061
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4068
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4095
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_2, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_9, KernelContext_0 thread* kernelContext_31)
{

#line 4096
    uint _S191 = probe_row_0(level_2, cell_2, kernelContext_31);


    GpuProbe_natural_0 stored_0 = kernelContext_31->probes_0[_S191];

#line 4099
    float _S192 = probe_weight_0(_S191, origin_1 + float3(cell_2) * spacing_0, world_position_12, normal_9, kernelContext_31);



    thread WeightedProbe_0 corner_2;

#line 4103
    float4 _S193 = float4(_S192) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S193;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S193;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S193;
    (&corner_2)->weight_3 = _S192;
    return corner_2;
}


#line 4079
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S194 = float4(t_1) ;

#line 4082
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S194);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S194);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S194);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4167
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_10, KernelContext_0 thread* kernelContext_32)
{

#line 4167
    float3 _S195 = float3(1.0f) ;

#line 4172
    float3 _S196 = float3(0.0f, 0.0f, 0.0f);

#line 4172
    float3 last_1 = max(float3(kernelContext_32->frame_0->probe_counts_0.xyz) - _S195, _S196);



    float3 origin_2 = kernelContext_32->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_32->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S196, last_1);
    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S197 = uint3(base_2);



    uint3 _S198 = uint3(min(base_2 + _S195, last_1));

#line 4192
    float _S199 = inv_0.x;

#line 4192
    float _S200;

#line 4192
    if(_S199 != 0.0f)
    {

#line 4192
        _S200 = 1.0f / _S199;

#line 4192
    }
    else
    {

#line 4192
        _S200 = 0.0f;

#line 4192
    }
    float _S201 = inv_0.y;

#line 4193
    float _S202;

#line 4193
    if(_S201 != 0.0f)
    {

#line 4193
        _S202 = 1.0f / _S201;

#line 4193
    }
    else
    {

#line 4193
        _S202 = 0.0f;

#line 4193
    }
    float _S203 = inv_0.z;

#line 4194
    float _S204;

#line 4194
    if(_S203 != 0.0f)
    {

#line 4194
        _S204 = 1.0f / _S203;

#line 4194
    }
    else
    {

#line 4194
        _S204 = 0.0f;

#line 4194
    }

#line 4192
    float3 spacing_1 = float3(_S200, _S202, _S204);

#line 4201
    uint _S205 = _S197.x;

#line 4201
    uint _S206 = _S197.y;

#line 4201
    uint _S207 = _S197.z;

#line 4201
    WeightedProbe_0 _S208 = probe_corner_0(level_3, uint3(_S205, _S206, _S207), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);
    uint _S209 = _S198.x;

#line 4202
    WeightedProbe_0 _S210 = probe_corner_0(level_3, uint3(_S209, _S206, _S207), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4202
    float _S211 = f_0.x;

#line 4202
    thread WeightedProbe_0 _S212 = _S208;

#line 4202
    thread WeightedProbe_0 _S213 = _S210;

#line 4202
    WeightedProbe_0 _S214 = lerp_probe_0(&_S212, &_S213, _S211);
    uint _S215 = _S198.y;

#line 4203
    WeightedProbe_0 _S216 = probe_corner_0(level_3, uint3(_S205, _S215, _S207), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4203
    WeightedProbe_0 _S217 = probe_corner_0(level_3, uint3(_S209, _S215, _S207), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4203
    thread WeightedProbe_0 _S218 = _S216;

#line 4203
    thread WeightedProbe_0 _S219 = _S217;

#line 4203
    WeightedProbe_0 _S220 = lerp_probe_0(&_S218, &_S219, _S211);

    uint _S221 = _S198.z;

#line 4205
    WeightedProbe_0 _S222 = probe_corner_0(level_3, uint3(_S205, _S206, _S221), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4205
    WeightedProbe_0 _S223 = probe_corner_0(level_3, uint3(_S209, _S206, _S221), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4205
    thread WeightedProbe_0 _S224 = _S222;

#line 4205
    thread WeightedProbe_0 _S225 = _S223;

#line 4205
    WeightedProbe_0 _S226 = lerp_probe_0(&_S224, &_S225, _S211);

#line 4205
    WeightedProbe_0 _S227 = probe_corner_0(level_3, uint3(_S205, _S215, _S221), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4205
    WeightedProbe_0 _S228 = probe_corner_0(level_3, uint3(_S209, _S215, _S221), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4205
    thread WeightedProbe_0 _S229 = _S227;

#line 4205
    thread WeightedProbe_0 _S230 = _S228;

#line 4205
    WeightedProbe_0 _S231 = lerp_probe_0(&_S229, &_S230, _S211);



    float _S232 = f_0.y;

#line 4209
    thread WeightedProbe_0 _S233 = _S214;

#line 4209
    thread WeightedProbe_0 _S234 = _S220;

#line 4209
    WeightedProbe_0 _S235 = lerp_probe_0(&_S233, &_S234, _S232);

#line 4209
    thread WeightedProbe_0 _S236 = _S226;

#line 4209
    thread WeightedProbe_0 _S237 = _S231;

#line 4209
    WeightedProbe_0 _S238 = lerp_probe_0(&_S236, &_S237, _S232);

    float _S239 = f_0.z;

#line 4211
    thread WeightedProbe_0 _S240 = _S235;

#line 4211
    thread WeightedProbe_0 _S241 = _S238;

#line 4211
    WeightedProbe_0 _S242 = lerp_probe_0(&_S240, &_S241, _S239);

    float4 basis_7 = float4(normal_10, 1.0f);
    return max(float3(dot(_S242.sh_0.sh_r_0, basis_7), dot(_S242.sh_0.sh_g_0, basis_7), dot(_S242.sh_0.sh_b_0, basis_7)) / float3(_S242.weight_3) , _S196);
}


#line 4301
float3 probe_irradiance_0(float3 world_position_14, float3 normal_11, KernelContext_0 thread* kernelContext_33)
{

#line 4309
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_33->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_33->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_33->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_33->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 4311
    float3 _S243 = probe_level_irradiance_0(level_4, world_position_14, normal_11, kernelContext_33);


    if(share_0 >= 1.0f)
    {

#line 4315
        return _S243;
    }

#line 4315
    float3 _S244 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_11, kernelContext_33);

    return _S244 * float3((1.0f - share_0))  + _S243 * float3(share_0) ;
}


#line 4763
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 4763
    float3 _S245 = float3(visibility_4) ;

#line 4769
    return min(float3(1.0f) , max(_S245, ((_S245 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S245 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S245));
}


#line 1034
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2530
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S246 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2538
    float kernel_0 = 0.0001984127011383f;

#line 2538
    int term_0 = int(6);

    for(;;)
    {

#line 2540
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2540
            break;
        }
        float _S247 = kernel_0 * _S246 + FOG_KERNEL_0[term_0];

#line 2540
        int term_1 = term_0 - int(1);

#line 2540
        kernel_0 = _S247;

#line 2540
        term_0 = term_1;

#line 2540
    }

#line 2547
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2557
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S248 = - d_0;

#line 2561
        float series_0 = 0.00833333376795053f;

#line 2561
        int term_2 = int(3);

        for(;;)
        {

#line 2563
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2563
                break;
            }
            float _S249 = series_0 * _S248 + FOG_RATIO_KERNEL_0[term_2];

#line 2563
            int term_3 = term_2 - int(1);

#line 2563
            series_0 = _S249;

#line 2563
            term_2 = term_3;

#line 2563
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2591
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2602
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2610
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4358
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4358
struct pixelInput_0
{
    float3 world_position_15 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    [[flat]] uint material_5 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
    float3 world_tangent_1 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_4 [[user(TEXCOORD_5)]];
};


#line 4805
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S250 [[stage_in]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 4805
    thread KernelContext_0 kernelContext_34;

#line 4805
    (&kernelContext_34)->draw_0 = draw_3;

#line 4805
    (&kernelContext_34)->visible_instances_0 = visible_instances_3;

#line 4805
    (&kernelContext_34)->instances_0 = instances_3;

#line 4805
    (&kernelContext_34)->meshes_0 = meshes_3;

#line 4805
    (&kernelContext_34)->frame_0 = frame_5;

#line 4805
    (&kernelContext_34)->vertices_0 = vertices_3;

#line 4805
    (&kernelContext_34)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4805
    (&kernelContext_34)->materials_0 = materials_3;

#line 4805
    (&kernelContext_34)->normal_textures_0 = normal_textures_3;

#line 4805
    (&kernelContext_34)->base_color_sampler_0 = base_color_sampler_3;

#line 4805
    (&kernelContext_34)->base_color_textures_0 = base_color_textures_3;

#line 4805
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_3;

#line 4805
    (&kernelContext_34)->specular_dfg_0 = specular_dfg_3;

#line 4805
    (&kernelContext_34)->lights_0 = lights_3;

#line 4805
    (&kernelContext_34)->ltc_matrix_0 = ltc_matrix_3;

#line 4805
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_3;

#line 4805
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_3;

#line 4805
    (&kernelContext_34)->contact_shadow_0 = contact_shadow_3;

#line 4805
    (&kernelContext_34)->probes_0 = probes_3;

#line 4805
    (&kernelContext_34)->probe_visibility_0 = probe_visibility_3;

#line 4817
    float3 vertex_normal_0 = normalize(_S250.world_normal_1);

#line 4822
    float2 motion_1 = motion_vector_0(_S250.clip_position_1, _S250.previous_clip_position_1);

#line 4838
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 4840
        float4 _S251 = occlusion_at_0(position_5.xy, &kernelContext_34);



        (&bent_0)->lit_0 = float4(_S251.yzw, 1.0f);


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

#line 4894
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 4894
        float4 _S252 = occlusion_at_0(position_5.xy, &kernelContext_34);


        float value_1 = _S252.x;

#line 4896
        thread FragmentOutput_0 occlusion_1;

#line 4905
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S250.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 4922
    thread GpuMaterial_natural_0 _S253 = (&kernelContext_34)->materials_0[_S250.material_5];

#line 4922
    float2 uv_3;

#line 4947
    if(((&_S253)->tiling_0) == 1U)
    {

#line 4947
        uv_3 = physical_tile_uv_0(_S250.world_position_15, vertex_normal_0, (&_S253)->tile_metres_0);

#line 4947
    }
    else
    {

#line 4947
        uv_3 = _S250.uv_2;

#line 4947
    }

#line 4947
    uint _S254 = normal_layer_0(&_S253);

#line 4947
    thread VertexOutput_0 _S255;

#line 4947
    (&_S255)->position_3 = position_5;

#line 4947
    (&_S255)->world_position_1 = _S250.world_position_15;

#line 4947
    (&_S255)->world_normal_0 = _S250.world_normal_1;

#line 4947
    (&_S255)->color_2 = _S250.color_3;

#line 4947
    (&_S255)->material_2 = _S250.material_5;

#line 4947
    (&_S255)->uv_0 = _S250.uv_2;

#line 4947
    (&_S255)->clip_position_0 = _S250.clip_position_1;

#line 4947
    (&_S255)->previous_clip_position_0 = _S250.previous_clip_position_1;

#line 4947
    (&_S255)->world_tangent_0 = _S250.world_tangent_1;

#line 4947
    (&_S255)->frame_3 = _S250.frame_4;

#line 4947
    float3 _S256 = shading_normal_of_0(_S254, (&_S253)->normal_scale_0, &_S255, vertex_normal_0, uv_3, &kernelContext_34);

#line 4954
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 4956
        float3 _S257 = float3(0.5f) ;

#line 4968
        (&normals_0)->lit_0 = float4(_S256 * _S257 + _S257, 1.0f);

#line 4974
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_34)->frame_0->camera_position_0.xyz - _S250.world_position_15);



    float3 _S258 = geometric_normal_of_0(_S250.world_position_15, vertex_normal_0);

#line 4983
    uint _S259 = base_color_layer_0(&_S253);

#line 4998
    float3 _S260 = float3(uv_3, float(_S259));
    float4 albedo_1 = _S250.color_3 * float4((&_S253)->base_color_0)  * (((&kernelContext_34)->base_color_textures_0).sample(((&kernelContext_34)->base_color_sampler_0), ((_S260)).xy, uint(((_S260)).z)));

#line 5005
    float metallic_1 = saturate((&_S253)->metallic_0);
    float roughness_2 = clamp((&_S253)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S261 = alpha_0 * alpha_0;

#line 5014
    float3 _S262 = albedo_1.xyz;

#line 5014
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S262, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S262 * float3((1.0f - metallic_1)) ;

#line 5021
    float _S263 = max(dot(_S256, to_eye_1), 0.00009999999747379f);

#line 5031
    float2 _S264 = position_5.xy;

#line 5031
    uint _S265 = froxel_of_0(_S264, (((float4(_S250.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_34);

#line 5031
    uint base_3 = _S265 * 17U;

#line 5036
    uint _S266 = min((&kernelContext_34)->cluster_lights_0[base_3], 16U);

#line 5036
    TableTap_0 _S267 = table_tap_0(_S263, roughness_2, &kernelContext_34);

#line 5036
    thread TableTap_0 _S268 = _S267;

#line 5036
    float2 _S269 = dfg_at_0(&_S268, &kernelContext_34);

#line 5045
    float _S270 = _S269.x;

#line 5045
    float _S271 = _S269.y;

#line 5045
    float3 _S272 = f0_2 * float3(_S270)  + float3(_S271) ;

#line 5051
    float3 _S273 = float3(0.0f, 0.0f, 0.0f);

#line 5051
    uint slot_0 = 0U;

#line 5051
    float3 direct_0 = _S273;

#line 5051
    float3 gloss_0 = _S273;

    for(;;)
    {

#line 5053
        if(slot_0 < _S266)
        {
        }
        else
        {

#line 5053
            break;
        }

#line 5053
        thread GpuLight_natural_0 _S274 = (&kernelContext_34)->lights_0[(&kernelContext_34)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 5053
        uint _S275 = (&_S274)->kind_0;

#line 5062
        bool _S276 = ((&_S274)->kind_0) == 0U;

#line 5062
        float3 to_light_7;

#line 5062
        float reach_2;

#line 5062
        if(_S276)
        {

#line 5062
            to_light_7 = normalize((float4((&_S274)->direction_0) ).xyz);

#line 5062
            reach_2 = 1.0f;

#line 5062
        }
        else
        {


            if(_S275 == 3U)
            {

#line 5067
                float4 _S277 = float4((&_S274)->position_0) ;

#line 5075
                float3 offset_0 = _S277.xyz - _S250.world_position_15;
                float distance_3 = length(offset_0);

                float _S278 = range_window_0(distance_3, _S277.w);

#line 5078
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5078
                reach_2 = _S278;

#line 5067
            }
            else
            {

#line 5067
                float4 _S279 = float4((&_S274)->position_0) ;

#line 5082
                float3 offset_1 = _S279.xyz - _S250.world_position_15;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S279.w);
                if(_S275 == 2U)
                {

#line 5086
                    float4 _S280 = float4((&_S274)->direction_0) ;

#line 5086
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S280.xyz, _S280.w, (&_S274)->cos_inner_0);

#line 5086
                }
                else
                {

#line 5086
                    reach_2 = reach_3;

#line 5086
                }

#line 5086
                to_light_7 = to_light_8;

#line 5067
            }

#line 5062
        }

#line 5095
        float n_dot_l_5 = dot(_S256, to_light_7);

#line 5095
        float3 specular_0;

#line 5095
        float diffuse_0;


        if(_S275 == 3U)
        {

#line 5108
            thread array<float3, int(4)> corners_2;

#line 5108
            rect_corners_0(&_S274, _S250.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S256, to_eye_1, _S263);

#line 5110
            thread array<float3, int(4)> _S281 = corners_2;

#line 5110
            float _S282 = ltc_irradiance_0(to_local_0, &_S281);

#line 5110
            thread TableTap_0 _S283 = _S267;

#line 5110
            float4 _S284 = ltc_at_0(&_S283, &kernelContext_34);

            matrix<float,int(3),int(3)>  _S285 = (((to_local_0) * (ltc_transform_0(_S284))));

#line 5112
            thread array<float3, int(4)> _S286 = corners_2;

#line 5112
            float _S287 = ltc_irradiance_0(_S285, &_S286);
            float3 _S288 = float3(_S287)  * _S272;

#line 5113
            diffuse_0 = _S282;

#line 5113
            specular_0 = _S288;

#line 5098
        }
        else
        {

#line 5118
            float _S289 = max(n_dot_l_5, 0.0f);

#line 5125
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5133
            float3 specular_1 = ggx_lobe_0(_S261, f0_2, _S289, _S263, max(dot(_S256, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S289) ;

#line 5133
            diffuse_0 = _S289;

#line 5133
            specular_0 = specular_1;

#line 5098
        }

#line 5098
        float3 specular_2;

#line 5141
        if((((&_S274)->flags_3) & 1U) != 0U)
        {

#line 5141
            specular_2 = _S273;

#line 5141
        }
        else
        {

#line 5141
            specular_2 = specular_0;

#line 5141
        }

#line 5141
        float reach_4;

#line 5159
        if(_S276)
        {

#line 5159
            float _S290 = sun_visibility_0(_S250.world_position_15, to_light_7, n_dot_l_5, _S258, _S264, &kernelContext_34);

#line 5159
            float _S291 = contact_at_0(_S264, &kernelContext_34);

#line 5159
            reach_4 = _S290 * _S291;

#line 5159
        }
        else
        {

#line 5171
            if(_S275 == 1U)
            {

#line 5171
                uint _S292 = (&_S274)->shadow_tile_0;

#line 5183
                if(((&_S274)->shadow_tile_0) <= 8U)
                {

#line 5183
                    float _S293 = point_visibility_0(&_S274, _S292, _S250.world_position_15, to_light_7, n_dot_l_5, _S258, _S264, &kernelContext_34);

#line 5183
                    reach_4 = reach_2 * _S293;

#line 5183
                }
                else
                {

#line 5183
                    reach_4 = reach_2;

#line 5183
                }

#line 5171
            }
            else
            {

#line 5171
                uint _S294 = (&_S274)->shadow_tile_0;

#line 5189
                if(((&_S274)->shadow_tile_0) < 14U)
                {

#line 5189
                    float _S295 = spot_visibility_0(&_S274, _S294, _S250.world_position_15, to_light_7, n_dot_l_5, _S258, _S264, &kernelContext_34);

#line 5189
                    reach_4 = reach_2 * _S295;

#line 5189
                }
                else
                {

#line 5189
                    reach_4 = reach_2;

#line 5189
                }

#line 5171
            }

#line 5159
        }

#line 5197
        float3 _S296 = (float4((&_S274)->color_0) ).xyz;

#line 5197
        float3 direct_1 = direct_0 + _S296 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S296 * (specular_2 * float3(reach_4) );

#line 5053
        slot_0 = slot_0 + 1U;

#line 5053
        direct_0 = direct_1;

#line 5053
        gloss_0 = gloss_1;

#line 5053
    }

#line 5212
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S270 + _S271);

#line 5212
    float4 _S297 = occlusion_at_0(_S264, &kernelContext_34);

#line 5231
    float occluded_0 = _S297.x;

#line 5240
    float3 bent_normal_0 = bent_normal_at_0(_S297, _S256);

#line 5263
    float3 _S298 = frame_5->ambient_0.xyz;

#line 5263
    float3 _S299 = sky_irradiance_0(bent_normal_0, &kernelContext_34);

#line 5263
    float3 _S300 = _S298 + _S299;

#line 5263
    float3 _S301 = probe_irradiance_0(_S250.world_position_15, bent_normal_0, &kernelContext_34);

#line 5299
    float3 lit_1 = diffuse_albedo_0 * ((_S300 + _S301) * multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) + direct_0) + gloss_2;

#line 5299
    float3 _S302 = emissive_of_0(&_S253);

#line 5335
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_34)->frame_0->fog_params_0.x, (&kernelContext_34)->frame_0->fog_params_0.y, (&kernelContext_34)->frame_0->camera_position_0.y - (&kernelContext_34)->frame_0->fog_params_0.z, _S250.world_position_15.y - (&kernelContext_34)->frame_0->fog_params_0.z, length((&kernelContext_34)->frame_0->camera_position_0.xyz - _S250.world_position_15)));


    thread FragmentOutput_0 output_2;



    (&output_2)->lit_0 = float4((lit_1 + _S302) * float3(fog_survives_0)  + (&kernelContext_34)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_1.w);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;
    return output_2;
}


#line 5379
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_12 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 5379
struct pixelInput_1
{
    float3 world_position_16 [[user(POSITION)]];
    float3 world_normal_2 [[user(NORMAL)]];
    float4 color_4 [[user(COLOR)]];
    [[flat]] uint material_6 [[user(TEXCOORD)]];
    float2 uv_4 [[user(TEXCOORD_1)]];
    float4 clip_position_2 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_2 [[user(TEXCOORD_3)]];
    float3 world_tangent_2 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_6 [[user(TEXCOORD_5)]];
};


#line 5422
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_1 _S303 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 5422
    thread KernelContext_0 kernelContext_35;

#line 5422
    (&kernelContext_35)->draw_0 = draw_4;

#line 5422
    (&kernelContext_35)->visible_instances_0 = visible_instances_4;

#line 5422
    (&kernelContext_35)->instances_0 = instances_4;

#line 5422
    (&kernelContext_35)->meshes_0 = meshes_4;

#line 5422
    (&kernelContext_35)->frame_0 = frame_7;

#line 5422
    (&kernelContext_35)->vertices_0 = vertices_4;

#line 5422
    (&kernelContext_35)->ambient_occlusion_0 = ambient_occlusion_4;

#line 5422
    (&kernelContext_35)->materials_0 = materials_4;

#line 5422
    (&kernelContext_35)->normal_textures_0 = normal_textures_4;

#line 5422
    (&kernelContext_35)->base_color_sampler_0 = base_color_sampler_4;

#line 5422
    (&kernelContext_35)->base_color_textures_0 = base_color_textures_4;

#line 5422
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_4;

#line 5422
    (&kernelContext_35)->specular_dfg_0 = specular_dfg_4;

#line 5422
    (&kernelContext_35)->lights_0 = lights_4;

#line 5422
    (&kernelContext_35)->ltc_matrix_0 = ltc_matrix_4;

#line 5422
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_4;

#line 5422
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_4;

#line 5422
    (&kernelContext_35)->contact_shadow_0 = contact_shadow_4;

#line 5422
    (&kernelContext_35)->probes_0 = probes_4;

#line 5422
    (&kernelContext_35)->probe_visibility_0 = probe_visibility_4;

#line 5427
    float3 vertex_normal_1 = normalize(_S303.world_normal_2);

#line 5427
    thread GpuMaterial_natural_0 _S304 = materials_4[_S303.material_6];

#line 5427
    float2 uv_5;

#line 5434
    if(((&_S304)->tiling_0) == 1U)
    {

#line 5434
        uv_5 = physical_tile_uv_0(_S303.world_position_16, vertex_normal_1, (&_S304)->tile_metres_0);

#line 5434
    }
    else
    {

#line 5434
        uv_5 = _S303.uv_4;

#line 5434
    }

#line 5434
    uint _S305 = base_color_layer_0(&_S304);

#line 5439
    float3 _S306 = float3(uv_5, float(_S305));


    thread RsmOutput_0 written_0;



    (&written_0)->albedo_2 = float4((_S303.color_4 * float4((&_S304)->base_color_0)  * (((&kernelContext_35)->base_color_textures_0).sample(((&kernelContext_35)->base_color_sampler_0), ((_S306)).xy, uint(((_S306)).z)))).xyz * float3((1.0f - saturate((&_S304)->metallic_0))) , 1.0f);

#line 5446
    float3 _S307 = float3(0.5f) ;
    (&written_0)->normal_12 = float4(vertex_normal_1 * _S307 + _S307, 1.0f);
    (&written_0)->world_0 = float4(_S303.world_position_16, 1.0f);
    return written_0;
}


#line 5449
struct vertexMain_Result_0
{
    float4 position_7 [[position]];
    float3 world_position_17 [[user(POSITION)]];
    float3 world_normal_3 [[user(NORMAL)]];
    float4 color_5 [[user(COLOR)]];
    uint material_7 [[user(TEXCOORD)]];
    float2 uv_6 [[user(TEXCOORD_1)]];
    float4 clip_position_3 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_3 [[user(TEXCOORD_3)]];
    float3 world_tangent_3 [[user(TEXCOORD_4)]];
    uint frame_8 [[user(TEXCOORD_5)]];
};


#line 5449
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 5449
    thread KernelContext_0 kernelContext_36;

#line 5449
    (&kernelContext_36)->draw_0 = draw_5;

#line 5449
    (&kernelContext_36)->visible_instances_0 = visible_instances_5;

#line 5449
    (&kernelContext_36)->instances_0 = instances_5;

#line 5449
    (&kernelContext_36)->meshes_0 = meshes_5;

#line 5449
    (&kernelContext_36)->frame_0 = frame_9;

#line 5449
    (&kernelContext_36)->vertices_0 = vertices_5;

#line 5449
    (&kernelContext_36)->ambient_occlusion_0 = ambient_occlusion_5;

#line 5449
    (&kernelContext_36)->materials_0 = materials_5;

#line 5449
    (&kernelContext_36)->normal_textures_0 = normal_textures_5;

#line 5449
    (&kernelContext_36)->base_color_sampler_0 = base_color_sampler_5;

#line 5449
    (&kernelContext_36)->base_color_textures_0 = base_color_textures_5;

#line 5449
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_5;

#line 5449
    (&kernelContext_36)->specular_dfg_0 = specular_dfg_5;

#line 5449
    (&kernelContext_36)->lights_0 = lights_5;

#line 5449
    (&kernelContext_36)->ltc_matrix_0 = ltc_matrix_5;

#line 5449
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_5;

#line 5449
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_5;

#line 5449
    (&kernelContext_36)->contact_shadow_0 = contact_shadow_5;

#line 5449
    (&kernelContext_36)->probes_0 = probes_5;

#line 5449
    (&kernelContext_36)->probe_visibility_0 = probe_visibility_5;

#line 5449
    GpuInstance_natural_0 device* _S308 = instances_5+visible_instances_5[draw_5->base_0 + instance_id_1];

#line 1820
    GpuMesh_0 mesh_3 = meshes_5[draw_5->mesh_0];

#line 1828
    bool _S309 = ((_S308->flags_0) & 2U) != 0U;

#line 1828
    uint base_vertex_3;
    if(_S309)
    {

#line 1829
        base_vertex_3 = _S308->base_vertex_0;

#line 1829
    }
    else
    {

#line 1829
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1829
    }

#line 1829
    MeshVertex_0 _S310 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_36);

#line 1829
    uint previous_base_0;

#line 1842
    if(_S309)
    {

#line 1842
        previous_base_0 = _S308->previous_base_vertex_0;

#line 1842
    }
    else
    {

#line 1842
        previous_base_0 = base_vertex_3;

#line 1842
    }

#line 1842
    float3 _S311 = load_position_0(index_8 + previous_base_0, &kernelContext_36);

#line 1842
    matrix<float,int(4),int(4)>  _S312 = matrix<float,int(4),int(4)> (_S308->transform_0.data_0[int(0)][int(0)], _S308->transform_0.data_0[int(1)][int(0)], _S308->transform_0.data_0[int(2)][int(0)], _S308->transform_0.data_0[int(3)][int(0)], _S308->transform_0.data_0[int(0)][int(1)], _S308->transform_0.data_0[int(1)][int(1)], _S308->transform_0.data_0[int(2)][int(1)], _S308->transform_0.data_0[int(3)][int(1)], _S308->transform_0.data_0[int(0)][int(2)], _S308->transform_0.data_0[int(1)][int(2)], _S308->transform_0.data_0[int(2)][int(2)], _S308->transform_0.data_0[int(3)][int(2)], _S308->transform_0.data_0[int(0)][int(3)], _S308->transform_0.data_0[int(1)][int(3)], _S308->transform_0.data_0[int(2)][int(3)], _S308->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S310.position_1, 1.0f)) * (_S312)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 1856
    matrix<float,int(3),int(3)>  _S313 = matrix<float,int(3),int(3)> (_S312[int(0)].xyz, _S312[int(1)].xyz, _S312[int(2)].xyz);

#line 1856
    (&output_3)->world_normal_0 = (((_S310.basis_1.normal_0) * (normal_basis_0(_S313))));

#line 1862
    (&output_3)->world_tangent_0 = (((_S310.basis_1.tangent_1) * (_S313)));

#line 1862
    thread TangentFrame_0 _S314 = _S310.basis_1;

#line 1862
    uint _S315 = frame_word_0(mesh_3.flags_1, &_S314);
    (&output_3)->frame_3 = _S315;

#line 1863
    float4 _S316;

#line 1870
    if(((&kernelContext_36)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1870
        _S316 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1870
    }
    else
    {

#line 1870
        _S316 = _S310.color_1;

#line 1870
    }

#line 1869
    (&output_3)->color_2 = _S316;

#line 1876
    (&output_3)->material_2 = _S308->material_0;
    (&output_3)->uv_0 = _S310.uv0_0;

#line 1883
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S311, 1.0f)) * (matrix<float,int(4),int(4)> (_S308->previous_transform_0.data_0[int(0)][int(0)], _S308->previous_transform_0.data_0[int(1)][int(0)], _S308->previous_transform_0.data_0[int(2)][int(0)], _S308->previous_transform_0.data_0[int(3)][int(0)], _S308->previous_transform_0.data_0[int(0)][int(1)], _S308->previous_transform_0.data_0[int(1)][int(1)], _S308->previous_transform_0.data_0[int(2)][int(1)], _S308->previous_transform_0.data_0[int(3)][int(1)], _S308->previous_transform_0.data_0[int(0)][int(2)], _S308->previous_transform_0.data_0[int(1)][int(2)], _S308->previous_transform_0.data_0[int(2)][int(2)], _S308->previous_transform_0.data_0[int(3)][int(2)], _S308->previous_transform_0.data_0[int(0)][int(3)], _S308->previous_transform_0.data_0[int(1)][int(3)], _S308->previous_transform_0.data_0[int(2)][int(3)], _S308->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S317 = output_3;

#line 1887
    thread vertexMain_Result_0 _S318;

#line 1887
    (&_S318)->position_7 = _S317.position_3;

#line 1887
    (&_S318)->world_position_17 = _S317.world_position_1;

#line 1887
    (&_S318)->world_normal_3 = _S317.world_normal_0;

#line 1887
    (&_S318)->color_5 = _S317.color_2;

#line 1887
    (&_S318)->material_7 = _S317.material_2;

#line 1887
    (&_S318)->uv_6 = _S317.uv_0;

#line 1887
    (&_S318)->clip_position_3 = _S317.clip_position_0;

#line 1887
    (&_S318)->previous_clip_position_3 = _S317.previous_clip_position_0;

#line 1887
    (&_S318)->world_tangent_3 = _S317.world_tangent_0;

#line 1887
    (&_S318)->frame_8 = _S317.frame_3;

#line 1887
    return _S318;
}

