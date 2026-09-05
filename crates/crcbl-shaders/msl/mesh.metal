#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2682 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2677
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 3679
constant array<float3, int(2)> CASCADE_TINTS_0 = { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) };

#line 3162
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2949
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 3009
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 3024
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 3052
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1283
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1927
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1927
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


#line 1933
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1933
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


#line 1326
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


#line 1337
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1340
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1791
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1914
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1914
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1916
        word_4 = 1U;

#line 1916
    }
    else
    {

#line 1916
        word_4 = 0U;

#line 1916
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1920
        word_4 = word_4 | 2U;

#line 1920
    }

#line 1919
    return word_4;
}


#line 1919
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 2035
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 2035
    thread KernelContext_0 kernelContext_2;

#line 2035
    (&kernelContext_2)->draw_0 = draw_1;

#line 2035
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 2035
    (&kernelContext_2)->instances_0 = instances_1;

#line 2035
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 2035
    (&kernelContext_2)->frame_0 = frame_1;

#line 2035
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 2035
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2035
    (&kernelContext_2)->materials_0 = materials_1;

#line 2035
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 2035
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 2035
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 2035
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 2035
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 2035
    (&kernelContext_2)->lights_0 = lights_1;

#line 2035
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 2035
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 2035
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 2035
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 2035
    (&kernelContext_2)->probes_0 = probes_1;

#line 2035
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 2035
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 2038
    uint base_vertex_2;

#line 2044
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 2044
        base_vertex_2 = _S7->base_vertex_0;

#line 2044
    }
    else
    {

#line 2044
        base_vertex_2 = mesh_2.base_vertex_1;

#line 2044
    }

#line 2044
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 2044
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 2044
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 2047
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 2068
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 2068
    thread KernelContext_0 kernelContext_3;

#line 2068
    (&kernelContext_3)->draw_0 = draw_2;

#line 2068
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 2068
    (&kernelContext_3)->instances_0 = instances_2;

#line 2068
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 2068
    (&kernelContext_3)->frame_0 = frame_2;

#line 2068
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 2068
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 2068
    (&kernelContext_3)->materials_0 = materials_2;

#line 2068
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 2068
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 2068
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 2068
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 2068
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 2068
    (&kernelContext_3)->lights_0 = lights_2;

#line 2068
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 2068
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 2068
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 2068
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 2068
    (&kernelContext_3)->probes_0 = probes_2;

#line 2068
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 2068
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 5084
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 5086
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4960
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4960
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4966
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4694
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4698
    float _S16 = axis_0.y;

#line 4698
    bool _S17;

#line 4698
    if(_S15 >= _S16)
    {

#line 4698
        _S17 = _S15 >= (axis_0.z);

#line 4698
    }
    else
    {

#line 4698
        _S17 = false;

#line 4698
    }

#line 4698
    float2 planar_0;

#line 4698
    if(_S17)
    {

#line 4698
        planar_0 = world_position_0.zy;

#line 4698
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4702
            planar_0 = world_position_0.xz;

#line 4702
        }
        else
        {

#line 4702
            planar_0 = world_position_0.xy;

#line 4702
        }

#line 4698
    }

#line 4710
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1044
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) & 65535U;
}


#line 1123
bool alpha_masked_0(const GpuMaterial_natural_0 thread* material_2, float alpha_0)
{

#line 1123
    bool _S18;

    if(((material_2->flags_2) & 1U) != 0U)
    {

#line 1125
        _S18 = alpha_0 < (material_2->alpha_cutoff_0);

#line 1125
    }
    else
    {

#line 1125
        _S18 = false;

#line 1125
    }

#line 1125
    return _S18;
}


#line 1158
float3 double_sided_normal_0(const GpuMaterial_natural_0 thread* material_3, float3 normal_2, bool front_facing_0)
{

#line 1158
    bool _S19;

    if(((material_3->flags_2) & 2U) != 0U)
    {

#line 1160
        _S19 = !front_facing_0;

#line 1160
    }
    else
    {

#line 1160
        _S19 = false;

#line 1160
    }

#line 1160
    float3 _S20;

#line 1160
    if(_S19)
    {

#line 1160
        _S20 = - normal_2;

#line 1160
    }
    else
    {

#line 1160
        _S20 = normal_2;

#line 1160
    }

#line 1160
    return _S20;
}


#line 1059
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_4)
{
    return (material_4->color_normal_pages_0) >> 16U;
}


#line 4731
float3 orthonormal_tangent_0(float3 normal_3)
{
    float _S21 = normal_3.z;

#line 4733
    float sign_z_0;

#line 4733
    if(_S21 >= 0.0f)
    {

#line 4733
        sign_z_0 = 1.0f;

#line 4733
    }
    else
    {

#line 4733
        sign_z_0 = -1.0f;

#line 4733
    }
    float a_0 = -1.0f / (sign_z_0 + _S21);
    float _S22 = normal_3.x;

#line 4735
    float _S23 = sign_z_0 * _S22;

#line 4735
    return float3(1.0f + _S23 * _S22 * a_0, _S23 * normal_3.y * a_0, - sign_z_0 * _S22);
}


#line 4785
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_4)
{
    float _S24 = duvdy_0.y;

#line 4787
    float _S25 = duvdx_0.y;

#line 4787
    float winding_0;
    if((duvdx_0.x * _S24 - duvdy_0.x * _S25) < 0.0f)
    {

#line 4788
        winding_0 = -1.0f;

#line 4788
    }
    else
    {

#line 4788
        winding_0 = 1.0f;

#line 4788
    }
    float3 tangent_2 = (float3(_S24)  * dpdx_0 - float3(_S25)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_4;

#line 4797
    float3 tangent_3 = tangent_2 - normal_4 * float3(dot(normal_4, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 4798
    float3 _S26;

#line 4807
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 4807
        _S26 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 4807
    }
    else
    {

#line 4807
        _S26 = orthonormal_tangent_0(normal_4);

#line 4807
    }

#line 4807
    (&basis_4)->tangent_1 = _S26;

    (&basis_4)->bitangent_0 = cross(normal_4, _S26);
    return basis_4;
}


#line 1798
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_1;
    float3 world_normal_0;
    float4 color_2;
    [[flat]] uint material_5;
    float2 uv_0;
    float4 clip_position_0;
    float4 previous_clip_position_0;
    float3 world_tangent_0;
    [[flat]] uint frame_3;
};


#line 4867
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_5, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 4879
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_5;
    }

    thread TangentFrame_0 basis_5;

#line 4889
    uint _S27 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4898
        (&basis_5)->normal_0 = normal_5;
        float3 tangent_4 = input_0->world_tangent_0 - normal_5 * float3(dot(normal_5, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4900
        float3 _S28;

#line 4905
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4905
            _S28 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4905
        }
        else
        {

#line 4905
            _S28 = orthonormal_tangent_0(normal_5);

#line 4905
        }

#line 4905
        (&basis_5)->tangent_1 = _S28;

#line 4911
        float3 _S29 = cross((&basis_5)->normal_0, _S28);

#line 4911
        float _S30;
        if((_S27 & 2U) != 0U)
        {

#line 4912
            _S30 = -1.0f;

#line 4912
        }
        else
        {

#line 4912
            _S30 = 1.0f;

#line 4912
        }

#line 4911
        (&basis_5)->bitangent_0 = _S29 * float3(_S30) ;

#line 4890
    }
    else
    {

#line 4916
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_5);

#line 4890
    }

#line 4920
    float3 _S31 = float3(uv_1, float(layer_0));
    float3 _S32 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S31)).xy, uint(((_S31)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4921
    thread float3 tangent_space_0 = _S32;
    tangent_space_0.xy = _S32.xy * float2(normal_scale_1) ;

#line 4927
    float3 _S33 = normalize(tangent_space_0);

#line 4927
    tangent_space_0 = _S33;
    return normalize(float3(_S33.x)  * (&basis_5)->tangent_1 + float3(_S33.y)  * (&basis_5)->bitangent_0 + float3(_S33.z)  * (&basis_5)->normal_0);
}


#line 2817
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2828
    float3 _S34;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2829
        _S34 = - facet_1;

#line 2829
    }
    else
    {

#line 2829
        _S34 = facet_1;

#line 2829
    }

#line 2829
    return _S34;
}


#line 2222
float specular_aa_kernel_0(float3 normal_6)
{
    float3 dndx_0 = dfdx(normal_6);
    float3 dndy_0 = dfdy(normal_6);


    return min(2.0f * (0.25f * (dot(dndx_0, dndx_0) + dot(dndy_0, dndy_0))), 0.18000000715255737f);
}


#line 4116
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S36 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S37 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S38 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 4126
    uint _S39 = uint(pixel_0.x) / _S38;

#line 4126
    uint _S40 = min(_S39, _S35 - 1U);
    uint _S41 = uint(pixel_0.y) / _S38;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4137
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S37 - 1U))) * _S36 + min(_S41, _S36 - 1U)) * _S35 + _S40;
}


#line 2249
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2270
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 2270
    texture2d<float, access::sample> _S42 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S42).get_width(0)),(*((&height_1)) = (_S42).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2276
    float2 _S43 = float2(1.0f) ;
    float2 _S44 = extent_1 - _S43;

#line 2277
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S44);
    float2 high_1 = min(low_1 + _S43, _S44);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2295
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 2307
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S45 = tap_1->lo_0.x;

#line 2309
    int _S46 = tap_1->lo_0.y;

#line 2309
    int3 _S47 = int3(_S45, _S46, int(0));
    int _S48 = tap_1->hi_0.x;

#line 2310
    int3 _S49 = int3(_S48, _S46, int(0));
    float2 _S50 = float2(tap_1->weight_0.x) ;
    int _S51 = tap_1->hi_0.y;

#line 2312
    int3 _S52 = int3(_S45, _S51, int(0));
    int3 _S53 = int3(_S48, _S51, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S47)).xy), uint(((_S47)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), _S50), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S52)).xy), uint(((_S52)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S53)).xy), uint(((_S53)).z)))), _S50), float2(tap_1->weight_0.y) );
}


#line 4067
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 4083
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 4095
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 4102
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2636
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2636
    float4 _S54 = float4(light_0->tangent_0) ;

    float3 _S55 = _S54.xyz;

#line 2638
    float3 across_0 = _S55 * float3(_S54.w) ;

#line 2638
    float4 _S56 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S55, _S56.xyz) * float3(_S56.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S57 = centre_0 - across_0;

#line 2641
    (*corners_0)[int(0)] = _S57 - down_0;
    float3 _S58 = centre_0 + across_0;

#line 2642
    (*corners_0)[int(1)] = _S58 - down_0;
    (*corners_0)[int(2)] = _S58 + down_0;
    (*corners_0)[int(3)] = _S57 + down_0;
    return;
}


#line 2394
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_7, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_7 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2397
    float3 seed_0;
    if((abs(normal_7.z)) < 0.89999997615814209f)
    {

#line 2398
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2398
    }
    else
    {

#line 2398
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2398
    }

#line 2398
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2399
        tangent_5 = across_1 / float3(span_0) ;

#line 2399
    }
    else
    {

#line 2399
        tangent_5 = normalize(cross(seed_0, normal_7));

#line 2399
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_7, tangent_5), normal_7);
}


#line 2375
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2465
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2465
    float3 _S59 = polygon_0->corner_0[int(0)];

#line 2465
    float3 _S60 = polygon_0->corner_0[int(1)];

#line 2465
    float3 _S61 = polygon_0->corner_0[int(2)];

#line 2465
    float3 _S62 = polygon_0->corner_0[int(3)];

#line 2471
    float3 _S63 = float3(0.0f, 0.0f, 0.0f);


    float _S64 = polygon_0->corner_0[int(0)].z;

#line 2474
    int count_1;

#line 2474
    if(_S64 > 0.0f)
    {

#line 2474
        count_1 = int(1);

#line 2474
    }
    else
    {

#line 2474
        count_1 = int(0);

#line 2474
    }
    float _S65 = _S60.z;

#line 2475
    int _S66;

#line 2475
    if(_S65 > 0.0f)
    {

#line 2475
        _S66 = int(2);

#line 2475
    }
    else
    {

#line 2475
        _S66 = int(0);

#line 2475
    }

#line 2475
    int config_0 = count_1 + _S66;
    float _S67 = _S61.z;

#line 2476
    if(_S67 > 0.0f)
    {

#line 2476
        count_1 = int(4);

#line 2476
    }
    else
    {

#line 2476
        count_1 = int(0);

#line 2476
    }

#line 2476
    int config_1 = config_0 + count_1;
    float _S68 = _S62.z;

#line 2477
    if(_S68 > 0.0f)
    {

#line 2477
        count_1 = int(8);

#line 2477
    }
    else
    {

#line 2477
        count_1 = int(0);

#line 2477
    }

#line 2477
    int config_2 = config_1 + count_1;

#line 2477
    float3 l0_0;

#line 2477
    float3 l1_0;

#line 2477
    float3 l2_0;

#line 2477
    float3 l3_0;

#line 2477
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2480
        float3 _S69 = float3(_S64) ;


        float3 _S70 = float3(- _S65)  * _S59 + _S69 * _S60;
        float3 _S71 = float3(- _S68)  * _S59 + _S69 * _S62;

#line 2484
        count_1 = int(3);

#line 2484
        l0_0 = _S59;

#line 2484
        l1_0 = _S70;

#line 2484
        l2_0 = _S71;

#line 2484
        l3_0 = _S62;

#line 2484
        l4_0 = _S63;

#line 2480
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2486
            float3 _S72 = float3(_S65) ;


            float3 _S73 = float3(- _S64)  * _S60 + _S72 * _S59;
            float3 _S74 = float3(- _S67)  * _S60 + _S72 * _S61;

#line 2490
            count_1 = int(3);

#line 2490
            l0_0 = _S73;

#line 2490
            l1_0 = _S60;

#line 2490
            l2_0 = _S74;

#line 2490
            l3_0 = _S62;

#line 2490
            l4_0 = _S63;

#line 2486
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S75 = float3(- _S67)  * _S60 + float3(_S65)  * _S61;
                float3 _S76 = float3(- _S68)  * _S59 + float3(_S64)  * _S62;

#line 2496
                count_1 = int(4);

#line 2496
                l0_0 = _S59;

#line 2496
                l1_0 = _S60;

#line 2496
                l2_0 = _S75;

#line 2496
                l3_0 = _S76;

#line 2496
                l4_0 = _S63;

#line 2492
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2498
                    float3 _S77 = float3(_S67) ;


                    float3 _S78 = float3(- _S68)  * _S61 + _S77 * _S62;
                    float3 _S79 = float3(- _S65)  * _S61 + _S77 * _S60;

#line 2502
                    count_1 = int(3);

#line 2502
                    l0_0 = _S78;

#line 2502
                    l1_0 = _S79;

#line 2502
                    l2_0 = _S61;

#line 2502
                    l3_0 = _S62;

#line 2502
                    l4_0 = _S63;

#line 2498
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S80 = float3(- _S64)  * _S60 + float3(_S65)  * _S59;
                        float3 _S81 = float3(- _S68)  * _S61 + float3(_S67)  * _S62;

#line 2508
                        count_1 = int(4);

#line 2508
                        l0_0 = _S80;

#line 2508
                        l1_0 = _S60;

#line 2508
                        l2_0 = _S61;

#line 2508
                        l3_0 = _S81;

#line 2508
                        l4_0 = _S63;

#line 2504
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2510
                            float3 _S82 = float3(- _S68) ;


                            float3 _S83 = _S82 * _S59 + float3(_S64)  * _S62;
                            float3 _S84 = _S82 * _S61 + float3(_S67)  * _S62;

#line 2514
                            count_1 = int(5);

#line 2514
                            l0_0 = _S59;

#line 2514
                            l1_0 = _S60;

#line 2514
                            l2_0 = _S61;

#line 2514
                            l3_0 = _S84;

#line 2514
                            l4_0 = _S83;

#line 2510
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2516
                                float3 _S85 = float3(_S68) ;


                                float3 _S86 = float3(- _S64)  * _S62 + _S85 * _S59;
                                float3 _S87 = float3(- _S67)  * _S62 + _S85 * _S61;

#line 2520
                                count_1 = int(3);

#line 2520
                                l0_0 = _S86;

#line 2520
                                l1_0 = _S87;

#line 2520
                                l2_0 = _S62;

#line 2520
                                l3_0 = _S62;

#line 2520
                                l4_0 = _S63;

#line 2516
                            }
                            else
                            {

#line 2523
                                if(config_2 == int(9))
                                {

                                    float3 _S88 = float3(- _S65)  * _S59 + float3(_S64)  * _S60;
                                    float3 _S89 = float3(- _S67)  * _S62 + float3(_S68)  * _S61;

#line 2527
                                    count_1 = int(4);

#line 2527
                                    l0_0 = _S59;

#line 2527
                                    l1_0 = _S88;

#line 2527
                                    l2_0 = _S89;

#line 2527
                                    l3_0 = _S62;

#line 2527
                                    l4_0 = _S63;

#line 2523
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S90 = float3(- _S68)  * _S61 + float3(_S67)  * _S62;
                                        float3 _S91 = float3(- _S67)  * _S60 + float3(_S65)  * _S61;

#line 2534
                                        count_1 = int(5);

#line 2534
                                        l0_0 = _S59;

#line 2534
                                        l1_0 = _S60;

#line 2534
                                        l2_0 = _S91;

#line 2534
                                        l3_0 = _S90;

#line 2534
                                        l4_0 = _S62;

#line 2529
                                    }
                                    else
                                    {

#line 2536
                                        if(config_2 == int(12))
                                        {

                                            float3 _S92 = float3(- _S65)  * _S61 + float3(_S67)  * _S60;
                                            float3 _S93 = float3(- _S64)  * _S62 + float3(_S68)  * _S59;

#line 2540
                                            count_1 = int(4);

#line 2540
                                            l0_0 = _S93;

#line 2540
                                            l1_0 = _S92;

#line 2540
                                            l2_0 = _S61;

#line 2540
                                            l3_0 = _S62;

#line 2540
                                            l4_0 = _S63;

#line 2536
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S94 = float3(- _S67)  * _S60 + float3(_S65)  * _S61;
                                                float3 _S95 = float3(- _S65)  * _S59 + float3(_S64)  * _S60;

#line 2548
                                                count_1 = int(5);

#line 2548
                                                l0_0 = _S59;

#line 2548
                                                l1_0 = _S95;

#line 2548
                                                l2_0 = _S94;

#line 2548
                                                l3_0 = _S61;

#line 2548
                                                l4_0 = _S62;

#line 2542
                                            }
                                            else
                                            {

#line 2550
                                                if(config_2 == int(14))
                                                {

#line 2550
                                                    float3 _S96 = float3(- _S64) ;


                                                    float3 _S97 = _S96 * _S62 + float3(_S68)  * _S59;
                                                    float3 _S98 = _S96 * _S60 + float3(_S65)  * _S59;

#line 2554
                                                    count_1 = int(5);

#line 2554
                                                    l0_0 = _S98;

#line 2554
                                                    l1_0 = _S97;

#line 2550
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2556
                                                        count_1 = int(4);

#line 2556
                                                    }
                                                    else
                                                    {

#line 2556
                                                        count_1 = int(0);

#line 2556
                                                    }

#line 2556
                                                    l0_0 = _S59;

#line 2556
                                                    l1_0 = _S63;

#line 2550
                                                }

#line 2471
                                                float3 _S99 = l1_0;

#line 2471
                                                l1_0 = _S60;

#line 2471
                                                l2_0 = _S61;

#line 2471
                                                l3_0 = _S62;

#line 2471
                                                l4_0 = _S99;

#line 2542
                                            }

#line 2536
                                        }

#line 2529
                                    }

#line 2523
                                }

#line 2516
                            }

#line 2510
                        }

#line 2504
                    }

#line 2498
                }

#line 2492
            }

#line 2486
        }

#line 2480
    }

#line 2564
    if(count_1 <= int(3))
    {

#line 2564
        l3_0 = l0_0;

#line 2564
        l4_0 = l0_0;

#line 2564
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2569
            l4_0 = l0_0;

#line 2569
        }

#line 2564
    }

#line 2574
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2437
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2443
    float weight_1;

#line 2448
    if(cosine_0 > 0.0f)
    {

#line 2448
        weight_1 = fit_0;

#line 2448
    }
    else
    {

#line 2448
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2448
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2594
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2596
    int corner_1 = int(0);
    for(;;)
    {

#line 2597
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2597
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2597
        corner_1 = corner_1 + int(1);

#line 2597
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2602
    thread LtcPolygon_0 _S100 = polygon_1;

#line 2602
    LtcPolygon_0 _S101 = ltc_clip_0(&_S100);
    polygon_1 = _S101;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2606
    int at_2 = int(0);

    for(;;)
    {

#line 2608
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2608
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2608
        at_2 = at_2 + int(1);

#line 2608
    }

#line 2615
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2615
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2616
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2616
    }
    else
    {

#line 2616
        sum_1 = sum_0;

#line 2616
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2620
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2620
    }

#line 2627
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2323
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S102 = tap_2->lo_0.x;

#line 2325
    int _S103 = tap_2->lo_0.y;

#line 2325
    int3 _S104 = int3(_S102, _S103, int(0));
    int _S105 = tap_2->hi_0.x;

#line 2326
    int3 _S106 = int3(_S105, _S103, int(0));
    float4 _S107 = float4(tap_2->weight_0.x) ;
    int _S108 = tap_2->hi_0.y;

#line 2328
    int3 _S109 = int3(_S102, _S108, int(0));
    int3 _S110 = int3(_S105, _S108, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S104)).xy), uint(((_S104)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), _S107), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S109)).xy), uint(((_S109)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S110)).xy), uint(((_S110)).z))), _S107), float4(tap_2->weight_0.y) );
}


#line 2410
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2147
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2154
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2161
    float _S111 = 1.0f - alpha2_0;

#line 2166
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S111 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S111 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3239
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3239
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3299
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3271
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2868
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3226
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_13)
{

#line 3226
    uint _S112;

    if(uint(pixel_1.x) < (kernelContext_13->frame_0->shadow_filter_0.z))
    {

#line 3228
        _S112 = kernelContext_13->frame_0->shadow_filter_0.x;

#line 3228
    }
    else
    {

#line 3228
        _S112 = kernelContext_13->frame_0->shadow_filter_0.y;

#line 3228
    }

#line 3228
    return _S112;
}


#line 3251
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3251
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 349
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3321
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S113 = spoke_0.x;

#line 3326
    float _S114 = rotation_0.x;

#line 3326
    float _S115 = spoke_0.y;

#line 3326
    float _S116 = rotation_0.y;


    float _S117 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S113 * _S114 - _S115 * _S116, _S113 * _S116 + _S115 * _S114) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3329
    return _S117;
}


#line 3409
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_17)
{

#line 3409
    float4 _S118 = atlas_rect_1(tile_2, kernelContext_17);


    if(atlas_rect_is_empty_0(_S118))
    {
        return 1.0f;
    }

#line 3414
    float2 _S119 = atlas_step_1(_S118, kernelContext_17);

#line 3414
    int y_1 = int(-1);

#line 3414
    float visibility_0 = 0.0f;

#line 3419
    for(;;)
    {

#line 3419
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3419
            break;
        }

#line 3419
        int x_0 = int(-1);

        for(;;)
        {

#line 3421
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3421
                break;
            }

#line 3421
            float _S120 = tile_tap_0(_S118, _S119, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_17);

            float visibility_1 = visibility_0 + _S120;

#line 3421
            x_0 = x_0 + int(1);

#line 3421
            visibility_0 = visibility_1;

#line 3421
        }

#line 3419
        y_1 = y_1 + int(1);

#line 3419
    }

#line 3427
    return visibility_0 / 9.0f;
}


#line 3184
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3351
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_18)
{
    float2 _S121 = shadow_rotation_0(pixel_3);

#line 3353
    float4 _S122 = atlas_rect_1(tile_3, kernelContext_18);

    if(atlas_rect_is_empty_0(_S122))
    {
        return 1.0f;
    }

#line 3357
    float2 _S123 = atlas_step_1(_S122, kernelContext_18);

#line 3357
    uint spot_0 = 0U;

#line 3357
    float probe_0 = 0.0f;

#line 3362
    for(;;)
    {

#line 3362
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3362
            break;
        }

#line 3362
        float _S124 = tile_tap_0(_S122, _S123, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S121, reference_2, kernelContext_18);

        float probe_1 = probe_0 + _S124;

#line 3362
        spot_0 = spot_0 + 1U;

#line 3362
        probe_0 = probe_1;

#line 3362
    }

#line 3371
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3377
    uint index_2 = 0U;

#line 3377
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3381
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3381
            break;
        }

#line 3381
        float _S125 = tile_tap_0(_S122, _S123, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S121, reference_2, kernelContext_18);

        float visibility_3 = visibility_2 + _S125;

#line 3381
        index_2 = index_2 + 1U;

#line 3381
        visibility_2 = visibility_3;

#line 3381
    }

#line 3386
    return visibility_2 / 32.0f;
}


#line 3462
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_19)
{
    float2 texel_1 = kernelContext_19->frame_0->shadow_params_0.xy;

#line 3464
    float4 _S126 = atlas_rect_0(cascade_0, kernelContext_19);

#line 3464
    float2 _S127 = atlas_step_0(_S126, kernelContext_19);


    float2 _S128 = float2(0.5f, 0.5f) * _S127;


    float2 _S129 = float2(1.0f, 1.0f);

#line 3470
    float2 _S130 = _S129 / texel_1;

#line 3470
    uint index_3 = 0U;

#line 3470
    float sum_2 = 0.0f;

#line 3470
    float found_0 = 0.0f;



    for(;;)
    {

#line 3474
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3474
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S131 = spoke_1.x;

#line 3477
        float _S132 = rotation_1.x;

#line 3477
        float _S133 = spoke_1.y;

#line 3477
        float _S134 = rotation_1.y;

#line 3485
        int3 _S135 = int3(int2(min(atlas_uv_0(_S126, clamp(tile_uv_4 + float2(_S131 * _S132 - _S133 * _S134, _S131 * _S134 + _S133 * _S132) * _S127, _S128, float2(1.0f)  - _S128)) * _S130, _S130 - _S129)), int(0));

#line 3485
        float depth_1 = ((kernelContext_19->shadow_atlas_0).read(vec<uint,2>(((_S135)).xy), uint(((_S135)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3489
            sum_2 = sum_2 + depth_1;

#line 3489
            found_0 = found_1;

#line 3486
        }

#line 3474
        index_3 = index_3 + 1U;

#line 3474
    }

#line 3493
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3504
    float _S136 = 2.0f * kernelContext_19->frame_0->cascade_far_0[cascade_0];

#line 3504
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S136 + 40.0f);

#line 3504
    float _S137 = tile_texels_0(_S126, kernelContext_19);

    return clamp(separation_0 * 0.01999999955296516f / (_S136 / _S137), 2.0f, 8.0f);
}


#line 3558
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_20)
{

#line 3559
    float4 _S138 = atlas_rect_0(cascade_1, kernelContext_20);

#line 3593
    if(atlas_rect_is_empty_0(_S138))
    {


        return 1.0f;
    }
    float _S139 = 2.0f * kernelContext_20->frame_0->cascade_far_0[cascade_1];

#line 3599
    float _S140 = tile_texels_0(_S138, kernelContext_20);

#line 3599
    float texel_world_0 = _S139 / _S140;

#line 3606
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3610
    bool _S141;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3611
        _S141 = true;

#line 3611
    }
    else
    {

#line 3611
        _S141 = (ndc_0.z) <= 0.0f;

#line 3611
    }

#line 3611
    if(_S141)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3621
    uint _S142 = shadow_filter_mode_0(pixel_4, kernelContext_20);

#line 3638
    if(_S142 == 2U)
    {

#line 3638
        float _S143 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_20);

        return _S143;
    }
    if(_S142 == 1U)
    {

#line 3642
        float _S144 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_20);



        return _S144;
    }

    float _S145 = ndc_0.z;

#line 3649
    float _S146 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S145, shadow_rotation_0(pixel_4), kernelContext_20);

#line 3649
    float _S147 = tile_pcf_0(cascade_1, tile_uv_5, _S145, pixel_4, _S146, kernelContext_20);
    return _S147;
}


#line 3729
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_21)
{
    uint cascade_2;

#line 3731
    bool covered_0;

#line 3740
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3752
    float eye_distance_0 = length(world_position_5 - kernelContext_21->frame_0->camera_position_0.xyz);

#line 3752
    uint index_4 = 0U;

#line 3760
    for(;;)
    {

#line 3760
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3760
            covered_0 = false;

#line 3760
            cascade_2 = 1U;

#line 3760
            break;
        }
        if(eye_distance_0 < kernelContext_21->frame_0->cascade_far_0[index_4])
        {

#line 3762
            covered_0 = true;

#line 3762
            cascade_2 = index_4;



            break;
        }

#line 3760
        index_4 = index_4 + 1U;

#line 3760
    }

#line 3769
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 3769
    }

#line 3769
    float _S148 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3776
    uint _S149 = cascade_2 + 1U;

#line 3776
    if(_S149 >= 2U)
    {



        return _S148;
    }

#line 3789
    float band_0 = kernelContext_21->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_21->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S148;
    }

#line 3797
    float _S150 = cascade_visibility_0(_S149, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3808
    return mix(_S148, _S150, blend_0);
}


#line 4996
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_22)
{

#line 4996
    texture2d<float, access::sample> _S151 = kernelContext_22->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S151).get_width(0)),(*((&height_2)) = (_S151).get_height(0));

    int3 _S152 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 5002
    return ((kernelContext_22->contact_shadow_0).read(vec<uint,2>(((_S152)).xy), uint(((_S152)).z)).x);
}


#line 3701
float3 cascade_tint_0(uint cascade_3, float blend_1)
{
    if(cascade_3 >= 2U)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }
    uint _S153 = cascade_3 + 1U;

#line 3707
    if(_S153 >= 2U)
    {


        return CASCADE_TINTS_0[cascade_3];
    }
    return mix(CASCADE_TINTS_0[cascade_3], CASCADE_TINTS_0[_S153], float3(blend_1) );
}


#line 4019
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S154 = axis_2.x;

#line 4022
    float _S155 = axis_2.y;

#line 4022
    bool _S156;

#line 4022
    if(_S154 >= _S155)
    {

#line 4022
        _S156 = _S154 >= (axis_2.z);

#line 4022
    }
    else
    {

#line 4022
        _S156 = false;

#line 4022
    }

#line 4022
    uint _S157;

#line 4022
    if(_S156)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 4024
            _S157 = 0U;

#line 4024
        }
        else
        {

#line 4024
            _S157 = 1U;

#line 4024
        }

#line 4024
        return _S157;
    }
    if(_S155 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 4028
            _S157 = 2U;

#line 4028
        }
        else
        {

#line 4028
            _S157 = 3U;

#line 4028
        }

#line 4028
        return _S157;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 4030
        _S157 = 4U;

#line 4030
    }
    else
    {

#line 4030
        _S157 = 5U;

#line 4030
    }

#line 4030
    return _S157;
}


#line 336
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 3915
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_23)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 3918
    float4 _S158 = atlas_rect_0(atlas_0, kernelContext_23);

    if(atlas_rect_is_empty_0(_S158))
    {


        return 1.0f;
    }

#line 3924
    float _S159 = tile_texels_0(_S158, kernelContext_23);

    float texel_world_1 = map_world_0 / _S159;

#line 3936
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 3943
    float _S160 = clip_1.w;

#line 3943
    if(_S160 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S160) ;

#line 3947
    bool _S161;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3948
        _S161 = true;

#line 3948
    }
    else
    {

#line 3948
        _S161 = (ndc_1.z) <= 0.0f;

#line 3948
    }

#line 3948
    if(_S161)
    {

#line 3948
        _S161 = true;

#line 3948
    }
    else
    {

#line 3948
        _S161 = (ndc_1.z) > 1.0f;

#line 3948
    }

#line 3948
    if(_S161)
    {

#line 3955
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 3960
    uint _S162 = shadow_filter_mode_0(pixel_6, kernelContext_23);

#line 3969
    if(_S162 == 2U)
    {

#line 3969
        float _S163 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_23);

        return _S163;
    }

#line 3971
    float _S164 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_23);

    return _S164;
}


#line 4038
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_24)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 4046
    float _S165 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_24);

#line 4052
    return _S165;
}


#line 3980
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_25)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3987
    float4 _S166 = float4(light_2->direction_0) ;

#line 3994
    float cos_outer_1 = _S166.w;

#line 3994
    float _S167 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S166.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_25);

#line 4001
    return _S167;
}


#line 2351
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 4983
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 4985
    float3 _S168;
    if((length(decoded_0)) < 0.5f)
    {

#line 4986
        _S168 = shading_normal_1;

#line 4986
    }
    else
    {

#line 4986
        _S168 = normalize(decoded_0);

#line 4986
    }

#line 4986
    return _S168;
}


#line 4621
float3 sky_irradiance_0(float3 normal_8, KernelContext_0 thread* kernelContext_26)
{
    float4 basis_6 = float4(normal_8, 1.0f);
    return max(float3(dot(kernelContext_26->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4525
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4525
    float reach_0 = 0.0f;

#line 4525
    uint axis_3 = 0U;


    for(;;)
    {

#line 4528
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4528
            break;
        }

#line 4528
        uint _S169 = axis_3;

#line 4528
        bool _S170;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4530
            _S170 = true;

#line 4530
        }
        else
        {

#line 4530
            _S170 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4530
        }

#line 4530
        if(_S170)
        {

#line 4531
            axis_3 = axis_3 + 1U;

#line 4528
            continue;
        }

#line 4528
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S169] - 1.0f));

#line 4528
        axis_3 = axis_3 + 1U;

#line 4528
    }

#line 4535
    return reach_0;
}


#line 4555
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4555
    uint level_0 = 0U;

    for(;;)
    {

#line 4557
        uint _S171 = level_0 + 1U;

#line 4557
        if(_S171 < levels_0)
        {
        }
        else
        {

#line 4557
            break;
        }
        float _S172 = float(level_0);

#line 4559
        float at_3 = reach_1 * exp2(- _S172);
        if(at_3 < 1.0f)
        {

#line 4561
            return float2(_S172, saturate((1.0f - at_3) / 0.25f));
        }

#line 4557
        level_0 = _S171;

#line 4557
    }

#line 4563
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4312
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4314
    uint _S173;
    if(at_4 >= count_2)
    {

#line 4315
        _S173 = at_4 - count_2;

#line 4315
    }
    else
    {

#line 4315
        _S173 = at_4;

#line 4315
    }

#line 4315
    return _S173;
}


#line 4338
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_27)
{
    uint3 counts_0 = kernelContext_27->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_27->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S174 = counts_0.x;
    uint _S175 = counts_0.y;



    return min(kernelContext_27->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S175 + probe_wrap_0(cell_2.y, offset_1.y, _S175)) * _S174 + probe_wrap_0(cell_2.x, offset_1.x, _S174), max(kernelContext_27->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4179
float sign_not_zero_0(float value_0)
{

#line 4179
    float _S176;

    if(value_0 >= 0.0f)
    {

#line 4181
        _S176 = 1.0f;

#line 4181
    }
    else
    {

#line 4181
        _S176 = -1.0f;

#line 4181
    }

#line 4181
    return _S176;
}


#line 4198
float2 oct_encode_0(float3 direction_1)
{
    float _S177 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S177) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4201
    float2 p_1;
    if(_S177 < 0.0f)
    {
        float _S178 = p_0.y;

#line 4204
        float _S179 = p_0.x;

#line 4204
        p_1 = float2((1.0f - abs(_S178)) * sign_not_zero_0(_S179), (1.0f - abs(_S179)) * sign_not_zero_0(_S178));

#line 4202
    }
    else
    {

#line 4202
        p_1 = p_0;

#line 4202
    }

#line 4207
    return p_1;
}


#line 4227
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_28)
{

#line 4227
    texture2d_array<float, access::sample> _S180 = kernelContext_28->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S180).get_width(0)),(*((&height_3)) = (_S180).get_height(0)),(*((&layers_0)) = (_S180).get_array_size());

#line 4232
    float2 _S181 = float2(0.5f) ;

#line 4232
    float2 _S182 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S181 + _S181) * float2(16.0f)  + _S182 - _S181;
    float2 _S183 = float2(float(width_3), float(height_3)) - _S182;

#line 4236
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S183);
    float2 high_2 = min(low_2 + _S182, _S183);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S184 = int(low_2.x);

#line 4241
    int _S185 = int(low_2.y);

#line 4241
    int4 _S186 = int4(_S184, _S185, layer_1, int(0));
    int _S187 = int(high_2.x);

#line 4242
    int4 _S188 = int4(_S187, _S185, layer_1, int(0));
    int _S189 = int(high_2.y);

#line 4243
    int4 _S190 = int4(_S184, _S189, layer_1, int(0));
    int4 _S191 = int4(_S187, _S189, layer_1, int(0));
    float2 _S192 = float2(weight_2.x) ;

#line 4245
    return mix(mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S186)).xy), uint(((_S186)).z), uint(((_S186)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S188)).xy), uint(((_S188)).z), uint(((_S188)).w))).xy, _S192), mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S190)).xy), uint(((_S190)).z), uint(((_S190)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S191)).xy), uint(((_S191)).z), uint(((_S191)).w))).xy, _S192), float2(weight_2.y) );
}


#line 4273
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_9, KernelContext_0 thread* kernelContext_29)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_9 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4276
    float2 _S193 = probe_moments_0(index_6, - to_probe_0, kernelContext_29);

#line 4282
    float _S194 = _S193.x;

#line 4282
    float _S195 = max(_S193.y - _S194 * _S194, 0.0f);
    float behind_0 = to_surface_0 - _S194;
    float bound_0 = _S195 / (_S195 + behind_0 * behind_0);

#line 4284
    float _S196;
    if(to_surface_0 <= _S194)
    {

#line 4285
        _S196 = 1.0f;

#line 4285
    }
    else
    {

#line 4285
        _S196 = bound_0 * bound_0 * bound_0;

#line 4285
    }

#line 4285
    return _S196;
}


#line 4295
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_10, KernelContext_0 thread* kernelContext_30)
{

#line 4295
    float _S197 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_10, kernelContext_30);

    return max(_S197, 0.00009999999747379f);
}


#line 1174
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4357
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4384
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_11, KernelContext_0 thread* kernelContext_31)
{

#line 4385
    uint _S198 = probe_row_0(level_2, cell_3, kernelContext_31);


    GpuProbe_natural_0 stored_0 = kernelContext_31->probes_0[_S198];

#line 4388
    float _S199 = probe_weight_0(_S198, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_11, kernelContext_31);



    thread WeightedProbe_0 corner_2;

#line 4392
    float4 _S200 = float4(_S199) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S200;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S200;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S200;
    (&corner_2)->weight_3 = _S199;
    return corner_2;
}


#line 4368
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S201 = float4(t_1) ;

#line 4371
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S201);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S201);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S201);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4456
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_12, KernelContext_0 thread* kernelContext_32)
{

#line 4456
    float3 _S202 = float3(1.0f) ;

#line 4461
    float3 _S203 = float3(0.0f, 0.0f, 0.0f);

#line 4461
    float3 last_1 = max(float3(kernelContext_32->frame_0->probe_counts_0.xyz) - _S202, _S203);



    float3 origin_2 = kernelContext_32->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_32->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S203, last_1);
    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S204 = uint3(base_2);



    uint3 _S205 = uint3(min(base_2 + _S202, last_1));

#line 4481
    float _S206 = inv_0.x;

#line 4481
    float _S207;

#line 4481
    if(_S206 != 0.0f)
    {

#line 4481
        _S207 = 1.0f / _S206;

#line 4481
    }
    else
    {

#line 4481
        _S207 = 0.0f;

#line 4481
    }
    float _S208 = inv_0.y;

#line 4482
    float _S209;

#line 4482
    if(_S208 != 0.0f)
    {

#line 4482
        _S209 = 1.0f / _S208;

#line 4482
    }
    else
    {

#line 4482
        _S209 = 0.0f;

#line 4482
    }
    float _S210 = inv_0.z;

#line 4483
    float _S211;

#line 4483
    if(_S210 != 0.0f)
    {

#line 4483
        _S211 = 1.0f / _S210;

#line 4483
    }
    else
    {

#line 4483
        _S211 = 0.0f;

#line 4483
    }

#line 4481
    float3 spacing_1 = float3(_S207, _S209, _S211);

#line 4490
    uint _S212 = _S204.x;

#line 4490
    uint _S213 = _S204.y;

#line 4490
    uint _S214 = _S204.z;

#line 4490
    WeightedProbe_0 _S215 = probe_corner_0(level_3, uint3(_S212, _S213, _S214), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);
    uint _S216 = _S205.x;

#line 4491
    WeightedProbe_0 _S217 = probe_corner_0(level_3, uint3(_S216, _S213, _S214), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);

#line 4491
    float _S218 = f_0.x;

#line 4491
    thread WeightedProbe_0 _S219 = _S215;

#line 4491
    thread WeightedProbe_0 _S220 = _S217;

#line 4491
    WeightedProbe_0 _S221 = lerp_probe_0(&_S219, &_S220, _S218);
    uint _S222 = _S205.y;

#line 4492
    WeightedProbe_0 _S223 = probe_corner_0(level_3, uint3(_S212, _S222, _S214), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);

#line 4492
    WeightedProbe_0 _S224 = probe_corner_0(level_3, uint3(_S216, _S222, _S214), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);

#line 4492
    thread WeightedProbe_0 _S225 = _S223;

#line 4492
    thread WeightedProbe_0 _S226 = _S224;

#line 4492
    WeightedProbe_0 _S227 = lerp_probe_0(&_S225, &_S226, _S218);

    uint _S228 = _S205.z;

#line 4494
    WeightedProbe_0 _S229 = probe_corner_0(level_3, uint3(_S212, _S213, _S228), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);

#line 4494
    WeightedProbe_0 _S230 = probe_corner_0(level_3, uint3(_S216, _S213, _S228), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);

#line 4494
    thread WeightedProbe_0 _S231 = _S229;

#line 4494
    thread WeightedProbe_0 _S232 = _S230;

#line 4494
    WeightedProbe_0 _S233 = lerp_probe_0(&_S231, &_S232, _S218);

#line 4494
    WeightedProbe_0 _S234 = probe_corner_0(level_3, uint3(_S212, _S222, _S228), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);

#line 4494
    WeightedProbe_0 _S235 = probe_corner_0(level_3, uint3(_S216, _S222, _S228), origin_2, spacing_1, world_position_13, normal_12, kernelContext_32);

#line 4494
    thread WeightedProbe_0 _S236 = _S234;

#line 4494
    thread WeightedProbe_0 _S237 = _S235;

#line 4494
    WeightedProbe_0 _S238 = lerp_probe_0(&_S236, &_S237, _S218);



    float _S239 = f_0.y;

#line 4498
    thread WeightedProbe_0 _S240 = _S221;

#line 4498
    thread WeightedProbe_0 _S241 = _S227;

#line 4498
    WeightedProbe_0 _S242 = lerp_probe_0(&_S240, &_S241, _S239);

#line 4498
    thread WeightedProbe_0 _S243 = _S233;

#line 4498
    thread WeightedProbe_0 _S244 = _S238;

#line 4498
    WeightedProbe_0 _S245 = lerp_probe_0(&_S243, &_S244, _S239);

    float _S246 = f_0.z;

#line 4500
    thread WeightedProbe_0 _S247 = _S242;

#line 4500
    thread WeightedProbe_0 _S248 = _S245;

#line 4500
    WeightedProbe_0 _S249 = lerp_probe_0(&_S247, &_S248, _S246);

    float4 basis_7 = float4(normal_12, 1.0f);
    return max(float3(dot(_S249.sh_0.sh_r_0, basis_7), dot(_S249.sh_0.sh_g_0, basis_7), dot(_S249.sh_0.sh_b_0, basis_7)) / float3(_S249.weight_3) , _S203);
}


#line 4590
float3 probe_irradiance_0(float3 world_position_14, float3 normal_13, KernelContext_0 thread* kernelContext_33)
{

#line 4598
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_33->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_33->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_33->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_33->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 4600
    float3 _S250 = probe_level_irradiance_0(level_4, world_position_14, normal_13, kernelContext_33);


    if(share_0 >= 1.0f)
    {

#line 4604
        return _S250;
    }

#line 4604
    float3 _S251 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_13, kernelContext_33);

    return _S251 * float3((1.0f - share_0))  + _S250 * float3(share_0) ;
}


#line 5052
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 5052
    float3 _S252 = float3(visibility_4) ;

#line 5058
    return min(float3(1.0f) , max(_S252, ((_S252 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S252 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S252));
}


#line 1069
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_6)
{
    return float3(material_6->emissive_r_0, material_6->emissive_g_0, material_6->emissive_b_0);
}


#line 2702
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S253 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2710
    float kernel_0 = 0.0001984127011383f;

#line 2710
    int term_0 = int(6);

    for(;;)
    {

#line 2712
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2712
            break;
        }
        float _S254 = kernel_0 * _S253 + FOG_KERNEL_0[term_0];

#line 2712
        int term_1 = term_0 - int(1);

#line 2712
        kernel_0 = _S254;

#line 2712
        term_0 = term_1;

#line 2712
    }

#line 2719
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2729
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S255 = - d_0;

#line 2733
        float series_0 = 0.00833333376795053f;

#line 2733
        int term_2 = int(3);

        for(;;)
        {

#line 2735
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2735
                break;
            }
            float _S256 = series_0 * _S255 + FOG_RATIO_KERNEL_0[term_2];

#line 2735
            int term_3 = term_2 - int(1);

#line 2735
            series_0 = _S256;

#line 2735
            term_2 = term_3;

#line 2735
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2763
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2774
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2782
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4647
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4647
struct pixelInput_0
{
    float3 world_position_15 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    [[flat]] uint material_7 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
    float3 world_tangent_1 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_4 [[user(TEXCOORD_5)]];
};


#line 5094
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S257 [[stage_in]], bool front_facing_1 [[front_facing]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 5094
    thread KernelContext_0 kernelContext_34;

#line 5094
    (&kernelContext_34)->draw_0 = draw_3;

#line 5094
    (&kernelContext_34)->visible_instances_0 = visible_instances_3;

#line 5094
    (&kernelContext_34)->instances_0 = instances_3;

#line 5094
    (&kernelContext_34)->meshes_0 = meshes_3;

#line 5094
    (&kernelContext_34)->frame_0 = frame_5;

#line 5094
    (&kernelContext_34)->vertices_0 = vertices_3;

#line 5094
    (&kernelContext_34)->ambient_occlusion_0 = ambient_occlusion_3;

#line 5094
    (&kernelContext_34)->materials_0 = materials_3;

#line 5094
    (&kernelContext_34)->base_color_textures_0 = base_color_textures_3;

#line 5094
    (&kernelContext_34)->base_color_sampler_0 = base_color_sampler_3;

#line 5094
    (&kernelContext_34)->normal_textures_0 = normal_textures_3;

#line 5094
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_3;

#line 5094
    (&kernelContext_34)->specular_dfg_0 = specular_dfg_3;

#line 5094
    (&kernelContext_34)->lights_0 = lights_3;

#line 5094
    (&kernelContext_34)->ltc_matrix_0 = ltc_matrix_3;

#line 5094
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_3;

#line 5094
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_3;

#line 5094
    (&kernelContext_34)->contact_shadow_0 = contact_shadow_3;

#line 5094
    (&kernelContext_34)->probes_0 = probes_3;

#line 5094
    (&kernelContext_34)->probe_visibility_0 = probe_visibility_3;

#line 5106
    float3 vertex_normal_0 = normalize(_S257.world_normal_1);

#line 5111
    float2 motion_1 = motion_vector_0(_S257.clip_position_1, _S257.previous_clip_position_1);

#line 5127
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 5129
        float4 _S258 = occlusion_at_0(position_5.xy, &kernelContext_34);



        (&bent_0)->lit_0 = float4(_S258.yzw, 1.0f);


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

#line 5183
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5183
        float4 _S259 = occlusion_at_0(position_5.xy, &kernelContext_34);


        float value_1 = _S259.x;

#line 5185
        thread FragmentOutput_0 occlusion_1;

#line 5194
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S257.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 5211
    thread GpuMaterial_natural_0 _S260 = (&kernelContext_34)->materials_0[_S257.material_7];

#line 5211
    float2 uv_3;

#line 5236
    if(((&_S260)->tiling_0) == 1U)
    {

#line 5236
        uv_3 = physical_tile_uv_0(_S257.world_position_15, vertex_normal_0, (&_S260)->tile_metres_0);

#line 5236
    }
    else
    {

#line 5236
        uv_3 = _S257.uv_2;

#line 5236
    }

#line 5236
    uint _S261 = base_color_layer_0(&_S260);

#line 5254
    float3 _S262 = float3(uv_3, float(_S261));
    float4 albedo_1 = _S257.color_3 * float4((&_S260)->base_color_0)  * (((&kernelContext_34)->base_color_textures_0).sample(((&kernelContext_34)->base_color_sampler_0), ((_S262)).xy, uint(((_S262)).z)));

#line 5269
    float _S263 = albedo_1.w;

#line 5269
    bool _S264 = alpha_masked_0(&_S260, _S263);

#line 5269
    if(_S264)
    {
        discard_fragment();

#line 5269
    }

#line 5269
    float3 _S265 = double_sided_normal_0(&_S260, vertex_normal_0, front_facing_1);

#line 5269
    uint _S266 = normal_layer_0(&_S260);

#line 5269
    thread VertexOutput_0 _S267;

#line 5269
    (&_S267)->position_3 = position_5;

#line 5269
    (&_S267)->world_position_1 = _S257.world_position_15;

#line 5269
    (&_S267)->world_normal_0 = _S257.world_normal_1;

#line 5269
    (&_S267)->color_2 = _S257.color_3;

#line 5269
    (&_S267)->material_5 = _S257.material_7;

#line 5269
    (&_S267)->uv_0 = _S257.uv_2;

#line 5269
    (&_S267)->clip_position_0 = _S257.clip_position_1;

#line 5269
    (&_S267)->previous_clip_position_0 = _S257.previous_clip_position_1;

#line 5269
    (&_S267)->world_tangent_0 = _S257.world_tangent_1;

#line 5269
    (&_S267)->frame_3 = _S257.frame_4;

#line 5269
    float3 _S268 = shading_normal_of_0(_S266, (&_S260)->normal_scale_0, &_S267, _S265, uv_3, &kernelContext_34);

#line 5288
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5290
        float3 _S269 = float3(0.5f) ;

#line 5302
        (&normals_0)->lit_0 = float4(_S268 * _S269 + _S269, 1.0f);

#line 5308
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_34)->frame_0->camera_position_0.xyz - _S257.world_position_15);



    float3 _S270 = geometric_normal_of_0(_S257.world_position_15, _S265);

#line 5323
    float metallic_1 = saturate((&_S260)->metallic_0);
    float roughness_2 = clamp((&_S260)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_1 = roughness_2 * roughness_2;

#line 5358
    float _S271 = saturate(alpha_1 * alpha_1 + specular_aa_kernel_0(_S268));

#line 5364
    float3 _S272 = albedo_1.xyz;

#line 5364
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S272, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S272 * float3((1.0f - metallic_1)) ;

#line 5371
    float _S273 = max(dot(_S268, to_eye_1), 0.00009999999747379f);

#line 5381
    float2 _S274 = position_5.xy;

#line 5381
    uint _S275 = froxel_of_0(_S274, (((float4(_S257.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_34);

#line 5381
    uint base_3 = _S275 * 17U;

#line 5386
    uint _S276 = min((&kernelContext_34)->cluster_lights_0[base_3], 16U);

#line 5386
    TableTap_0 _S277 = table_tap_0(_S273, roughness_2, &kernelContext_34);

#line 5386
    thread TableTap_0 _S278 = _S277;

#line 5386
    float2 _S279 = dfg_at_0(&_S278, &kernelContext_34);

#line 5395
    float _S280 = _S279.x;

#line 5395
    float _S281 = _S279.y;

#line 5395
    float3 _S282 = f0_2 * float3(_S280)  + float3(_S281) ;

#line 5401
    float3 _S283 = float3(0.0f, 0.0f, 0.0f);

#line 5401
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5401
    uint slot_0 = 0U;

#line 5401
    float3 direct_0 = _S283;

#line 5401
    float3 gloss_0 = _S283;

#line 5411
    for(;;)
    {

#line 5411
        if(slot_0 < _S276)
        {
        }
        else
        {

#line 5411
            break;
        }

#line 5411
        thread GpuLight_natural_0 _S284 = (&kernelContext_34)->lights_0[(&kernelContext_34)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 5411
        uint _S285 = (&_S284)->kind_0;

#line 5420
        bool _S286 = ((&_S284)->kind_0) == 0U;

#line 5420
        float3 to_light_7;

#line 5420
        float reach_2;

#line 5420
        if(_S286)
        {

#line 5420
            to_light_7 = normalize((float4((&_S284)->direction_0) ).xyz);

#line 5420
            reach_2 = 1.0f;

#line 5420
        }
        else
        {


            if(_S285 == 3U)
            {

#line 5425
                float4 _S287 = float4((&_S284)->position_0) ;

#line 5433
                float3 offset_2 = _S287.xyz - _S257.world_position_15;
                float distance_3 = length(offset_2);

                float _S288 = range_window_0(distance_3, _S287.w);

#line 5436
                to_light_7 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5436
                reach_2 = _S288;

#line 5425
            }
            else
            {

#line 5425
                float4 _S289 = float4((&_S284)->position_0) ;

#line 5440
                float3 offset_3 = _S289.xyz - _S257.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_8 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S289.w);
                if(_S285 == 2U)
                {

#line 5444
                    float4 _S290 = float4((&_S284)->direction_0) ;

#line 5444
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S290.xyz, _S290.w, (&_S284)->cos_inner_0);

#line 5444
                }
                else
                {

#line 5444
                    reach_2 = reach_3;

#line 5444
                }

#line 5444
                to_light_7 = to_light_8;

#line 5425
            }

#line 5420
        }

#line 5453
        float n_dot_l_5 = dot(_S268, to_light_7);

#line 5453
        float3 specular_0;

#line 5453
        float diffuse_0;


        if(_S285 == 3U)
        {

#line 5466
            thread array<float3, int(4)> corners_2;

#line 5466
            rect_corners_0(&_S284, _S257.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S268, to_eye_1, _S273);

#line 5468
            thread array<float3, int(4)> _S291 = corners_2;

#line 5468
            float _S292 = ltc_irradiance_0(to_local_0, &_S291);

#line 5468
            thread TableTap_0 _S293 = _S277;

#line 5468
            float4 _S294 = ltc_at_0(&_S293, &kernelContext_34);

            matrix<float,int(3),int(3)>  _S295 = (((to_local_0) * (ltc_transform_0(_S294))));

#line 5470
            thread array<float3, int(4)> _S296 = corners_2;

#line 5470
            float _S297 = ltc_irradiance_0(_S295, &_S296);
            float3 _S298 = float3(_S297)  * _S282;

#line 5471
            diffuse_0 = _S292;

#line 5471
            specular_0 = _S298;

#line 5456
        }
        else
        {

#line 5476
            float _S299 = max(n_dot_l_5, 0.0f);

#line 5483
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5491
            float3 specular_1 = ggx_lobe_0(_S271, f0_2, _S299, _S273, max(dot(_S268, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S299) ;

#line 5491
            diffuse_0 = _S299;

#line 5491
            specular_0 = specular_1;

#line 5456
        }

#line 5456
        float3 specular_2;

#line 5499
        if((((&_S284)->flags_3) & 1U) != 0U)
        {

#line 5499
            specular_2 = _S283;

#line 5499
        }
        else
        {

#line 5499
            specular_2 = specular_0;

#line 5499
        }

#line 5499
        float reach_4;

#line 5517
        if(_S286)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 5520
            float _S300 = sun_visibility_0(_S257.world_position_15, to_light_7, n_dot_l_5, _S270, _S274, &sun_cascade_0, &sun_fade_0, &kernelContext_34);

#line 5520
            float _S301 = contact_at_0(_S274, &kernelContext_34);

#line 5529
            float _S302 = _S300 * _S301;

#line 5529
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 5529
            reach_4 = _S302;

#line 5517
        }
        else
        {

#line 5534
            if(_S285 == 1U)
            {

#line 5534
                uint _S303 = (&_S284)->shadow_tile_0;

#line 5546
                if(((&_S284)->shadow_tile_0) <= 8U)
                {

#line 5546
                    float _S304 = point_visibility_0(&_S284, _S303, _S257.world_position_15, to_light_7, n_dot_l_5, _S270, _S274, &kernelContext_34);

#line 5546
                    reach_4 = reach_2 * _S304;

#line 5546
                }
                else
                {

#line 5546
                    reach_4 = reach_2;

#line 5546
                }

#line 5534
            }
            else
            {

#line 5534
                uint _S305 = (&_S284)->shadow_tile_0;

#line 5552
                if(((&_S284)->shadow_tile_0) < 14U)
                {

#line 5552
                    float _S306 = spot_visibility_0(&_S284, _S305, _S257.world_position_15, to_light_7, n_dot_l_5, _S270, _S274, &kernelContext_34);

#line 5552
                    reach_4 = reach_2 * _S306;

#line 5552
                }
                else
                {

#line 5552
                    reach_4 = reach_2;

#line 5552
                }

#line 5534
            }

#line 5517
        }

#line 5560
        float3 _S307 = (float4((&_S284)->color_0) ).xyz;

#line 5560
        float3 direct_1 = direct_0 + _S307 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S307 * (specular_2 * float3(reach_4) );

#line 5411
        slot_0 = slot_0 + 1U;

#line 5411
        direct_0 = direct_1;

#line 5411
        gloss_0 = gloss_1;

#line 5411
    }

#line 5575
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S280 + _S281);

#line 5575
    float4 _S308 = occlusion_at_0(_S274, &kernelContext_34);

#line 5594
    float occluded_0 = _S308.x;

#line 5603
    float3 bent_normal_0 = bent_normal_at_0(_S308, _S268);

#line 5626
    float3 _S309 = frame_5->ambient_0.xyz;

#line 5626
    float3 _S310 = sky_irradiance_0(bent_normal_0, &kernelContext_34);

#line 5626
    float3 _S311 = _S309 + _S310;

#line 5626
    float3 _S312 = probe_irradiance_0(_S257.world_position_15, bent_normal_0, &kernelContext_34);

#line 5662
    float3 lit_1 = diffuse_albedo_0 * ((_S311 + _S312) * multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) + direct_0) + gloss_2;

#line 5662
    float3 _S313 = emissive_of_0(&_S260);

#line 5698
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_34)->frame_0->fog_params_0.x, (&kernelContext_34)->frame_0->fog_params_0.y, (&kernelContext_34)->frame_0->camera_position_0.y - (&kernelContext_34)->frame_0->fog_params_0.z, _S257.world_position_15.y - (&kernelContext_34)->frame_0->fog_params_0.z, length((&kernelContext_34)->frame_0->camera_position_0.xyz - _S257.world_position_15)));
    float3 lit_2 = (lit_1 + _S313) * float3(fog_survives_0)  + (&kernelContext_34)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;



    (&output_2)->lit_0 = float4(lit_2, _S263);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 5718
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, _S263);

#line 5727
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 5718
    }

#line 5729
    return output_2;
}


#line 5729
struct pixelInput_1
{
    float3 world_position_16 [[user(POSITION)]];
    float3 world_normal_2 [[user(NORMAL)]];
    float4 color_4 [[user(COLOR)]];
    [[flat]] uint material_8 [[user(TEXCOORD)]];
    float2 uv_4 [[user(TEXCOORD_1)]];
    float4 clip_position_2 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_2 [[user(TEXCOORD_3)]];
    float3 world_tangent_2 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_6 [[user(TEXCOORD_5)]];
};


#line 5762
[[fragment]] void depthMaskedFragmentMain(pixelInput_1 _S314 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 5762
    thread KernelContext_0 kernelContext_35;

#line 5762
    (&kernelContext_35)->draw_0 = draw_4;

#line 5762
    (&kernelContext_35)->visible_instances_0 = visible_instances_4;

#line 5762
    (&kernelContext_35)->instances_0 = instances_4;

#line 5762
    (&kernelContext_35)->meshes_0 = meshes_4;

#line 5762
    (&kernelContext_35)->frame_0 = frame_7;

#line 5762
    (&kernelContext_35)->vertices_0 = vertices_4;

#line 5762
    (&kernelContext_35)->ambient_occlusion_0 = ambient_occlusion_4;

#line 5762
    (&kernelContext_35)->materials_0 = materials_4;

#line 5762
    (&kernelContext_35)->base_color_textures_0 = base_color_textures_4;

#line 5762
    (&kernelContext_35)->base_color_sampler_0 = base_color_sampler_4;

#line 5762
    (&kernelContext_35)->normal_textures_0 = normal_textures_4;

#line 5762
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_4;

#line 5762
    (&kernelContext_35)->specular_dfg_0 = specular_dfg_4;

#line 5762
    (&kernelContext_35)->lights_0 = lights_4;

#line 5762
    (&kernelContext_35)->ltc_matrix_0 = ltc_matrix_4;

#line 5762
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_4;

#line 5762
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_4;

#line 5762
    (&kernelContext_35)->contact_shadow_0 = contact_shadow_4;

#line 5762
    (&kernelContext_35)->probes_0 = probes_4;

#line 5762
    (&kernelContext_35)->probe_visibility_0 = probe_visibility_4;

#line 5762
    thread GpuMaterial_natural_0 _S315 = materials_4[_S314.material_8];

#line 5762
    float2 uv_5;

#line 5771
    if(((&_S315)->tiling_0) == 1U)
    {

#line 5771
        uv_5 = physical_tile_uv_0(_S314.world_position_16, normalize(_S314.world_normal_2), (&_S315)->tile_metres_0);

#line 5771
    }
    else
    {

#line 5771
        uv_5 = _S314.uv_4;

#line 5771
    }

#line 5771
    uint _S316 = base_color_layer_0(&_S315);

#line 5777
    float3 _S317 = float3(uv_5, float(_S316));

#line 5777
    bool _S318 = alpha_masked_0(&_S315, _S314.color_4.w * (float4((&_S315)->base_color_0) ).w * (((&kernelContext_35)->base_color_textures_0).sample(((&kernelContext_35)->base_color_sampler_0), ((_S317)).xy, uint(((_S317)).z))).w);



    if(_S318)
    {
        discard_fragment();

#line 5781
    }



    return;
}


#line 5815
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_14 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 5815
struct pixelInput_2
{
    float3 world_position_17 [[user(POSITION)]];
    float3 world_normal_3 [[user(NORMAL)]];
    float4 color_5 [[user(COLOR)]];
    [[flat]] uint material_9 [[user(TEXCOORD)]];
    float2 uv_6 [[user(TEXCOORD_1)]];
    float4 clip_position_3 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_3 [[user(TEXCOORD_3)]];
    float3 world_tangent_3 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_8 [[user(TEXCOORD_5)]];
};


#line 5858
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_2 _S319 [[stage_in]], bool front_facing_2 [[front_facing]], float4 position_7 [[position]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 5858
    thread KernelContext_0 kernelContext_36;

#line 5858
    (&kernelContext_36)->draw_0 = draw_5;

#line 5858
    (&kernelContext_36)->visible_instances_0 = visible_instances_5;

#line 5858
    (&kernelContext_36)->instances_0 = instances_5;

#line 5858
    (&kernelContext_36)->meshes_0 = meshes_5;

#line 5858
    (&kernelContext_36)->frame_0 = frame_9;

#line 5858
    (&kernelContext_36)->vertices_0 = vertices_5;

#line 5858
    (&kernelContext_36)->ambient_occlusion_0 = ambient_occlusion_5;

#line 5858
    (&kernelContext_36)->materials_0 = materials_5;

#line 5858
    (&kernelContext_36)->base_color_textures_0 = base_color_textures_5;

#line 5858
    (&kernelContext_36)->base_color_sampler_0 = base_color_sampler_5;

#line 5858
    (&kernelContext_36)->normal_textures_0 = normal_textures_5;

#line 5858
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_5;

#line 5858
    (&kernelContext_36)->specular_dfg_0 = specular_dfg_5;

#line 5858
    (&kernelContext_36)->lights_0 = lights_5;

#line 5858
    (&kernelContext_36)->ltc_matrix_0 = ltc_matrix_5;

#line 5858
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_5;

#line 5858
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_5;

#line 5858
    (&kernelContext_36)->contact_shadow_0 = contact_shadow_5;

#line 5858
    (&kernelContext_36)->probes_0 = probes_5;

#line 5858
    (&kernelContext_36)->probe_visibility_0 = probe_visibility_5;

#line 5863
    float3 vertex_normal_1 = normalize(_S319.world_normal_3);

#line 5863
    thread GpuMaterial_natural_0 _S320 = materials_5[_S319.material_9];

#line 5863
    float2 uv_7;

#line 5870
    if(((&_S320)->tiling_0) == 1U)
    {

#line 5870
        uv_7 = physical_tile_uv_0(_S319.world_position_17, vertex_normal_1, (&_S320)->tile_metres_0);

#line 5870
    }
    else
    {

#line 5870
        uv_7 = _S319.uv_6;

#line 5870
    }

#line 5870
    uint _S321 = base_color_layer_0(&_S320);

#line 5875
    float3 _S322 = float3(uv_7, float(_S321));
    float4 albedo_3 = _S319.color_5 * float4((&_S320)->base_color_0)  * (((&kernelContext_36)->base_color_textures_0).sample(((&kernelContext_36)->base_color_sampler_0), ((_S322)).xy, uint(((_S322)).z)));

#line 5876
    bool _S323 = alpha_masked_0(&_S320, albedo_3.w);

#line 5882
    if(_S323)
    {
        discard_fragment();

#line 5882
    }

#line 5887
    thread RsmOutput_0 written_0;



    (&written_0)->albedo_2 = float4(albedo_3.xyz * float3((1.0f - saturate((&_S320)->metallic_0))) , 1.0f);

#line 5891
    float3 _S324 = double_sided_normal_0(&_S320, vertex_normal_1, front_facing_2);

#line 5891
    float3 _S325 = float3(0.5f) ;

#line 5897
    (&written_0)->normal_14 = float4(_S324 * _S325 + _S325, 1.0f);

    (&written_0)->world_0 = float4(_S319.world_position_17, 1.0f);
    return written_0;
}


#line 5900
struct vertexMain_Result_0
{
    float4 position_8 [[position]];
    float3 world_position_18 [[user(POSITION)]];
    float3 world_normal_4 [[user(NORMAL)]];
    float4 color_6 [[user(COLOR)]];
    uint material_10 [[user(TEXCOORD)]];
    float2 uv_8 [[user(TEXCOORD_1)]];
    float4 clip_position_4 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_4 [[user(TEXCOORD_3)]];
    float3 world_tangent_4 [[user(TEXCOORD_4)]];
    uint frame_10 [[user(TEXCOORD_5)]];
};


#line 5900
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_6 [[buffer(3)]], uint device* visible_instances_6 [[buffer(5)]], GpuInstance_natural_0 device* instances_6 [[buffer(2)]], GpuMesh_0 device* meshes_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_11 [[buffer(0)]], uint device* vertices_6 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_6 [[texture(2)]], GpuMaterial_natural_0 device* materials_6 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_6 [[texture(0)]], sampler base_color_sampler_6 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_6 [[texture(4)]], uint device* cluster_lights_6 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_6 [[texture(3)]], GpuLight_natural_0 device* lights_6 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_6 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_6 [[texture(6)]], GpuProbe_natural_0 device* probes_6 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_6 [[texture(7)]])
{

#line 5900
    thread KernelContext_0 kernelContext_37;

#line 5900
    (&kernelContext_37)->draw_0 = draw_6;

#line 5900
    (&kernelContext_37)->visible_instances_0 = visible_instances_6;

#line 5900
    (&kernelContext_37)->instances_0 = instances_6;

#line 5900
    (&kernelContext_37)->meshes_0 = meshes_6;

#line 5900
    (&kernelContext_37)->frame_0 = frame_11;

#line 5900
    (&kernelContext_37)->vertices_0 = vertices_6;

#line 5900
    (&kernelContext_37)->ambient_occlusion_0 = ambient_occlusion_6;

#line 5900
    (&kernelContext_37)->materials_0 = materials_6;

#line 5900
    (&kernelContext_37)->base_color_textures_0 = base_color_textures_6;

#line 5900
    (&kernelContext_37)->base_color_sampler_0 = base_color_sampler_6;

#line 5900
    (&kernelContext_37)->normal_textures_0 = normal_textures_6;

#line 5900
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_6;

#line 5900
    (&kernelContext_37)->specular_dfg_0 = specular_dfg_6;

#line 5900
    (&kernelContext_37)->lights_0 = lights_6;

#line 5900
    (&kernelContext_37)->ltc_matrix_0 = ltc_matrix_6;

#line 5900
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_6;

#line 5900
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_6;

#line 5900
    (&kernelContext_37)->contact_shadow_0 = contact_shadow_6;

#line 5900
    (&kernelContext_37)->probes_0 = probes_6;

#line 5900
    (&kernelContext_37)->probe_visibility_0 = probe_visibility_6;

#line 5900
    GpuInstance_natural_0 device* _S326 = instances_6+visible_instances_6[draw_6->base_0 + instance_id_1];

#line 1933
    GpuMesh_0 mesh_3 = meshes_6[draw_6->mesh_0];

#line 1941
    bool _S327 = ((_S326->flags_0) & 2U) != 0U;

#line 1941
    uint base_vertex_3;
    if(_S327)
    {

#line 1942
        base_vertex_3 = _S326->base_vertex_0;

#line 1942
    }
    else
    {

#line 1942
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1942
    }

#line 1942
    MeshVertex_0 _S328 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_37);

#line 1942
    uint previous_base_0;

#line 1955
    if(_S327)
    {

#line 1955
        previous_base_0 = _S326->previous_base_vertex_0;

#line 1955
    }
    else
    {

#line 1955
        previous_base_0 = base_vertex_3;

#line 1955
    }

#line 1955
    float3 _S329 = load_position_0(index_8 + previous_base_0, &kernelContext_37);

#line 1955
    matrix<float,int(4),int(4)>  _S330 = matrix<float,int(4),int(4)> (_S326->transform_0.data_0[int(0)][int(0)], _S326->transform_0.data_0[int(1)][int(0)], _S326->transform_0.data_0[int(2)][int(0)], _S326->transform_0.data_0[int(3)][int(0)], _S326->transform_0.data_0[int(0)][int(1)], _S326->transform_0.data_0[int(1)][int(1)], _S326->transform_0.data_0[int(2)][int(1)], _S326->transform_0.data_0[int(3)][int(1)], _S326->transform_0.data_0[int(0)][int(2)], _S326->transform_0.data_0[int(1)][int(2)], _S326->transform_0.data_0[int(2)][int(2)], _S326->transform_0.data_0[int(3)][int(2)], _S326->transform_0.data_0[int(0)][int(3)], _S326->transform_0.data_0[int(1)][int(3)], _S326->transform_0.data_0[int(2)][int(3)], _S326->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S328.position_1, 1.0f)) * (_S330)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 1969
    matrix<float,int(3),int(3)>  _S331 = matrix<float,int(3),int(3)> (_S330[int(0)].xyz, _S330[int(1)].xyz, _S330[int(2)].xyz);

#line 1969
    (&output_3)->world_normal_0 = (((_S328.basis_1.normal_0) * (normal_basis_0(_S331))));

#line 1975
    (&output_3)->world_tangent_0 = (((_S328.basis_1.tangent_1) * (_S331)));

#line 1975
    thread TangentFrame_0 _S332 = _S328.basis_1;

#line 1975
    uint _S333 = frame_word_0(mesh_3.flags_1, &_S332);
    (&output_3)->frame_3 = _S333;

#line 1976
    float4 _S334;

#line 1983
    if(((&kernelContext_37)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1983
        _S334 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1983
    }
    else
    {

#line 1983
        _S334 = _S328.color_1;

#line 1983
    }

#line 1982
    (&output_3)->color_2 = _S334;

#line 1989
    (&output_3)->material_5 = _S326->material_0;
    (&output_3)->uv_0 = _S328.uv0_0;

#line 1996
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S329, 1.0f)) * (matrix<float,int(4),int(4)> (_S326->previous_transform_0.data_0[int(0)][int(0)], _S326->previous_transform_0.data_0[int(1)][int(0)], _S326->previous_transform_0.data_0[int(2)][int(0)], _S326->previous_transform_0.data_0[int(3)][int(0)], _S326->previous_transform_0.data_0[int(0)][int(1)], _S326->previous_transform_0.data_0[int(1)][int(1)], _S326->previous_transform_0.data_0[int(2)][int(1)], _S326->previous_transform_0.data_0[int(3)][int(1)], _S326->previous_transform_0.data_0[int(0)][int(2)], _S326->previous_transform_0.data_0[int(1)][int(2)], _S326->previous_transform_0.data_0[int(2)][int(2)], _S326->previous_transform_0.data_0[int(3)][int(2)], _S326->previous_transform_0.data_0[int(0)][int(3)], _S326->previous_transform_0.data_0[int(1)][int(3)], _S326->previous_transform_0.data_0[int(2)][int(3)], _S326->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_37)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S335 = output_3;

#line 2000
    thread vertexMain_Result_0 _S336;

#line 2000
    (&_S336)->position_8 = _S335.position_3;

#line 2000
    (&_S336)->world_position_18 = _S335.world_position_1;

#line 2000
    (&_S336)->world_normal_4 = _S335.world_normal_0;

#line 2000
    (&_S336)->color_6 = _S335.color_2;

#line 2000
    (&_S336)->material_10 = _S335.material_5;

#line 2000
    (&_S336)->uv_8 = _S335.uv_0;

#line 2000
    (&_S336)->clip_position_4 = _S335.clip_position_0;

#line 2000
    (&_S336)->previous_clip_position_4 = _S335.previous_clip_position_0;

#line 2000
    (&_S336)->world_tangent_4 = _S335.world_tangent_0;

#line 2000
    (&_S336)->frame_10 = _S335.frame_3;

#line 2000
    return _S336;
}

