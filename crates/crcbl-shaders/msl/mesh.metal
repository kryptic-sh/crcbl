#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2545 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2540
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 3542
constant array<float3, int(2)> CASCADE_TINTS_0 = { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) };

#line 3025
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2812
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2872
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2887
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2915
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1205
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1849
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1849
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


#line 1855
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1855
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


#line 1248
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


#line 1259
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1262
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1713
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1836
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1836
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1838
        word_4 = 1U;

#line 1838
    }
    else
    {

#line 1838
        word_4 = 0U;

#line 1838
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1842
        word_4 = word_4 | 2U;

#line 1842
    }

#line 1841
    return word_4;
}


#line 1841
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1956
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 1956
    thread KernelContext_0 kernelContext_2;

#line 1956
    (&kernelContext_2)->draw_0 = draw_1;

#line 1956
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1956
    (&kernelContext_2)->instances_0 = instances_1;

#line 1956
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1956
    (&kernelContext_2)->frame_0 = frame_1;

#line 1956
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1956
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1956
    (&kernelContext_2)->materials_0 = materials_1;

#line 1956
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1956
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1956
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1956
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1956
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1956
    (&kernelContext_2)->lights_0 = lights_1;

#line 1956
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1956
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1956
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1956
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 1956
    (&kernelContext_2)->probes_0 = probes_1;

#line 1956
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 1956
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1959
    uint base_vertex_2;

#line 1965
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1965
        base_vertex_2 = _S7->base_vertex_0;

#line 1965
    }
    else
    {

#line 1965
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1965
    }

#line 1965
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1965
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1965
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 1968
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 1989
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 1989
    thread KernelContext_0 kernelContext_3;

#line 1989
    (&kernelContext_3)->draw_0 = draw_2;

#line 1989
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 1989
    (&kernelContext_3)->instances_0 = instances_2;

#line 1989
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 1989
    (&kernelContext_3)->frame_0 = frame_2;

#line 1989
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 1989
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1989
    (&kernelContext_3)->materials_0 = materials_2;

#line 1989
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 1989
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 1989
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 1989
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 1989
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 1989
    (&kernelContext_3)->lights_0 = lights_2;

#line 1989
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 1989
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 1989
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 1989
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 1989
    (&kernelContext_3)->probes_0 = probes_2;

#line 1989
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 1989
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 4947
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 4949
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4823
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4823
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4829
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4557
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4561
    float _S16 = axis_0.y;

#line 4561
    bool _S17;

#line 4561
    if(_S15 >= _S16)
    {

#line 4561
        _S17 = _S15 >= (axis_0.z);

#line 4561
    }
    else
    {

#line 4561
        _S17 = false;

#line 4561
    }

#line 4561
    float2 planar_0;

#line 4561
    if(_S17)
    {

#line 4561
        planar_0 = world_position_0.zy;

#line 4561
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4565
            planar_0 = world_position_0.xz;

#line 4565
        }
        else
        {

#line 4565
            planar_0 = world_position_0.xy;

#line 4565
        }

#line 4561
    }

#line 4573
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1059
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 4594
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S18 = normal_2.z;

#line 4596
    float sign_z_0;

#line 4596
    if(_S18 >= 0.0f)
    {

#line 4596
        sign_z_0 = 1.0f;

#line 4596
    }
    else
    {

#line 4596
        sign_z_0 = -1.0f;

#line 4596
    }
    float a_0 = -1.0f / (sign_z_0 + _S18);
    float _S19 = normal_2.x;

#line 4598
    float _S20 = sign_z_0 * _S19;

#line 4598
    return float3(1.0f + _S20 * _S19 * a_0, _S20 * normal_2.y * a_0, - sign_z_0 * _S19);
}


#line 4648
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S21 = duvdy_0.y;

#line 4650
    float _S22 = duvdx_0.y;

#line 4650
    float winding_0;
    if((duvdx_0.x * _S21 - duvdy_0.x * _S22) < 0.0f)
    {

#line 4651
        winding_0 = -1.0f;

#line 4651
    }
    else
    {

#line 4651
        winding_0 = 1.0f;

#line 4651
    }
    float3 tangent_2 = (float3(_S21)  * dpdx_0 - float3(_S22)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 4660
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 4661
    float3 _S23;

#line 4670
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 4670
        _S23 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 4670
    }
    else
    {

#line 4670
        _S23 = orthonormal_tangent_0(normal_3);

#line 4670
    }

#line 4670
    (&basis_4)->tangent_1 = _S23;

    (&basis_4)->bitangent_0 = cross(normal_3, _S23);
    return basis_4;
}


#line 1720
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


#line 4730
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 4742
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 4752
    uint _S24 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4761
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4763
        float3 _S25;

#line 4768
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4768
            _S25 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4768
        }
        else
        {

#line 4768
            _S25 = orthonormal_tangent_0(normal_4);

#line 4768
        }

#line 4768
        (&basis_5)->tangent_1 = _S25;

#line 4774
        float3 _S26 = cross((&basis_5)->normal_0, _S25);

#line 4774
        float _S27;
        if((_S24 & 2U) != 0U)
        {

#line 4775
            _S27 = -1.0f;

#line 4775
        }
        else
        {

#line 4775
            _S27 = 1.0f;

#line 4775
        }

#line 4774
        (&basis_5)->bitangent_0 = _S26 * float3(_S27) ;

#line 4753
    }
    else
    {

#line 4779
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 4753
    }

#line 4783
    float3 _S28 = float3(uv_1, float(layer_0));
    float3 _S29 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S28)).xy, uint(((_S28)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4784
    thread float3 tangent_space_0 = _S29;
    tangent_space_0.xy = _S29.xy * float2(normal_scale_1) ;

#line 4790
    float3 _S30 = normalize(tangent_space_0);

#line 4790
    tangent_space_0 = _S30;
    return normalize(float3(_S30.x)  * (&basis_5)->tangent_1 + float3(_S30.y)  * (&basis_5)->bitangent_0 + float3(_S30.z)  * (&basis_5)->normal_0);
}


#line 2680
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2691
    float3 _S31;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2692
        _S31 = - facet_1;

#line 2692
    }
    else
    {

#line 2692
        _S31 = facet_1;

#line 2692
    }

#line 2692
    return _S31;
}


#line 1044
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3979
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S32 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S33 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S34 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 3989
    uint _S36 = uint(pixel_0.x) / _S35;

#line 3989
    uint _S37 = min(_S36, _S32 - 1U);
    uint _S38 = uint(pixel_0.y) / _S35;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4000
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S34 - 1U))) * _S33 + min(_S38, _S33 - 1U)) * _S32 + _S37;
}


#line 2112
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2133
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 2133
    texture2d<float, access::sample> _S39 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S39).get_width(0)),(*((&height_1)) = (_S39).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2139
    float2 _S40 = float2(1.0f) ;
    float2 _S41 = extent_1 - _S40;

#line 2140
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S41);
    float2 high_1 = min(low_1 + _S40, _S41);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2158
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 2170
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S42 = tap_1->lo_0.x;

#line 2172
    int _S43 = tap_1->lo_0.y;

#line 2172
    int3 _S44 = int3(_S42, _S43, int(0));
    int _S45 = tap_1->hi_0.x;

#line 2173
    int3 _S46 = int3(_S45, _S43, int(0));
    float2 _S47 = float2(tap_1->weight_0.x) ;
    int _S48 = tap_1->hi_0.y;

#line 2175
    int3 _S49 = int3(_S42, _S48, int(0));
    int3 _S50 = int3(_S45, _S48, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)))), _S47), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), _S47), float2(tap_1->weight_0.y) );
}


#line 3930
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3946
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3958
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3965
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2499
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2499
    float4 _S51 = float4(light_0->tangent_0) ;

    float3 _S52 = _S51.xyz;

#line 2501
    float3 across_0 = _S52 * float3(_S51.w) ;

#line 2501
    float4 _S53 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S52, _S53.xyz) * float3(_S53.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S54 = centre_0 - across_0;

#line 2504
    (*corners_0)[int(0)] = _S54 - down_0;
    float3 _S55 = centre_0 + across_0;

#line 2505
    (*corners_0)[int(1)] = _S55 - down_0;
    (*corners_0)[int(2)] = _S55 + down_0;
    (*corners_0)[int(3)] = _S54 + down_0;
    return;
}


#line 2257
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2260
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2261
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2261
    }
    else
    {

#line 2261
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2261
    }

#line 2261
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2262
        tangent_5 = across_1 / float3(span_0) ;

#line 2262
    }
    else
    {

#line 2262
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2262
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 2238
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2328
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2328
    float3 _S56 = polygon_0->corner_0[int(0)];

#line 2328
    float3 _S57 = polygon_0->corner_0[int(1)];

#line 2328
    float3 _S58 = polygon_0->corner_0[int(2)];

#line 2328
    float3 _S59 = polygon_0->corner_0[int(3)];

#line 2334
    float3 _S60 = float3(0.0f, 0.0f, 0.0f);


    float _S61 = polygon_0->corner_0[int(0)].z;

#line 2337
    int count_1;

#line 2337
    if(_S61 > 0.0f)
    {

#line 2337
        count_1 = int(1);

#line 2337
    }
    else
    {

#line 2337
        count_1 = int(0);

#line 2337
    }
    float _S62 = _S57.z;

#line 2338
    int _S63;

#line 2338
    if(_S62 > 0.0f)
    {

#line 2338
        _S63 = int(2);

#line 2338
    }
    else
    {

#line 2338
        _S63 = int(0);

#line 2338
    }

#line 2338
    int config_0 = count_1 + _S63;
    float _S64 = _S58.z;

#line 2339
    if(_S64 > 0.0f)
    {

#line 2339
        count_1 = int(4);

#line 2339
    }
    else
    {

#line 2339
        count_1 = int(0);

#line 2339
    }

#line 2339
    int config_1 = config_0 + count_1;
    float _S65 = _S59.z;

#line 2340
    if(_S65 > 0.0f)
    {

#line 2340
        count_1 = int(8);

#line 2340
    }
    else
    {

#line 2340
        count_1 = int(0);

#line 2340
    }

#line 2340
    int config_2 = config_1 + count_1;

#line 2340
    float3 l0_0;

#line 2340
    float3 l1_0;

#line 2340
    float3 l2_0;

#line 2340
    float3 l3_0;

#line 2340
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2343
        float3 _S66 = float3(_S61) ;


        float3 _S67 = float3(- _S62)  * _S56 + _S66 * _S57;
        float3 _S68 = float3(- _S65)  * _S56 + _S66 * _S59;

#line 2347
        count_1 = int(3);

#line 2347
        l0_0 = _S56;

#line 2347
        l1_0 = _S67;

#line 2347
        l2_0 = _S68;

#line 2347
        l3_0 = _S59;

#line 2347
        l4_0 = _S60;

#line 2343
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2349
            float3 _S69 = float3(_S62) ;


            float3 _S70 = float3(- _S61)  * _S57 + _S69 * _S56;
            float3 _S71 = float3(- _S64)  * _S57 + _S69 * _S58;

#line 2353
            count_1 = int(3);

#line 2353
            l0_0 = _S70;

#line 2353
            l1_0 = _S57;

#line 2353
            l2_0 = _S71;

#line 2353
            l3_0 = _S59;

#line 2353
            l4_0 = _S60;

#line 2349
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S72 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                float3 _S73 = float3(- _S65)  * _S56 + float3(_S61)  * _S59;

#line 2359
                count_1 = int(4);

#line 2359
                l0_0 = _S56;

#line 2359
                l1_0 = _S57;

#line 2359
                l2_0 = _S72;

#line 2359
                l3_0 = _S73;

#line 2359
                l4_0 = _S60;

#line 2355
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2361
                    float3 _S74 = float3(_S64) ;


                    float3 _S75 = float3(- _S65)  * _S58 + _S74 * _S59;
                    float3 _S76 = float3(- _S62)  * _S58 + _S74 * _S57;

#line 2365
                    count_1 = int(3);

#line 2365
                    l0_0 = _S75;

#line 2365
                    l1_0 = _S76;

#line 2365
                    l2_0 = _S58;

#line 2365
                    l3_0 = _S59;

#line 2365
                    l4_0 = _S60;

#line 2361
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S77 = float3(- _S61)  * _S57 + float3(_S62)  * _S56;
                        float3 _S78 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;

#line 2371
                        count_1 = int(4);

#line 2371
                        l0_0 = _S77;

#line 2371
                        l1_0 = _S57;

#line 2371
                        l2_0 = _S58;

#line 2371
                        l3_0 = _S78;

#line 2371
                        l4_0 = _S60;

#line 2367
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2373
                            float3 _S79 = float3(- _S65) ;


                            float3 _S80 = _S79 * _S56 + float3(_S61)  * _S59;
                            float3 _S81 = _S79 * _S58 + float3(_S64)  * _S59;

#line 2377
                            count_1 = int(5);

#line 2377
                            l0_0 = _S56;

#line 2377
                            l1_0 = _S57;

#line 2377
                            l2_0 = _S58;

#line 2377
                            l3_0 = _S81;

#line 2377
                            l4_0 = _S80;

#line 2373
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2379
                                float3 _S82 = float3(_S65) ;


                                float3 _S83 = float3(- _S61)  * _S59 + _S82 * _S56;
                                float3 _S84 = float3(- _S64)  * _S59 + _S82 * _S58;

#line 2383
                                count_1 = int(3);

#line 2383
                                l0_0 = _S83;

#line 2383
                                l1_0 = _S84;

#line 2383
                                l2_0 = _S59;

#line 2383
                                l3_0 = _S59;

#line 2383
                                l4_0 = _S60;

#line 2379
                            }
                            else
                            {

#line 2386
                                if(config_2 == int(9))
                                {

                                    float3 _S85 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;
                                    float3 _S86 = float3(- _S64)  * _S59 + float3(_S65)  * _S58;

#line 2390
                                    count_1 = int(4);

#line 2390
                                    l0_0 = _S56;

#line 2390
                                    l1_0 = _S85;

#line 2390
                                    l2_0 = _S86;

#line 2390
                                    l3_0 = _S59;

#line 2390
                                    l4_0 = _S60;

#line 2386
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S87 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;
                                        float3 _S88 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;

#line 2397
                                        count_1 = int(5);

#line 2397
                                        l0_0 = _S56;

#line 2397
                                        l1_0 = _S57;

#line 2397
                                        l2_0 = _S88;

#line 2397
                                        l3_0 = _S87;

#line 2397
                                        l4_0 = _S59;

#line 2392
                                    }
                                    else
                                    {

#line 2399
                                        if(config_2 == int(12))
                                        {

                                            float3 _S89 = float3(- _S62)  * _S58 + float3(_S64)  * _S57;
                                            float3 _S90 = float3(- _S61)  * _S59 + float3(_S65)  * _S56;

#line 2403
                                            count_1 = int(4);

#line 2403
                                            l0_0 = _S90;

#line 2403
                                            l1_0 = _S89;

#line 2403
                                            l2_0 = _S58;

#line 2403
                                            l3_0 = _S59;

#line 2403
                                            l4_0 = _S60;

#line 2399
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S91 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                                                float3 _S92 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;

#line 2411
                                                count_1 = int(5);

#line 2411
                                                l0_0 = _S56;

#line 2411
                                                l1_0 = _S92;

#line 2411
                                                l2_0 = _S91;

#line 2411
                                                l3_0 = _S58;

#line 2411
                                                l4_0 = _S59;

#line 2405
                                            }
                                            else
                                            {

#line 2413
                                                if(config_2 == int(14))
                                                {

#line 2413
                                                    float3 _S93 = float3(- _S61) ;


                                                    float3 _S94 = _S93 * _S59 + float3(_S65)  * _S56;
                                                    float3 _S95 = _S93 * _S57 + float3(_S62)  * _S56;

#line 2417
                                                    count_1 = int(5);

#line 2417
                                                    l0_0 = _S95;

#line 2417
                                                    l1_0 = _S94;

#line 2413
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2419
                                                        count_1 = int(4);

#line 2419
                                                    }
                                                    else
                                                    {

#line 2419
                                                        count_1 = int(0);

#line 2419
                                                    }

#line 2419
                                                    l0_0 = _S56;

#line 2419
                                                    l1_0 = _S60;

#line 2413
                                                }

#line 2334
                                                float3 _S96 = l1_0;

#line 2334
                                                l1_0 = _S57;

#line 2334
                                                l2_0 = _S58;

#line 2334
                                                l3_0 = _S59;

#line 2334
                                                l4_0 = _S96;

#line 2405
                                            }

#line 2399
                                        }

#line 2392
                                    }

#line 2386
                                }

#line 2379
                            }

#line 2373
                        }

#line 2367
                    }

#line 2361
                }

#line 2355
            }

#line 2349
        }

#line 2343
    }

#line 2427
    if(count_1 <= int(3))
    {

#line 2427
        l3_0 = l0_0;

#line 2427
        l4_0 = l0_0;

#line 2427
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2432
            l4_0 = l0_0;

#line 2432
        }

#line 2427
    }

#line 2437
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2300
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2306
    float weight_1;

#line 2311
    if(cosine_0 > 0.0f)
    {

#line 2311
        weight_1 = fit_0;

#line 2311
    }
    else
    {

#line 2311
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2311
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2457
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2459
    int corner_1 = int(0);
    for(;;)
    {

#line 2460
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2460
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2460
        corner_1 = corner_1 + int(1);

#line 2460
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2465
    thread LtcPolygon_0 _S97 = polygon_1;

#line 2465
    LtcPolygon_0 _S98 = ltc_clip_0(&_S97);
    polygon_1 = _S98;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2469
    int at_2 = int(0);

    for(;;)
    {

#line 2471
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2471
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2471
        at_2 = at_2 + int(1);

#line 2471
    }

#line 2478
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2478
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2479
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2479
    }
    else
    {

#line 2479
        sum_1 = sum_0;

#line 2479
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2483
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2483
    }

#line 2490
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2186
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S99 = tap_2->lo_0.x;

#line 2188
    int _S100 = tap_2->lo_0.y;

#line 2188
    int3 _S101 = int3(_S99, _S100, int(0));
    int _S102 = tap_2->hi_0.x;

#line 2189
    int3 _S103 = int3(_S102, _S100, int(0));
    float4 _S104 = float4(tap_2->weight_0.x) ;
    int _S105 = tap_2->hi_0.y;

#line 2191
    int3 _S106 = int3(_S99, _S105, int(0));
    int3 _S107 = int3(_S102, _S105, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S101)).xy), uint(((_S101)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S103)).xy), uint(((_S103)).z))), _S104), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S107)).xy), uint(((_S107)).z))), _S104), float4(tap_2->weight_0.y) );
}


#line 2273
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2068
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2075
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2082
    float _S108 = 1.0f - alpha2_0;

#line 2087
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S108 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S108 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3102
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3102
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3162
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3134
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2731
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3089
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_13)
{

#line 3089
    uint _S109;

    if(uint(pixel_1.x) < (kernelContext_13->frame_0->shadow_filter_0.z))
    {

#line 3091
        _S109 = kernelContext_13->frame_0->shadow_filter_0.x;

#line 3091
    }
    else
    {

#line 3091
        _S109 = kernelContext_13->frame_0->shadow_filter_0.y;

#line 3091
    }

#line 3091
    return _S109;
}


#line 3114
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3114
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 349
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3184
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S110 = spoke_0.x;

#line 3189
    float _S111 = rotation_0.x;

#line 3189
    float _S112 = spoke_0.y;

#line 3189
    float _S113 = rotation_0.y;


    float _S114 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S110 * _S111 - _S112 * _S113, _S110 * _S113 + _S112 * _S111) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3192
    return _S114;
}


#line 3272
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_17)
{

#line 3272
    float4 _S115 = atlas_rect_1(tile_2, kernelContext_17);


    if(atlas_rect_is_empty_0(_S115))
    {
        return 1.0f;
    }

#line 3277
    float2 _S116 = atlas_step_1(_S115, kernelContext_17);

#line 3277
    int y_1 = int(-1);

#line 3277
    float visibility_0 = 0.0f;

#line 3282
    for(;;)
    {

#line 3282
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3282
            break;
        }

#line 3282
        int x_0 = int(-1);

        for(;;)
        {

#line 3284
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3284
                break;
            }

#line 3284
            float _S117 = tile_tap_0(_S115, _S116, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_17);

            float visibility_1 = visibility_0 + _S117;

#line 3284
            x_0 = x_0 + int(1);

#line 3284
            visibility_0 = visibility_1;

#line 3284
        }

#line 3282
        y_1 = y_1 + int(1);

#line 3282
    }

#line 3290
    return visibility_0 / 9.0f;
}


#line 3047
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3214
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_18)
{
    float2 _S118 = shadow_rotation_0(pixel_3);

#line 3216
    float4 _S119 = atlas_rect_1(tile_3, kernelContext_18);

    if(atlas_rect_is_empty_0(_S119))
    {
        return 1.0f;
    }

#line 3220
    float2 _S120 = atlas_step_1(_S119, kernelContext_18);

#line 3220
    uint spot_0 = 0U;

#line 3220
    float probe_0 = 0.0f;

#line 3225
    for(;;)
    {

#line 3225
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3225
            break;
        }

#line 3225
        float _S121 = tile_tap_0(_S119, _S120, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S118, reference_2, kernelContext_18);

        float probe_1 = probe_0 + _S121;

#line 3225
        spot_0 = spot_0 + 1U;

#line 3225
        probe_0 = probe_1;

#line 3225
    }

#line 3234
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3240
    uint index_2 = 0U;

#line 3240
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3244
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3244
            break;
        }

#line 3244
        float _S122 = tile_tap_0(_S119, _S120, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S118, reference_2, kernelContext_18);

        float visibility_3 = visibility_2 + _S122;

#line 3244
        index_2 = index_2 + 1U;

#line 3244
        visibility_2 = visibility_3;

#line 3244
    }

#line 3249
    return visibility_2 / 32.0f;
}


#line 3325
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_19)
{
    float2 texel_1 = kernelContext_19->frame_0->shadow_params_0.xy;

#line 3327
    float4 _S123 = atlas_rect_0(cascade_0, kernelContext_19);

#line 3327
    float2 _S124 = atlas_step_0(_S123, kernelContext_19);


    float2 _S125 = float2(0.5f, 0.5f) * _S124;


    float2 _S126 = float2(1.0f, 1.0f);

#line 3333
    float2 _S127 = _S126 / texel_1;

#line 3333
    uint index_3 = 0U;

#line 3333
    float sum_2 = 0.0f;

#line 3333
    float found_0 = 0.0f;



    for(;;)
    {

#line 3337
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3337
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S128 = spoke_1.x;

#line 3340
        float _S129 = rotation_1.x;

#line 3340
        float _S130 = spoke_1.y;

#line 3340
        float _S131 = rotation_1.y;

#line 3348
        int3 _S132 = int3(int2(min(atlas_uv_0(_S123, clamp(tile_uv_4 + float2(_S128 * _S129 - _S130 * _S131, _S128 * _S131 + _S130 * _S129) * _S124, _S125, float2(1.0f)  - _S125)) * _S127, _S127 - _S126)), int(0));

#line 3348
        float depth_1 = ((kernelContext_19->shadow_atlas_0).read(vec<uint,2>(((_S132)).xy), uint(((_S132)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3352
            sum_2 = sum_2 + depth_1;

#line 3352
            found_0 = found_1;

#line 3349
        }

#line 3337
        index_3 = index_3 + 1U;

#line 3337
    }

#line 3356
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3367
    float _S133 = 2.0f * kernelContext_19->frame_0->cascade_far_0[cascade_0];

#line 3367
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S133 + 40.0f);

#line 3367
    float _S134 = tile_texels_0(_S123, kernelContext_19);

    return clamp(separation_0 * 0.01999999955296516f / (_S133 / _S134), 2.0f, 8.0f);
}


#line 3421
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_20)
{

#line 3422
    float4 _S135 = atlas_rect_0(cascade_1, kernelContext_20);

#line 3456
    if(atlas_rect_is_empty_0(_S135))
    {


        return 1.0f;
    }
    float _S136 = 2.0f * kernelContext_20->frame_0->cascade_far_0[cascade_1];

#line 3462
    float _S137 = tile_texels_0(_S135, kernelContext_20);

#line 3462
    float texel_world_0 = _S136 / _S137;

#line 3469
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3473
    bool _S138;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3474
        _S138 = true;

#line 3474
    }
    else
    {

#line 3474
        _S138 = (ndc_0.z) <= 0.0f;

#line 3474
    }

#line 3474
    if(_S138)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3484
    uint _S139 = shadow_filter_mode_0(pixel_4, kernelContext_20);

#line 3501
    if(_S139 == 2U)
    {

#line 3501
        float _S140 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_20);

        return _S140;
    }
    if(_S139 == 1U)
    {

#line 3505
        float _S141 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_20);



        return _S141;
    }

    float _S142 = ndc_0.z;

#line 3512
    float _S143 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S142, shadow_rotation_0(pixel_4), kernelContext_20);

#line 3512
    float _S144 = tile_pcf_0(cascade_1, tile_uv_5, _S142, pixel_4, _S143, kernelContext_20);
    return _S144;
}


#line 3592
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_21)
{
    uint cascade_2;

#line 3594
    bool covered_0;

#line 3603
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3615
    float eye_distance_0 = length(world_position_5 - kernelContext_21->frame_0->camera_position_0.xyz);

#line 3615
    uint index_4 = 0U;

#line 3623
    for(;;)
    {

#line 3623
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3623
            covered_0 = false;

#line 3623
            cascade_2 = 1U;

#line 3623
            break;
        }
        if(eye_distance_0 < kernelContext_21->frame_0->cascade_far_0[index_4])
        {

#line 3625
            covered_0 = true;

#line 3625
            cascade_2 = index_4;



            break;
        }

#line 3623
        index_4 = index_4 + 1U;

#line 3623
    }

#line 3632
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 3632
    }

#line 3632
    float _S145 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3639
    uint _S146 = cascade_2 + 1U;

#line 3639
    if(_S146 >= 2U)
    {



        return _S145;
    }

#line 3652
    float band_0 = kernelContext_21->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_21->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S145;
    }

#line 3660
    float _S147 = cascade_visibility_0(_S146, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3671
    return mix(_S145, _S147, blend_0);
}


#line 4859
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_22)
{

#line 4859
    texture2d<float, access::sample> _S148 = kernelContext_22->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S148).get_width(0)),(*((&height_2)) = (_S148).get_height(0));

    int3 _S149 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 4865
    return ((kernelContext_22->contact_shadow_0).read(vec<uint,2>(((_S149)).xy), uint(((_S149)).z)).x);
}


#line 3564
float3 cascade_tint_0(uint cascade_3, float blend_1)
{
    if(cascade_3 >= 2U)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }
    uint _S150 = cascade_3 + 1U;

#line 3570
    if(_S150 >= 2U)
    {


        return CASCADE_TINTS_0[cascade_3];
    }
    return mix(CASCADE_TINTS_0[cascade_3], CASCADE_TINTS_0[_S150], float3(blend_1) );
}


#line 3882
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S151 = axis_2.x;

#line 3885
    float _S152 = axis_2.y;

#line 3885
    bool _S153;

#line 3885
    if(_S151 >= _S152)
    {

#line 3885
        _S153 = _S151 >= (axis_2.z);

#line 3885
    }
    else
    {

#line 3885
        _S153 = false;

#line 3885
    }

#line 3885
    uint _S154;

#line 3885
    if(_S153)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3887
            _S154 = 0U;

#line 3887
        }
        else
        {

#line 3887
            _S154 = 1U;

#line 3887
        }

#line 3887
        return _S154;
    }
    if(_S152 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3891
            _S154 = 2U;

#line 3891
        }
        else
        {

#line 3891
            _S154 = 3U;

#line 3891
        }

#line 3891
        return _S154;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3893
        _S154 = 4U;

#line 3893
    }
    else
    {

#line 3893
        _S154 = 5U;

#line 3893
    }

#line 3893
    return _S154;
}


#line 336
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 3778
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_23)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 3781
    float4 _S155 = atlas_rect_0(atlas_0, kernelContext_23);

    if(atlas_rect_is_empty_0(_S155))
    {


        return 1.0f;
    }

#line 3787
    float _S156 = tile_texels_0(_S155, kernelContext_23);

    float texel_world_1 = map_world_0 / _S156;

#line 3799
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 3806
    float _S157 = clip_1.w;

#line 3806
    if(_S157 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S157) ;

#line 3810
    bool _S158;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3811
        _S158 = true;

#line 3811
    }
    else
    {

#line 3811
        _S158 = (ndc_1.z) <= 0.0f;

#line 3811
    }

#line 3811
    if(_S158)
    {

#line 3811
        _S158 = true;

#line 3811
    }
    else
    {

#line 3811
        _S158 = (ndc_1.z) > 1.0f;

#line 3811
    }

#line 3811
    if(_S158)
    {

#line 3818
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 3823
    uint _S159 = shadow_filter_mode_0(pixel_6, kernelContext_23);

#line 3832
    if(_S159 == 2U)
    {

#line 3832
        float _S160 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_23);

        return _S160;
    }

#line 3834
    float _S161 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_23);

    return _S161;
}


#line 3901
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_24)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3909
    float _S162 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_24);

#line 3915
    return _S162;
}


#line 3843
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_25)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3850
    float4 _S163 = float4(light_2->direction_0) ;

#line 3857
    float cos_outer_1 = _S163.w;

#line 3857
    float _S164 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S163.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_25);

#line 3864
    return _S164;
}


#line 2214
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 4846
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 4848
    float3 _S165;
    if((length(decoded_0)) < 0.5f)
    {

#line 4849
        _S165 = shading_normal_1;

#line 4849
    }
    else
    {

#line 4849
        _S165 = normalize(decoded_0);

#line 4849
    }

#line 4849
    return _S165;
}


#line 4484
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_26)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_26->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4388
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4388
    float reach_0 = 0.0f;

#line 4388
    uint axis_3 = 0U;


    for(;;)
    {

#line 4391
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4391
            break;
        }

#line 4391
        uint _S166 = axis_3;

#line 4391
        bool _S167;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4393
            _S167 = true;

#line 4393
        }
        else
        {

#line 4393
            _S167 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4393
        }

#line 4393
        if(_S167)
        {

#line 4394
            axis_3 = axis_3 + 1U;

#line 4391
            continue;
        }

#line 4391
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S166] - 1.0f));

#line 4391
        axis_3 = axis_3 + 1U;

#line 4391
    }

#line 4398
    return reach_0;
}


#line 4418
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4418
    uint level_0 = 0U;

    for(;;)
    {

#line 4420
        uint _S168 = level_0 + 1U;

#line 4420
        if(_S168 < levels_0)
        {
        }
        else
        {

#line 4420
            break;
        }
        float _S169 = float(level_0);

#line 4422
        float at_3 = reach_1 * exp2(- _S169);
        if(at_3 < 1.0f)
        {

#line 4424
            return float2(_S169, saturate((1.0f - at_3) / 0.25f));
        }

#line 4420
        level_0 = _S168;

#line 4420
    }

#line 4426
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4175
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4177
    uint _S170;
    if(at_4 >= count_2)
    {

#line 4178
        _S170 = at_4 - count_2;

#line 4178
    }
    else
    {

#line 4178
        _S170 = at_4;

#line 4178
    }

#line 4178
    return _S170;
}


#line 4201
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_27)
{
    uint3 counts_0 = kernelContext_27->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_27->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S171 = counts_0.x;
    uint _S172 = counts_0.y;



    return min(kernelContext_27->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S172 + probe_wrap_0(cell_2.y, offset_1.y, _S172)) * _S171 + probe_wrap_0(cell_2.x, offset_1.x, _S171), max(kernelContext_27->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4042
float sign_not_zero_0(float value_0)
{

#line 4042
    float _S173;

    if(value_0 >= 0.0f)
    {

#line 4044
        _S173 = 1.0f;

#line 4044
    }
    else
    {

#line 4044
        _S173 = -1.0f;

#line 4044
    }

#line 4044
    return _S173;
}


#line 4061
float2 oct_encode_0(float3 direction_1)
{
    float _S174 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S174) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4064
    float2 p_1;
    if(_S174 < 0.0f)
    {
        float _S175 = p_0.y;

#line 4067
        float _S176 = p_0.x;

#line 4067
        p_1 = float2((1.0f - abs(_S175)) * sign_not_zero_0(_S176), (1.0f - abs(_S176)) * sign_not_zero_0(_S175));

#line 4065
    }
    else
    {

#line 4065
        p_1 = p_0;

#line 4065
    }

#line 4070
    return p_1;
}


#line 4090
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_28)
{

#line 4090
    texture2d_array<float, access::sample> _S177 = kernelContext_28->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S177).get_width(0)),(*((&height_3)) = (_S177).get_height(0)),(*((&layers_0)) = (_S177).get_array_size());

#line 4095
    float2 _S178 = float2(0.5f) ;

#line 4095
    float2 _S179 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S178 + _S178) * float2(16.0f)  + _S179 - _S178;
    float2 _S180 = float2(float(width_3), float(height_3)) - _S179;

#line 4099
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S180);
    float2 high_2 = min(low_2 + _S179, _S180);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S181 = int(low_2.x);

#line 4104
    int _S182 = int(low_2.y);

#line 4104
    int4 _S183 = int4(_S181, _S182, layer_1, int(0));
    int _S184 = int(high_2.x);

#line 4105
    int4 _S185 = int4(_S184, _S182, layer_1, int(0));
    int _S186 = int(high_2.y);

#line 4106
    int4 _S187 = int4(_S181, _S186, layer_1, int(0));
    int4 _S188 = int4(_S184, _S186, layer_1, int(0));
    float2 _S189 = float2(weight_2.x) ;

#line 4108
    return mix(mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S183)).xy), uint(((_S183)).z), uint(((_S183)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S185)).xy), uint(((_S185)).z), uint(((_S185)).w))).xy, _S189), mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S187)).xy), uint(((_S187)).z), uint(((_S187)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S188)).xy), uint(((_S188)).z), uint(((_S188)).w))).xy, _S189), float2(weight_2.y) );
}


#line 4136
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_7, KernelContext_0 thread* kernelContext_29)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_7 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4139
    float2 _S190 = probe_moments_0(index_6, - to_probe_0, kernelContext_29);

#line 4145
    float _S191 = _S190.x;

#line 4145
    float _S192 = max(_S190.y - _S191 * _S191, 0.0f);
    float behind_0 = to_surface_0 - _S191;
    float bound_0 = _S192 / (_S192 + behind_0 * behind_0);

#line 4147
    float _S193;
    if(to_surface_0 <= _S191)
    {

#line 4148
        _S193 = 1.0f;

#line 4148
    }
    else
    {

#line 4148
        _S193 = bound_0 * bound_0 * bound_0;

#line 4148
    }

#line 4148
    return _S193;
}


#line 4158
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_8, KernelContext_0 thread* kernelContext_30)
{

#line 4158
    float _S194 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_8, kernelContext_30);

    return max(_S194, 0.00009999999747379f);
}


#line 1096
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4220
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4247
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_9, KernelContext_0 thread* kernelContext_31)
{

#line 4248
    uint _S195 = probe_row_0(level_2, cell_3, kernelContext_31);


    GpuProbe_natural_0 stored_0 = kernelContext_31->probes_0[_S195];

#line 4251
    float _S196 = probe_weight_0(_S195, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_9, kernelContext_31);



    thread WeightedProbe_0 corner_2;

#line 4255
    float4 _S197 = float4(_S196) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S197;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S197;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S197;
    (&corner_2)->weight_3 = _S196;
    return corner_2;
}


#line 4231
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S198 = float4(t_1) ;

#line 4234
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S198);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S198);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S198);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4319
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_10, KernelContext_0 thread* kernelContext_32)
{

#line 4319
    float3 _S199 = float3(1.0f) ;

#line 4324
    float3 _S200 = float3(0.0f, 0.0f, 0.0f);

#line 4324
    float3 last_1 = max(float3(kernelContext_32->frame_0->probe_counts_0.xyz) - _S199, _S200);



    float3 origin_2 = kernelContext_32->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_32->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S200, last_1);
    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S201 = uint3(base_2);



    uint3 _S202 = uint3(min(base_2 + _S199, last_1));

#line 4344
    float _S203 = inv_0.x;

#line 4344
    float _S204;

#line 4344
    if(_S203 != 0.0f)
    {

#line 4344
        _S204 = 1.0f / _S203;

#line 4344
    }
    else
    {

#line 4344
        _S204 = 0.0f;

#line 4344
    }
    float _S205 = inv_0.y;

#line 4345
    float _S206;

#line 4345
    if(_S205 != 0.0f)
    {

#line 4345
        _S206 = 1.0f / _S205;

#line 4345
    }
    else
    {

#line 4345
        _S206 = 0.0f;

#line 4345
    }
    float _S207 = inv_0.z;

#line 4346
    float _S208;

#line 4346
    if(_S207 != 0.0f)
    {

#line 4346
        _S208 = 1.0f / _S207;

#line 4346
    }
    else
    {

#line 4346
        _S208 = 0.0f;

#line 4346
    }

#line 4344
    float3 spacing_1 = float3(_S204, _S206, _S208);

#line 4353
    uint _S209 = _S201.x;

#line 4353
    uint _S210 = _S201.y;

#line 4353
    uint _S211 = _S201.z;

#line 4353
    WeightedProbe_0 _S212 = probe_corner_0(level_3, uint3(_S209, _S210, _S211), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);
    uint _S213 = _S202.x;

#line 4354
    WeightedProbe_0 _S214 = probe_corner_0(level_3, uint3(_S213, _S210, _S211), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4354
    float _S215 = f_0.x;

#line 4354
    thread WeightedProbe_0 _S216 = _S212;

#line 4354
    thread WeightedProbe_0 _S217 = _S214;

#line 4354
    WeightedProbe_0 _S218 = lerp_probe_0(&_S216, &_S217, _S215);
    uint _S219 = _S202.y;

#line 4355
    WeightedProbe_0 _S220 = probe_corner_0(level_3, uint3(_S209, _S219, _S211), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4355
    WeightedProbe_0 _S221 = probe_corner_0(level_3, uint3(_S213, _S219, _S211), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4355
    thread WeightedProbe_0 _S222 = _S220;

#line 4355
    thread WeightedProbe_0 _S223 = _S221;

#line 4355
    WeightedProbe_0 _S224 = lerp_probe_0(&_S222, &_S223, _S215);

    uint _S225 = _S202.z;

#line 4357
    WeightedProbe_0 _S226 = probe_corner_0(level_3, uint3(_S209, _S210, _S225), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4357
    WeightedProbe_0 _S227 = probe_corner_0(level_3, uint3(_S213, _S210, _S225), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4357
    thread WeightedProbe_0 _S228 = _S226;

#line 4357
    thread WeightedProbe_0 _S229 = _S227;

#line 4357
    WeightedProbe_0 _S230 = lerp_probe_0(&_S228, &_S229, _S215);

#line 4357
    WeightedProbe_0 _S231 = probe_corner_0(level_3, uint3(_S209, _S219, _S225), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4357
    WeightedProbe_0 _S232 = probe_corner_0(level_3, uint3(_S213, _S219, _S225), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4357
    thread WeightedProbe_0 _S233 = _S231;

#line 4357
    thread WeightedProbe_0 _S234 = _S232;

#line 4357
    WeightedProbe_0 _S235 = lerp_probe_0(&_S233, &_S234, _S215);



    float _S236 = f_0.y;

#line 4361
    thread WeightedProbe_0 _S237 = _S218;

#line 4361
    thread WeightedProbe_0 _S238 = _S224;

#line 4361
    WeightedProbe_0 _S239 = lerp_probe_0(&_S237, &_S238, _S236);

#line 4361
    thread WeightedProbe_0 _S240 = _S230;

#line 4361
    thread WeightedProbe_0 _S241 = _S235;

#line 4361
    WeightedProbe_0 _S242 = lerp_probe_0(&_S240, &_S241, _S236);

    float _S243 = f_0.z;

#line 4363
    thread WeightedProbe_0 _S244 = _S239;

#line 4363
    thread WeightedProbe_0 _S245 = _S242;

#line 4363
    WeightedProbe_0 _S246 = lerp_probe_0(&_S244, &_S245, _S243);

    float4 basis_7 = float4(normal_10, 1.0f);
    return max(float3(dot(_S246.sh_0.sh_r_0, basis_7), dot(_S246.sh_0.sh_g_0, basis_7), dot(_S246.sh_0.sh_b_0, basis_7)) / float3(_S246.weight_3) , _S200);
}


#line 4453
float3 probe_irradiance_0(float3 world_position_14, float3 normal_11, KernelContext_0 thread* kernelContext_33)
{

#line 4461
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_33->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_33->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_33->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_33->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 4463
    float3 _S247 = probe_level_irradiance_0(level_4, world_position_14, normal_11, kernelContext_33);


    if(share_0 >= 1.0f)
    {

#line 4467
        return _S247;
    }

#line 4467
    float3 _S248 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_11, kernelContext_33);

    return _S248 * float3((1.0f - share_0))  + _S247 * float3(share_0) ;
}


#line 4915
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 4915
    float3 _S249 = float3(visibility_4) ;

#line 4921
    return min(float3(1.0f) , max(_S249, ((_S249 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S249 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S249));
}


#line 1069
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2565
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S250 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2573
    float kernel_0 = 0.0001984127011383f;

#line 2573
    int term_0 = int(6);

    for(;;)
    {

#line 2575
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2575
            break;
        }
        float _S251 = kernel_0 * _S250 + FOG_KERNEL_0[term_0];

#line 2575
        int term_1 = term_0 - int(1);

#line 2575
        kernel_0 = _S251;

#line 2575
        term_0 = term_1;

#line 2575
    }

#line 2582
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2592
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S252 = - d_0;

#line 2596
        float series_0 = 0.00833333376795053f;

#line 2596
        int term_2 = int(3);

        for(;;)
        {

#line 2598
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2598
                break;
            }
            float _S253 = series_0 * _S252 + FOG_RATIO_KERNEL_0[term_2];

#line 2598
            int term_3 = term_2 - int(1);

#line 2598
            series_0 = _S253;

#line 2598
            term_2 = term_3;

#line 2598
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2626
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2637
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2645
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4510
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4510
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


#line 4957
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S254 [[stage_in]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 4957
    thread KernelContext_0 kernelContext_34;

#line 4957
    (&kernelContext_34)->draw_0 = draw_3;

#line 4957
    (&kernelContext_34)->visible_instances_0 = visible_instances_3;

#line 4957
    (&kernelContext_34)->instances_0 = instances_3;

#line 4957
    (&kernelContext_34)->meshes_0 = meshes_3;

#line 4957
    (&kernelContext_34)->frame_0 = frame_5;

#line 4957
    (&kernelContext_34)->vertices_0 = vertices_3;

#line 4957
    (&kernelContext_34)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4957
    (&kernelContext_34)->materials_0 = materials_3;

#line 4957
    (&kernelContext_34)->normal_textures_0 = normal_textures_3;

#line 4957
    (&kernelContext_34)->base_color_sampler_0 = base_color_sampler_3;

#line 4957
    (&kernelContext_34)->base_color_textures_0 = base_color_textures_3;

#line 4957
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_3;

#line 4957
    (&kernelContext_34)->specular_dfg_0 = specular_dfg_3;

#line 4957
    (&kernelContext_34)->lights_0 = lights_3;

#line 4957
    (&kernelContext_34)->ltc_matrix_0 = ltc_matrix_3;

#line 4957
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_3;

#line 4957
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_3;

#line 4957
    (&kernelContext_34)->contact_shadow_0 = contact_shadow_3;

#line 4957
    (&kernelContext_34)->probes_0 = probes_3;

#line 4957
    (&kernelContext_34)->probe_visibility_0 = probe_visibility_3;

#line 4969
    float3 vertex_normal_0 = normalize(_S254.world_normal_1);

#line 4974
    float2 motion_1 = motion_vector_0(_S254.clip_position_1, _S254.previous_clip_position_1);

#line 4990
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 4992
        float4 _S255 = occlusion_at_0(position_5.xy, &kernelContext_34);



        (&bent_0)->lit_0 = float4(_S255.yzw, 1.0f);


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

#line 5046
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5046
        float4 _S256 = occlusion_at_0(position_5.xy, &kernelContext_34);


        float value_1 = _S256.x;

#line 5048
        thread FragmentOutput_0 occlusion_1;

#line 5057
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S254.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 5074
    thread GpuMaterial_natural_0 _S257 = (&kernelContext_34)->materials_0[_S254.material_5];

#line 5074
    float2 uv_3;

#line 5099
    if(((&_S257)->tiling_0) == 1U)
    {

#line 5099
        uv_3 = physical_tile_uv_0(_S254.world_position_15, vertex_normal_0, (&_S257)->tile_metres_0);

#line 5099
    }
    else
    {

#line 5099
        uv_3 = _S254.uv_2;

#line 5099
    }

#line 5099
    uint _S258 = normal_layer_0(&_S257);

#line 5099
    thread VertexOutput_0 _S259;

#line 5099
    (&_S259)->position_3 = position_5;

#line 5099
    (&_S259)->world_position_1 = _S254.world_position_15;

#line 5099
    (&_S259)->world_normal_0 = _S254.world_normal_1;

#line 5099
    (&_S259)->color_2 = _S254.color_3;

#line 5099
    (&_S259)->material_2 = _S254.material_5;

#line 5099
    (&_S259)->uv_0 = _S254.uv_2;

#line 5099
    (&_S259)->clip_position_0 = _S254.clip_position_1;

#line 5099
    (&_S259)->previous_clip_position_0 = _S254.previous_clip_position_1;

#line 5099
    (&_S259)->world_tangent_0 = _S254.world_tangent_1;

#line 5099
    (&_S259)->frame_3 = _S254.frame_4;

#line 5099
    float3 _S260 = shading_normal_of_0(_S258, (&_S257)->normal_scale_0, &_S259, vertex_normal_0, uv_3, &kernelContext_34);

#line 5106
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5108
        float3 _S261 = float3(0.5f) ;

#line 5120
        (&normals_0)->lit_0 = float4(_S260 * _S261 + _S261, 1.0f);

#line 5126
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_34)->frame_0->camera_position_0.xyz - _S254.world_position_15);



    float3 _S262 = geometric_normal_of_0(_S254.world_position_15, vertex_normal_0);

#line 5135
    uint _S263 = base_color_layer_0(&_S257);

#line 5150
    float3 _S264 = float3(uv_3, float(_S263));
    float4 albedo_1 = _S254.color_3 * float4((&_S257)->base_color_0)  * (((&kernelContext_34)->base_color_textures_0).sample(((&kernelContext_34)->base_color_sampler_0), ((_S264)).xy, uint(((_S264)).z)));

#line 5157
    float metallic_1 = saturate((&_S257)->metallic_0);
    float roughness_2 = clamp((&_S257)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S265 = alpha_0 * alpha_0;

#line 5166
    float3 _S266 = albedo_1.xyz;

#line 5166
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S266, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S266 * float3((1.0f - metallic_1)) ;

#line 5173
    float _S267 = max(dot(_S260, to_eye_1), 0.00009999999747379f);

#line 5183
    float2 _S268 = position_5.xy;

#line 5183
    uint _S269 = froxel_of_0(_S268, (((float4(_S254.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_34);

#line 5183
    uint base_3 = _S269 * 17U;

#line 5188
    uint _S270 = min((&kernelContext_34)->cluster_lights_0[base_3], 16U);

#line 5188
    TableTap_0 _S271 = table_tap_0(_S267, roughness_2, &kernelContext_34);

#line 5188
    thread TableTap_0 _S272 = _S271;

#line 5188
    float2 _S273 = dfg_at_0(&_S272, &kernelContext_34);

#line 5197
    float _S274 = _S273.x;

#line 5197
    float _S275 = _S273.y;

#line 5197
    float3 _S276 = f0_2 * float3(_S274)  + float3(_S275) ;

#line 5203
    float3 _S277 = float3(0.0f, 0.0f, 0.0f);

#line 5203
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5203
    uint slot_0 = 0U;

#line 5203
    float3 direct_0 = _S277;

#line 5203
    float3 gloss_0 = _S277;

#line 5213
    for(;;)
    {

#line 5213
        if(slot_0 < _S270)
        {
        }
        else
        {

#line 5213
            break;
        }

#line 5213
        thread GpuLight_natural_0 _S278 = (&kernelContext_34)->lights_0[(&kernelContext_34)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 5213
        uint _S279 = (&_S278)->kind_0;

#line 5222
        bool _S280 = ((&_S278)->kind_0) == 0U;

#line 5222
        float3 to_light_7;

#line 5222
        float reach_2;

#line 5222
        if(_S280)
        {

#line 5222
            to_light_7 = normalize((float4((&_S278)->direction_0) ).xyz);

#line 5222
            reach_2 = 1.0f;

#line 5222
        }
        else
        {


            if(_S279 == 3U)
            {

#line 5227
                float4 _S281 = float4((&_S278)->position_0) ;

#line 5235
                float3 offset_2 = _S281.xyz - _S254.world_position_15;
                float distance_3 = length(offset_2);

                float _S282 = range_window_0(distance_3, _S281.w);

#line 5238
                to_light_7 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5238
                reach_2 = _S282;

#line 5227
            }
            else
            {

#line 5227
                float4 _S283 = float4((&_S278)->position_0) ;

#line 5242
                float3 offset_3 = _S283.xyz - _S254.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_8 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S283.w);
                if(_S279 == 2U)
                {

#line 5246
                    float4 _S284 = float4((&_S278)->direction_0) ;

#line 5246
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S284.xyz, _S284.w, (&_S278)->cos_inner_0);

#line 5246
                }
                else
                {

#line 5246
                    reach_2 = reach_3;

#line 5246
                }

#line 5246
                to_light_7 = to_light_8;

#line 5227
            }

#line 5222
        }

#line 5255
        float n_dot_l_5 = dot(_S260, to_light_7);

#line 5255
        float3 specular_0;

#line 5255
        float diffuse_0;


        if(_S279 == 3U)
        {

#line 5268
            thread array<float3, int(4)> corners_2;

#line 5268
            rect_corners_0(&_S278, _S254.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S260, to_eye_1, _S267);

#line 5270
            thread array<float3, int(4)> _S285 = corners_2;

#line 5270
            float _S286 = ltc_irradiance_0(to_local_0, &_S285);

#line 5270
            thread TableTap_0 _S287 = _S271;

#line 5270
            float4 _S288 = ltc_at_0(&_S287, &kernelContext_34);

            matrix<float,int(3),int(3)>  _S289 = (((to_local_0) * (ltc_transform_0(_S288))));

#line 5272
            thread array<float3, int(4)> _S290 = corners_2;

#line 5272
            float _S291 = ltc_irradiance_0(_S289, &_S290);
            float3 _S292 = float3(_S291)  * _S276;

#line 5273
            diffuse_0 = _S286;

#line 5273
            specular_0 = _S292;

#line 5258
        }
        else
        {

#line 5278
            float _S293 = max(n_dot_l_5, 0.0f);

#line 5285
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5293
            float3 specular_1 = ggx_lobe_0(_S265, f0_2, _S293, _S267, max(dot(_S260, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S293) ;

#line 5293
            diffuse_0 = _S293;

#line 5293
            specular_0 = specular_1;

#line 5258
        }

#line 5258
        float3 specular_2;

#line 5301
        if((((&_S278)->flags_3) & 1U) != 0U)
        {

#line 5301
            specular_2 = _S277;

#line 5301
        }
        else
        {

#line 5301
            specular_2 = specular_0;

#line 5301
        }

#line 5301
        float reach_4;

#line 5319
        if(_S280)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 5322
            float _S294 = sun_visibility_0(_S254.world_position_15, to_light_7, n_dot_l_5, _S262, _S268, &sun_cascade_0, &sun_fade_0, &kernelContext_34);

#line 5322
            float _S295 = contact_at_0(_S268, &kernelContext_34);

#line 5331
            float _S296 = _S294 * _S295;

#line 5331
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 5331
            reach_4 = _S296;

#line 5319
        }
        else
        {

#line 5336
            if(_S279 == 1U)
            {

#line 5336
                uint _S297 = (&_S278)->shadow_tile_0;

#line 5348
                if(((&_S278)->shadow_tile_0) <= 8U)
                {

#line 5348
                    float _S298 = point_visibility_0(&_S278, _S297, _S254.world_position_15, to_light_7, n_dot_l_5, _S262, _S268, &kernelContext_34);

#line 5348
                    reach_4 = reach_2 * _S298;

#line 5348
                }
                else
                {

#line 5348
                    reach_4 = reach_2;

#line 5348
                }

#line 5336
            }
            else
            {

#line 5336
                uint _S299 = (&_S278)->shadow_tile_0;

#line 5354
                if(((&_S278)->shadow_tile_0) < 14U)
                {

#line 5354
                    float _S300 = spot_visibility_0(&_S278, _S299, _S254.world_position_15, to_light_7, n_dot_l_5, _S262, _S268, &kernelContext_34);

#line 5354
                    reach_4 = reach_2 * _S300;

#line 5354
                }
                else
                {

#line 5354
                    reach_4 = reach_2;

#line 5354
                }

#line 5336
            }

#line 5319
        }

#line 5362
        float3 _S301 = (float4((&_S278)->color_0) ).xyz;

#line 5362
        float3 direct_1 = direct_0 + _S301 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S301 * (specular_2 * float3(reach_4) );

#line 5213
        slot_0 = slot_0 + 1U;

#line 5213
        direct_0 = direct_1;

#line 5213
        gloss_0 = gloss_1;

#line 5213
    }

#line 5377
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S274 + _S275);

#line 5377
    float4 _S302 = occlusion_at_0(_S268, &kernelContext_34);

#line 5396
    float occluded_0 = _S302.x;

#line 5405
    float3 bent_normal_0 = bent_normal_at_0(_S302, _S260);

#line 5428
    float3 _S303 = frame_5->ambient_0.xyz;

#line 5428
    float3 _S304 = sky_irradiance_0(bent_normal_0, &kernelContext_34);

#line 5428
    float3 _S305 = _S303 + _S304;

#line 5428
    float3 _S306 = probe_irradiance_0(_S254.world_position_15, bent_normal_0, &kernelContext_34);

#line 5464
    float3 lit_1 = diffuse_albedo_0 * ((_S305 + _S306) * multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) + direct_0) + gloss_2;

#line 5464
    float3 _S307 = emissive_of_0(&_S257);

#line 5500
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_34)->frame_0->fog_params_0.x, (&kernelContext_34)->frame_0->fog_params_0.y, (&kernelContext_34)->frame_0->camera_position_0.y - (&kernelContext_34)->frame_0->fog_params_0.z, _S254.world_position_15.y - (&kernelContext_34)->frame_0->fog_params_0.z, length((&kernelContext_34)->frame_0->camera_position_0.xyz - _S254.world_position_15)));
    float3 lit_2 = (lit_1 + _S307) * float3(fog_survives_0)  + (&kernelContext_34)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;



    float _S308 = albedo_1.w;

#line 5507
    (&output_2)->lit_0 = float4(lit_2, _S308);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 5520
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, _S308);

#line 5529
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 5520
    }

#line 5531
    return output_2;
}


#line 5562
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_12 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 5562
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


#line 5605
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_1 _S309 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 5605
    thread KernelContext_0 kernelContext_35;

#line 5605
    (&kernelContext_35)->draw_0 = draw_4;

#line 5605
    (&kernelContext_35)->visible_instances_0 = visible_instances_4;

#line 5605
    (&kernelContext_35)->instances_0 = instances_4;

#line 5605
    (&kernelContext_35)->meshes_0 = meshes_4;

#line 5605
    (&kernelContext_35)->frame_0 = frame_7;

#line 5605
    (&kernelContext_35)->vertices_0 = vertices_4;

#line 5605
    (&kernelContext_35)->ambient_occlusion_0 = ambient_occlusion_4;

#line 5605
    (&kernelContext_35)->materials_0 = materials_4;

#line 5605
    (&kernelContext_35)->normal_textures_0 = normal_textures_4;

#line 5605
    (&kernelContext_35)->base_color_sampler_0 = base_color_sampler_4;

#line 5605
    (&kernelContext_35)->base_color_textures_0 = base_color_textures_4;

#line 5605
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_4;

#line 5605
    (&kernelContext_35)->specular_dfg_0 = specular_dfg_4;

#line 5605
    (&kernelContext_35)->lights_0 = lights_4;

#line 5605
    (&kernelContext_35)->ltc_matrix_0 = ltc_matrix_4;

#line 5605
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_4;

#line 5605
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_4;

#line 5605
    (&kernelContext_35)->contact_shadow_0 = contact_shadow_4;

#line 5605
    (&kernelContext_35)->probes_0 = probes_4;

#line 5605
    (&kernelContext_35)->probe_visibility_0 = probe_visibility_4;

#line 5610
    float3 vertex_normal_1 = normalize(_S309.world_normal_2);

#line 5610
    thread GpuMaterial_natural_0 _S310 = materials_4[_S309.material_6];

#line 5610
    float2 uv_5;

#line 5617
    if(((&_S310)->tiling_0) == 1U)
    {

#line 5617
        uv_5 = physical_tile_uv_0(_S309.world_position_16, vertex_normal_1, (&_S310)->tile_metres_0);

#line 5617
    }
    else
    {

#line 5617
        uv_5 = _S309.uv_4;

#line 5617
    }

#line 5617
    uint _S311 = base_color_layer_0(&_S310);

#line 5622
    float3 _S312 = float3(uv_5, float(_S311));


    thread RsmOutput_0 written_0;



    (&written_0)->albedo_2 = float4((_S309.color_4 * float4((&_S310)->base_color_0)  * (((&kernelContext_35)->base_color_textures_0).sample(((&kernelContext_35)->base_color_sampler_0), ((_S312)).xy, uint(((_S312)).z)))).xyz * float3((1.0f - saturate((&_S310)->metallic_0))) , 1.0f);

#line 5629
    float3 _S313 = float3(0.5f) ;
    (&written_0)->normal_12 = float4(vertex_normal_1 * _S313 + _S313, 1.0f);
    (&written_0)->world_0 = float4(_S309.world_position_16, 1.0f);
    return written_0;
}


#line 5632
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


#line 5632
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 5632
    thread KernelContext_0 kernelContext_36;

#line 5632
    (&kernelContext_36)->draw_0 = draw_5;

#line 5632
    (&kernelContext_36)->visible_instances_0 = visible_instances_5;

#line 5632
    (&kernelContext_36)->instances_0 = instances_5;

#line 5632
    (&kernelContext_36)->meshes_0 = meshes_5;

#line 5632
    (&kernelContext_36)->frame_0 = frame_9;

#line 5632
    (&kernelContext_36)->vertices_0 = vertices_5;

#line 5632
    (&kernelContext_36)->ambient_occlusion_0 = ambient_occlusion_5;

#line 5632
    (&kernelContext_36)->materials_0 = materials_5;

#line 5632
    (&kernelContext_36)->normal_textures_0 = normal_textures_5;

#line 5632
    (&kernelContext_36)->base_color_sampler_0 = base_color_sampler_5;

#line 5632
    (&kernelContext_36)->base_color_textures_0 = base_color_textures_5;

#line 5632
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_5;

#line 5632
    (&kernelContext_36)->specular_dfg_0 = specular_dfg_5;

#line 5632
    (&kernelContext_36)->lights_0 = lights_5;

#line 5632
    (&kernelContext_36)->ltc_matrix_0 = ltc_matrix_5;

#line 5632
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_5;

#line 5632
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_5;

#line 5632
    (&kernelContext_36)->contact_shadow_0 = contact_shadow_5;

#line 5632
    (&kernelContext_36)->probes_0 = probes_5;

#line 5632
    (&kernelContext_36)->probe_visibility_0 = probe_visibility_5;

#line 5632
    GpuInstance_natural_0 device* _S314 = instances_5+visible_instances_5[draw_5->base_0 + instance_id_1];

#line 1855
    GpuMesh_0 mesh_3 = meshes_5[draw_5->mesh_0];

#line 1863
    bool _S315 = ((_S314->flags_0) & 2U) != 0U;

#line 1863
    uint base_vertex_3;
    if(_S315)
    {

#line 1864
        base_vertex_3 = _S314->base_vertex_0;

#line 1864
    }
    else
    {

#line 1864
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1864
    }

#line 1864
    MeshVertex_0 _S316 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_36);

#line 1864
    uint previous_base_0;

#line 1877
    if(_S315)
    {

#line 1877
        previous_base_0 = _S314->previous_base_vertex_0;

#line 1877
    }
    else
    {

#line 1877
        previous_base_0 = base_vertex_3;

#line 1877
    }

#line 1877
    float3 _S317 = load_position_0(index_8 + previous_base_0, &kernelContext_36);

#line 1877
    matrix<float,int(4),int(4)>  _S318 = matrix<float,int(4),int(4)> (_S314->transform_0.data_0[int(0)][int(0)], _S314->transform_0.data_0[int(1)][int(0)], _S314->transform_0.data_0[int(2)][int(0)], _S314->transform_0.data_0[int(3)][int(0)], _S314->transform_0.data_0[int(0)][int(1)], _S314->transform_0.data_0[int(1)][int(1)], _S314->transform_0.data_0[int(2)][int(1)], _S314->transform_0.data_0[int(3)][int(1)], _S314->transform_0.data_0[int(0)][int(2)], _S314->transform_0.data_0[int(1)][int(2)], _S314->transform_0.data_0[int(2)][int(2)], _S314->transform_0.data_0[int(3)][int(2)], _S314->transform_0.data_0[int(0)][int(3)], _S314->transform_0.data_0[int(1)][int(3)], _S314->transform_0.data_0[int(2)][int(3)], _S314->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S316.position_1, 1.0f)) * (_S318)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 1891
    matrix<float,int(3),int(3)>  _S319 = matrix<float,int(3),int(3)> (_S318[int(0)].xyz, _S318[int(1)].xyz, _S318[int(2)].xyz);

#line 1891
    (&output_3)->world_normal_0 = (((_S316.basis_1.normal_0) * (normal_basis_0(_S319))));

#line 1897
    (&output_3)->world_tangent_0 = (((_S316.basis_1.tangent_1) * (_S319)));

#line 1897
    thread TangentFrame_0 _S320 = _S316.basis_1;

#line 1897
    uint _S321 = frame_word_0(mesh_3.flags_1, &_S320);
    (&output_3)->frame_3 = _S321;

#line 1898
    float4 _S322;

#line 1905
    if(((&kernelContext_36)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1905
        _S322 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1905
    }
    else
    {

#line 1905
        _S322 = _S316.color_1;

#line 1905
    }

#line 1904
    (&output_3)->color_2 = _S322;

#line 1911
    (&output_3)->material_2 = _S314->material_0;
    (&output_3)->uv_0 = _S316.uv0_0;

#line 1918
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S317, 1.0f)) * (matrix<float,int(4),int(4)> (_S314->previous_transform_0.data_0[int(0)][int(0)], _S314->previous_transform_0.data_0[int(1)][int(0)], _S314->previous_transform_0.data_0[int(2)][int(0)], _S314->previous_transform_0.data_0[int(3)][int(0)], _S314->previous_transform_0.data_0[int(0)][int(1)], _S314->previous_transform_0.data_0[int(1)][int(1)], _S314->previous_transform_0.data_0[int(2)][int(1)], _S314->previous_transform_0.data_0[int(3)][int(1)], _S314->previous_transform_0.data_0[int(0)][int(2)], _S314->previous_transform_0.data_0[int(1)][int(2)], _S314->previous_transform_0.data_0[int(2)][int(2)], _S314->previous_transform_0.data_0[int(3)][int(2)], _S314->previous_transform_0.data_0[int(0)][int(3)], _S314->previous_transform_0.data_0[int(1)][int(3)], _S314->previous_transform_0.data_0[int(2)][int(3)], _S314->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S323 = output_3;

#line 1922
    thread vertexMain_Result_0 _S324;

#line 1922
    (&_S324)->position_7 = _S323.position_3;

#line 1922
    (&_S324)->world_position_17 = _S323.world_position_1;

#line 1922
    (&_S324)->world_normal_3 = _S323.world_normal_0;

#line 1922
    (&_S324)->color_5 = _S323.color_2;

#line 1922
    (&_S324)->material_7 = _S323.material_2;

#line 1922
    (&_S324)->uv_6 = _S323.uv_0;

#line 1922
    (&_S324)->clip_position_3 = _S323.clip_position_0;

#line 1922
    (&_S324)->previous_clip_position_3 = _S323.previous_clip_position_0;

#line 1922
    (&_S324)->world_tangent_3 = _S323.world_tangent_0;

#line 1922
    (&_S324)->frame_8 = _S323.frame_3;

#line 1922
    return _S324;
}

