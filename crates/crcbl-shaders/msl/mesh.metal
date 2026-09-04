#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2530 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2525
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 3527
constant array<float3, int(2)> CASCADE_TINTS_0 = { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) };

#line 3010
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2797
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2857
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2872
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2900
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1190
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1834
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1834
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


#line 859
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


#line 1840
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1840
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


#line 1233
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


#line 1244
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1247
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1698
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1821
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1821
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1823
        word_4 = 1U;

#line 1823
    }
    else
    {

#line 1823
        word_4 = 0U;

#line 1823
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1827
        word_4 = word_4 | 2U;

#line 1827
    }

#line 1826
    return word_4;
}


#line 1826
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1941
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 1941
    thread KernelContext_0 kernelContext_2;

#line 1941
    (&kernelContext_2)->draw_0 = draw_1;

#line 1941
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1941
    (&kernelContext_2)->instances_0 = instances_1;

#line 1941
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1941
    (&kernelContext_2)->frame_0 = frame_1;

#line 1941
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1941
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1941
    (&kernelContext_2)->materials_0 = materials_1;

#line 1941
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1941
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1941
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1941
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1941
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1941
    (&kernelContext_2)->lights_0 = lights_1;

#line 1941
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1941
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1941
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1941
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 1941
    (&kernelContext_2)->probes_0 = probes_1;

#line 1941
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 1941
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1944
    uint base_vertex_2;

#line 1950
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1950
        base_vertex_2 = _S7->base_vertex_0;

#line 1950
    }
    else
    {

#line 1950
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1950
    }

#line 1950
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1950
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1950
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 1953
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 1974
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 1974
    thread KernelContext_0 kernelContext_3;

#line 1974
    (&kernelContext_3)->draw_0 = draw_2;

#line 1974
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 1974
    (&kernelContext_3)->instances_0 = instances_2;

#line 1974
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 1974
    (&kernelContext_3)->frame_0 = frame_2;

#line 1974
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 1974
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1974
    (&kernelContext_3)->materials_0 = materials_2;

#line 1974
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 1974
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 1974
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 1974
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 1974
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 1974
    (&kernelContext_3)->lights_0 = lights_2;

#line 1974
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 1974
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 1974
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 1974
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 1974
    (&kernelContext_3)->probes_0 = probes_2;

#line 1974
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 1974
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 4903
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 4905
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4779
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4779
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4785
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4513
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4517
    float _S16 = axis_0.y;

#line 4517
    bool _S17;

#line 4517
    if(_S15 >= _S16)
    {

#line 4517
        _S17 = _S15 >= (axis_0.z);

#line 4517
    }
    else
    {

#line 4517
        _S17 = false;

#line 4517
    }

#line 4517
    float2 planar_0;

#line 4517
    if(_S17)
    {

#line 4517
        planar_0 = world_position_0.zy;

#line 4517
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4521
            planar_0 = world_position_0.xz;

#line 4521
        }
        else
        {

#line 4521
            planar_0 = world_position_0.xy;

#line 4521
        }

#line 4517
    }

#line 4529
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1044
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 4550
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S18 = normal_2.z;

#line 4552
    float sign_z_0;

#line 4552
    if(_S18 >= 0.0f)
    {

#line 4552
        sign_z_0 = 1.0f;

#line 4552
    }
    else
    {

#line 4552
        sign_z_0 = -1.0f;

#line 4552
    }
    float a_0 = -1.0f / (sign_z_0 + _S18);
    float _S19 = normal_2.x;

#line 4554
    float _S20 = sign_z_0 * _S19;

#line 4554
    return float3(1.0f + _S20 * _S19 * a_0, _S20 * normal_2.y * a_0, - sign_z_0 * _S19);
}


#line 4604
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S21 = duvdy_0.y;

#line 4606
    float _S22 = duvdx_0.y;

#line 4606
    float winding_0;
    if((duvdx_0.x * _S21 - duvdy_0.x * _S22) < 0.0f)
    {

#line 4607
        winding_0 = -1.0f;

#line 4607
    }
    else
    {

#line 4607
        winding_0 = 1.0f;

#line 4607
    }
    float3 tangent_2 = (float3(_S21)  * dpdx_0 - float3(_S22)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 4616
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 4617
    float3 _S23;

#line 4626
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 4626
        _S23 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 4626
    }
    else
    {

#line 4626
        _S23 = orthonormal_tangent_0(normal_3);

#line 4626
    }

#line 4626
    (&basis_4)->tangent_1 = _S23;

    (&basis_4)->bitangent_0 = cross(normal_3, _S23);
    return basis_4;
}


#line 1705
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


#line 4686
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 4698
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 4708
    uint _S24 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4717
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4719
        float3 _S25;

#line 4724
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4724
            _S25 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4724
        }
        else
        {

#line 4724
            _S25 = orthonormal_tangent_0(normal_4);

#line 4724
        }

#line 4724
        (&basis_5)->tangent_1 = _S25;

#line 4730
        float3 _S26 = cross((&basis_5)->normal_0, _S25);

#line 4730
        float _S27;
        if((_S24 & 2U) != 0U)
        {

#line 4731
            _S27 = -1.0f;

#line 4731
        }
        else
        {

#line 4731
            _S27 = 1.0f;

#line 4731
        }

#line 4730
        (&basis_5)->bitangent_0 = _S26 * float3(_S27) ;

#line 4709
    }
    else
    {

#line 4735
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 4709
    }

#line 4739
    float3 _S28 = float3(uv_1, float(layer_0));
    float3 _S29 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S28)).xy, uint(((_S28)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4740
    thread float3 tangent_space_0 = _S29;
    tangent_space_0.xy = _S29.xy * float2(normal_scale_1) ;

#line 4746
    float3 _S30 = normalize(tangent_space_0);

#line 4746
    tangent_space_0 = _S30;
    return normalize(float3(_S30.x)  * (&basis_5)->tangent_1 + float3(_S30.y)  * (&basis_5)->bitangent_0 + float3(_S30.z)  * (&basis_5)->normal_0);
}


#line 2665
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2676
    float3 _S31;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2677
        _S31 = - facet_1;

#line 2677
    }
    else
    {

#line 2677
        _S31 = facet_1;

#line 2677
    }

#line 2677
    return _S31;
}


#line 1029
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3964
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S32 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S33 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S34 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 3974
    uint _S36 = uint(pixel_0.x) / _S35;

#line 3974
    uint _S37 = min(_S36, _S32 - 1U);
    uint _S38 = uint(pixel_0.y) / _S35;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3985
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S34 - 1U))) * _S33 + min(_S38, _S33 - 1U)) * _S32 + _S37;
}


#line 2097
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2118
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 2118
    texture2d<float, access::sample> _S39 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S39).get_width(0)),(*((&height_1)) = (_S39).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2124
    float2 _S40 = float2(1.0f) ;
    float2 _S41 = extent_1 - _S40;

#line 2125
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S41);
    float2 high_1 = min(low_1 + _S40, _S41);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2143
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 2155
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S42 = tap_1->lo_0.x;

#line 2157
    int _S43 = tap_1->lo_0.y;

#line 2157
    int3 _S44 = int3(_S42, _S43, int(0));
    int _S45 = tap_1->hi_0.x;

#line 2158
    int3 _S46 = int3(_S45, _S43, int(0));
    float2 _S47 = float2(tap_1->weight_0.x) ;
    int _S48 = tap_1->hi_0.y;

#line 2160
    int3 _S49 = int3(_S42, _S48, int(0));
    int3 _S50 = int3(_S45, _S48, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)))), _S47), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), _S47), float2(tap_1->weight_0.y) );
}


#line 3915
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3931
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3943
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3950
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2484
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2484
    float4 _S51 = float4(light_0->tangent_0) ;

    float3 _S52 = _S51.xyz;

#line 2486
    float3 across_0 = _S52 * float3(_S51.w) ;

#line 2486
    float4 _S53 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S52, _S53.xyz) * float3(_S53.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S54 = centre_0 - across_0;

#line 2489
    (*corners_0)[int(0)] = _S54 - down_0;
    float3 _S55 = centre_0 + across_0;

#line 2490
    (*corners_0)[int(1)] = _S55 - down_0;
    (*corners_0)[int(2)] = _S55 + down_0;
    (*corners_0)[int(3)] = _S54 + down_0;
    return;
}


#line 2242
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2245
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2246
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2246
    }
    else
    {

#line 2246
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2246
    }

#line 2246
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2247
        tangent_5 = across_1 / float3(span_0) ;

#line 2247
    }
    else
    {

#line 2247
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2247
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 2223
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2313
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2313
    float3 _S56 = polygon_0->corner_0[int(0)];

#line 2313
    float3 _S57 = polygon_0->corner_0[int(1)];

#line 2313
    float3 _S58 = polygon_0->corner_0[int(2)];

#line 2313
    float3 _S59 = polygon_0->corner_0[int(3)];

#line 2319
    float3 _S60 = float3(0.0f, 0.0f, 0.0f);


    float _S61 = polygon_0->corner_0[int(0)].z;

#line 2322
    int count_1;

#line 2322
    if(_S61 > 0.0f)
    {

#line 2322
        count_1 = int(1);

#line 2322
    }
    else
    {

#line 2322
        count_1 = int(0);

#line 2322
    }
    float _S62 = _S57.z;

#line 2323
    int _S63;

#line 2323
    if(_S62 > 0.0f)
    {

#line 2323
        _S63 = int(2);

#line 2323
    }
    else
    {

#line 2323
        _S63 = int(0);

#line 2323
    }

#line 2323
    int config_0 = count_1 + _S63;
    float _S64 = _S58.z;

#line 2324
    if(_S64 > 0.0f)
    {

#line 2324
        count_1 = int(4);

#line 2324
    }
    else
    {

#line 2324
        count_1 = int(0);

#line 2324
    }

#line 2324
    int config_1 = config_0 + count_1;
    float _S65 = _S59.z;

#line 2325
    if(_S65 > 0.0f)
    {

#line 2325
        count_1 = int(8);

#line 2325
    }
    else
    {

#line 2325
        count_1 = int(0);

#line 2325
    }

#line 2325
    int config_2 = config_1 + count_1;

#line 2325
    float3 l0_0;

#line 2325
    float3 l1_0;

#line 2325
    float3 l2_0;

#line 2325
    float3 l3_0;

#line 2325
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2328
        float3 _S66 = float3(_S61) ;


        float3 _S67 = float3(- _S62)  * _S56 + _S66 * _S57;
        float3 _S68 = float3(- _S65)  * _S56 + _S66 * _S59;

#line 2332
        count_1 = int(3);

#line 2332
        l0_0 = _S56;

#line 2332
        l1_0 = _S67;

#line 2332
        l2_0 = _S68;

#line 2332
        l3_0 = _S59;

#line 2332
        l4_0 = _S60;

#line 2328
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2334
            float3 _S69 = float3(_S62) ;


            float3 _S70 = float3(- _S61)  * _S57 + _S69 * _S56;
            float3 _S71 = float3(- _S64)  * _S57 + _S69 * _S58;

#line 2338
            count_1 = int(3);

#line 2338
            l0_0 = _S70;

#line 2338
            l1_0 = _S57;

#line 2338
            l2_0 = _S71;

#line 2338
            l3_0 = _S59;

#line 2338
            l4_0 = _S60;

#line 2334
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S72 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                float3 _S73 = float3(- _S65)  * _S56 + float3(_S61)  * _S59;

#line 2344
                count_1 = int(4);

#line 2344
                l0_0 = _S56;

#line 2344
                l1_0 = _S57;

#line 2344
                l2_0 = _S72;

#line 2344
                l3_0 = _S73;

#line 2344
                l4_0 = _S60;

#line 2340
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2346
                    float3 _S74 = float3(_S64) ;


                    float3 _S75 = float3(- _S65)  * _S58 + _S74 * _S59;
                    float3 _S76 = float3(- _S62)  * _S58 + _S74 * _S57;

#line 2350
                    count_1 = int(3);

#line 2350
                    l0_0 = _S75;

#line 2350
                    l1_0 = _S76;

#line 2350
                    l2_0 = _S58;

#line 2350
                    l3_0 = _S59;

#line 2350
                    l4_0 = _S60;

#line 2346
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S77 = float3(- _S61)  * _S57 + float3(_S62)  * _S56;
                        float3 _S78 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;

#line 2356
                        count_1 = int(4);

#line 2356
                        l0_0 = _S77;

#line 2356
                        l1_0 = _S57;

#line 2356
                        l2_0 = _S58;

#line 2356
                        l3_0 = _S78;

#line 2356
                        l4_0 = _S60;

#line 2352
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2358
                            float3 _S79 = float3(- _S65) ;


                            float3 _S80 = _S79 * _S56 + float3(_S61)  * _S59;
                            float3 _S81 = _S79 * _S58 + float3(_S64)  * _S59;

#line 2362
                            count_1 = int(5);

#line 2362
                            l0_0 = _S56;

#line 2362
                            l1_0 = _S57;

#line 2362
                            l2_0 = _S58;

#line 2362
                            l3_0 = _S81;

#line 2362
                            l4_0 = _S80;

#line 2358
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2364
                                float3 _S82 = float3(_S65) ;


                                float3 _S83 = float3(- _S61)  * _S59 + _S82 * _S56;
                                float3 _S84 = float3(- _S64)  * _S59 + _S82 * _S58;

#line 2368
                                count_1 = int(3);

#line 2368
                                l0_0 = _S83;

#line 2368
                                l1_0 = _S84;

#line 2368
                                l2_0 = _S59;

#line 2368
                                l3_0 = _S59;

#line 2368
                                l4_0 = _S60;

#line 2364
                            }
                            else
                            {

#line 2371
                                if(config_2 == int(9))
                                {

                                    float3 _S85 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;
                                    float3 _S86 = float3(- _S64)  * _S59 + float3(_S65)  * _S58;

#line 2375
                                    count_1 = int(4);

#line 2375
                                    l0_0 = _S56;

#line 2375
                                    l1_0 = _S85;

#line 2375
                                    l2_0 = _S86;

#line 2375
                                    l3_0 = _S59;

#line 2375
                                    l4_0 = _S60;

#line 2371
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S87 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;
                                        float3 _S88 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;

#line 2382
                                        count_1 = int(5);

#line 2382
                                        l0_0 = _S56;

#line 2382
                                        l1_0 = _S57;

#line 2382
                                        l2_0 = _S88;

#line 2382
                                        l3_0 = _S87;

#line 2382
                                        l4_0 = _S59;

#line 2377
                                    }
                                    else
                                    {

#line 2384
                                        if(config_2 == int(12))
                                        {

                                            float3 _S89 = float3(- _S62)  * _S58 + float3(_S64)  * _S57;
                                            float3 _S90 = float3(- _S61)  * _S59 + float3(_S65)  * _S56;

#line 2388
                                            count_1 = int(4);

#line 2388
                                            l0_0 = _S90;

#line 2388
                                            l1_0 = _S89;

#line 2388
                                            l2_0 = _S58;

#line 2388
                                            l3_0 = _S59;

#line 2388
                                            l4_0 = _S60;

#line 2384
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S91 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                                                float3 _S92 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;

#line 2396
                                                count_1 = int(5);

#line 2396
                                                l0_0 = _S56;

#line 2396
                                                l1_0 = _S92;

#line 2396
                                                l2_0 = _S91;

#line 2396
                                                l3_0 = _S58;

#line 2396
                                                l4_0 = _S59;

#line 2390
                                            }
                                            else
                                            {

#line 2398
                                                if(config_2 == int(14))
                                                {

#line 2398
                                                    float3 _S93 = float3(- _S61) ;


                                                    float3 _S94 = _S93 * _S59 + float3(_S65)  * _S56;
                                                    float3 _S95 = _S93 * _S57 + float3(_S62)  * _S56;

#line 2402
                                                    count_1 = int(5);

#line 2402
                                                    l0_0 = _S95;

#line 2402
                                                    l1_0 = _S94;

#line 2398
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2404
                                                        count_1 = int(4);

#line 2404
                                                    }
                                                    else
                                                    {

#line 2404
                                                        count_1 = int(0);

#line 2404
                                                    }

#line 2404
                                                    l0_0 = _S56;

#line 2404
                                                    l1_0 = _S60;

#line 2398
                                                }

#line 2319
                                                float3 _S96 = l1_0;

#line 2319
                                                l1_0 = _S57;

#line 2319
                                                l2_0 = _S58;

#line 2319
                                                l3_0 = _S59;

#line 2319
                                                l4_0 = _S96;

#line 2390
                                            }

#line 2384
                                        }

#line 2377
                                    }

#line 2371
                                }

#line 2364
                            }

#line 2358
                        }

#line 2352
                    }

#line 2346
                }

#line 2340
            }

#line 2334
        }

#line 2328
    }

#line 2412
    if(count_1 <= int(3))
    {

#line 2412
        l3_0 = l0_0;

#line 2412
        l4_0 = l0_0;

#line 2412
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2417
            l4_0 = l0_0;

#line 2417
        }

#line 2412
    }

#line 2422
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2285
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2291
    float weight_1;

#line 2296
    if(cosine_0 > 0.0f)
    {

#line 2296
        weight_1 = fit_0;

#line 2296
    }
    else
    {

#line 2296
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2296
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2442
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2444
    int corner_1 = int(0);
    for(;;)
    {

#line 2445
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2445
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2445
        corner_1 = corner_1 + int(1);

#line 2445
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2450
    thread LtcPolygon_0 _S97 = polygon_1;

#line 2450
    LtcPolygon_0 _S98 = ltc_clip_0(&_S97);
    polygon_1 = _S98;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2454
    int at_2 = int(0);

    for(;;)
    {

#line 2456
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2456
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2456
        at_2 = at_2 + int(1);

#line 2456
    }

#line 2463
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2463
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2464
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2464
    }
    else
    {

#line 2464
        sum_1 = sum_0;

#line 2464
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2468
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2468
    }

#line 2475
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2171
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S99 = tap_2->lo_0.x;

#line 2173
    int _S100 = tap_2->lo_0.y;

#line 2173
    int3 _S101 = int3(_S99, _S100, int(0));
    int _S102 = tap_2->hi_0.x;

#line 2174
    int3 _S103 = int3(_S102, _S100, int(0));
    float4 _S104 = float4(tap_2->weight_0.x) ;
    int _S105 = tap_2->hi_0.y;

#line 2176
    int3 _S106 = int3(_S99, _S105, int(0));
    int3 _S107 = int3(_S102, _S105, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S101)).xy), uint(((_S101)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S103)).xy), uint(((_S103)).z))), _S104), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S107)).xy), uint(((_S107)).z))), _S104), float4(tap_2->weight_0.y) );
}


#line 2258
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2053
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2060
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2067
    float _S108 = 1.0f - alpha2_0;

#line 2072
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S108 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S108 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3087
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3087
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3147
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3119
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2716
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3074
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_13)
{

#line 3074
    uint _S109;

    if(uint(pixel_1.x) < (kernelContext_13->frame_0->shadow_filter_0.z))
    {

#line 3076
        _S109 = kernelContext_13->frame_0->shadow_filter_0.x;

#line 3076
    }
    else
    {

#line 3076
        _S109 = kernelContext_13->frame_0->shadow_filter_0.y;

#line 3076
    }

#line 3076
    return _S109;
}


#line 3099
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3099
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 349
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3169
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S110 = spoke_0.x;

#line 3174
    float _S111 = rotation_0.x;

#line 3174
    float _S112 = spoke_0.y;

#line 3174
    float _S113 = rotation_0.y;


    float _S114 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S110 * _S111 - _S112 * _S113, _S110 * _S113 + _S112 * _S111) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3177
    return _S114;
}


#line 3257
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_17)
{

#line 3257
    float4 _S115 = atlas_rect_1(tile_2, kernelContext_17);


    if(atlas_rect_is_empty_0(_S115))
    {
        return 1.0f;
    }

#line 3262
    float2 _S116 = atlas_step_1(_S115, kernelContext_17);

#line 3262
    int y_1 = int(-1);

#line 3262
    float visibility_0 = 0.0f;

#line 3267
    for(;;)
    {

#line 3267
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3267
            break;
        }

#line 3267
        int x_0 = int(-1);

        for(;;)
        {

#line 3269
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3269
                break;
            }

#line 3269
            float _S117 = tile_tap_0(_S115, _S116, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_17);

            float visibility_1 = visibility_0 + _S117;

#line 3269
            x_0 = x_0 + int(1);

#line 3269
            visibility_0 = visibility_1;

#line 3269
        }

#line 3267
        y_1 = y_1 + int(1);

#line 3267
    }

#line 3275
    return visibility_0 / 9.0f;
}


#line 3032
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3199
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_18)
{
    float2 _S118 = shadow_rotation_0(pixel_3);

#line 3201
    float4 _S119 = atlas_rect_1(tile_3, kernelContext_18);

    if(atlas_rect_is_empty_0(_S119))
    {
        return 1.0f;
    }

#line 3205
    float2 _S120 = atlas_step_1(_S119, kernelContext_18);

#line 3205
    uint spot_0 = 0U;

#line 3205
    float probe_0 = 0.0f;

#line 3210
    for(;;)
    {

#line 3210
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3210
            break;
        }

#line 3210
        float _S121 = tile_tap_0(_S119, _S120, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S118, reference_2, kernelContext_18);

        float probe_1 = probe_0 + _S121;

#line 3210
        spot_0 = spot_0 + 1U;

#line 3210
        probe_0 = probe_1;

#line 3210
    }

#line 3219
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3225
    uint index_2 = 0U;

#line 3225
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3229
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3229
            break;
        }

#line 3229
        float _S122 = tile_tap_0(_S119, _S120, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S118, reference_2, kernelContext_18);

        float visibility_3 = visibility_2 + _S122;

#line 3229
        index_2 = index_2 + 1U;

#line 3229
        visibility_2 = visibility_3;

#line 3229
    }

#line 3234
    return visibility_2 / 32.0f;
}


#line 3310
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_19)
{
    float2 texel_1 = kernelContext_19->frame_0->shadow_params_0.xy;

#line 3312
    float4 _S123 = atlas_rect_0(cascade_0, kernelContext_19);

#line 3312
    float2 _S124 = atlas_step_0(_S123, kernelContext_19);


    float2 _S125 = float2(0.5f, 0.5f) * _S124;


    float2 _S126 = float2(1.0f, 1.0f);

#line 3318
    float2 _S127 = _S126 / texel_1;

#line 3318
    uint index_3 = 0U;

#line 3318
    float sum_2 = 0.0f;

#line 3318
    float found_0 = 0.0f;



    for(;;)
    {

#line 3322
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3322
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S128 = spoke_1.x;

#line 3325
        float _S129 = rotation_1.x;

#line 3325
        float _S130 = spoke_1.y;

#line 3325
        float _S131 = rotation_1.y;

#line 3333
        int3 _S132 = int3(int2(min(atlas_uv_0(_S123, clamp(tile_uv_4 + float2(_S128 * _S129 - _S130 * _S131, _S128 * _S131 + _S130 * _S129) * _S124, _S125, float2(1.0f)  - _S125)) * _S127, _S127 - _S126)), int(0));

#line 3333
        float depth_1 = ((kernelContext_19->shadow_atlas_0).read(vec<uint,2>(((_S132)).xy), uint(((_S132)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3337
            sum_2 = sum_2 + depth_1;

#line 3337
            found_0 = found_1;

#line 3334
        }

#line 3322
        index_3 = index_3 + 1U;

#line 3322
    }

#line 3341
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3352
    float _S133 = 2.0f * kernelContext_19->frame_0->cascade_far_0[cascade_0];

#line 3352
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S133 + 40.0f);

#line 3352
    float _S134 = tile_texels_0(_S123, kernelContext_19);

    return clamp(separation_0 * 0.01999999955296516f / (_S133 / _S134), 2.0f, 8.0f);
}


#line 3406
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_20)
{

#line 3407
    float4 _S135 = atlas_rect_0(cascade_1, kernelContext_20);

#line 3441
    if(atlas_rect_is_empty_0(_S135))
    {


        return 1.0f;
    }
    float _S136 = 2.0f * kernelContext_20->frame_0->cascade_far_0[cascade_1];

#line 3447
    float _S137 = tile_texels_0(_S135, kernelContext_20);

#line 3447
    float texel_world_0 = _S136 / _S137;

#line 3454
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_20->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_20->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3458
    bool _S138;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3459
        _S138 = true;

#line 3459
    }
    else
    {

#line 3459
        _S138 = (ndc_0.z) <= 0.0f;

#line 3459
    }

#line 3459
    if(_S138)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3469
    uint _S139 = shadow_filter_mode_0(pixel_4, kernelContext_20);

#line 3486
    if(_S139 == 2U)
    {

#line 3486
        float _S140 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_20);

        return _S140;
    }
    if(_S139 == 1U)
    {

#line 3490
        float _S141 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_20);



        return _S141;
    }

    float _S142 = ndc_0.z;

#line 3497
    float _S143 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S142, shadow_rotation_0(pixel_4), kernelContext_20);

#line 3497
    float _S144 = tile_pcf_0(cascade_1, tile_uv_5, _S142, pixel_4, _S143, kernelContext_20);
    return _S144;
}


#line 3577
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_21)
{
    uint cascade_2;

#line 3579
    bool covered_0;

#line 3588
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3600
    float eye_distance_0 = length(world_position_5 - kernelContext_21->frame_0->camera_position_0.xyz);

#line 3600
    uint index_4 = 0U;

#line 3608
    for(;;)
    {

#line 3608
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3608
            covered_0 = false;

#line 3608
            cascade_2 = 1U;

#line 3608
            break;
        }
        if(eye_distance_0 < kernelContext_21->frame_0->cascade_far_0[index_4])
        {

#line 3610
            covered_0 = true;

#line 3610
            cascade_2 = index_4;



            break;
        }

#line 3608
        index_4 = index_4 + 1U;

#line 3608
    }

#line 3617
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 3617
    }

#line 3617
    float _S145 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3624
    uint _S146 = cascade_2 + 1U;

#line 3624
    if(_S146 >= 2U)
    {



        return _S145;
    }

#line 3637
    float band_0 = kernelContext_21->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_21->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S145;
    }

#line 3645
    float _S147 = cascade_visibility_0(_S146, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_21);

#line 3656
    return mix(_S145, _S147, blend_0);
}


#line 4815
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_22)
{

#line 4815
    texture2d<float, access::sample> _S148 = kernelContext_22->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S148).get_width(0)),(*((&height_2)) = (_S148).get_height(0));

    int3 _S149 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 4821
    return ((kernelContext_22->contact_shadow_0).read(vec<uint,2>(((_S149)).xy), uint(((_S149)).z)).x);
}


#line 3549
float3 cascade_tint_0(uint cascade_3, float blend_1)
{
    if(cascade_3 >= 2U)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }
    uint _S150 = cascade_3 + 1U;

#line 3555
    if(_S150 >= 2U)
    {


        return CASCADE_TINTS_0[cascade_3];
    }
    return mix(CASCADE_TINTS_0[cascade_3], CASCADE_TINTS_0[_S150], float3(blend_1) );
}


#line 3867
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S151 = axis_2.x;

#line 3870
    float _S152 = axis_2.y;

#line 3870
    bool _S153;

#line 3870
    if(_S151 >= _S152)
    {

#line 3870
        _S153 = _S151 >= (axis_2.z);

#line 3870
    }
    else
    {

#line 3870
        _S153 = false;

#line 3870
    }

#line 3870
    uint _S154;

#line 3870
    if(_S153)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3872
            _S154 = 0U;

#line 3872
        }
        else
        {

#line 3872
            _S154 = 1U;

#line 3872
        }

#line 3872
        return _S154;
    }
    if(_S152 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3876
            _S154 = 2U;

#line 3876
        }
        else
        {

#line 3876
            _S154 = 3U;

#line 3876
        }

#line 3876
        return _S154;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3878
        _S154 = 4U;

#line 3878
    }
    else
    {

#line 3878
        _S154 = 5U;

#line 3878
    }

#line 3878
    return _S154;
}


#line 336
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 3763
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_23)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 3766
    float4 _S155 = atlas_rect_0(atlas_0, kernelContext_23);

    if(atlas_rect_is_empty_0(_S155))
    {


        return 1.0f;
    }

#line 3772
    float _S156 = tile_texels_0(_S155, kernelContext_23);

    float texel_world_1 = map_world_0 / _S156;

#line 3784
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_23->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 3791
    float _S157 = clip_1.w;

#line 3791
    if(_S157 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S157) ;

#line 3795
    bool _S158;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3796
        _S158 = true;

#line 3796
    }
    else
    {

#line 3796
        _S158 = (ndc_1.z) <= 0.0f;

#line 3796
    }

#line 3796
    if(_S158)
    {

#line 3796
        _S158 = true;

#line 3796
    }
    else
    {

#line 3796
        _S158 = (ndc_1.z) > 1.0f;

#line 3796
    }

#line 3796
    if(_S158)
    {

#line 3803
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 3808
    uint _S159 = shadow_filter_mode_0(pixel_6, kernelContext_23);

#line 3817
    if(_S159 == 2U)
    {

#line 3817
        float _S160 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_23);

        return _S160;
    }

#line 3819
    float _S161 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_23);

    return _S161;
}


#line 3886
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_24)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3894
    float _S162 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_24);

#line 3900
    return _S162;
}


#line 3828
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_25)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3835
    float4 _S163 = float4(light_2->direction_0) ;

#line 3842
    float cos_outer_1 = _S163.w;

#line 3842
    float _S164 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S163.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_25);

#line 3849
    return _S164;
}


#line 2199
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 4802
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 4804
    float3 _S165;
    if((length(decoded_0)) < 0.5f)
    {

#line 4805
        _S165 = shading_normal_1;

#line 4805
    }
    else
    {

#line 4805
        _S165 = normalize(decoded_0);

#line 4805
    }

#line 4805
    return _S165;
}


#line 4440
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_26)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_26->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_26->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4344
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4344
    float reach_0 = 0.0f;

#line 4344
    uint axis_3 = 0U;


    for(;;)
    {

#line 4347
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4347
            break;
        }

#line 4347
        uint _S166 = axis_3;

#line 4347
        bool _S167;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4349
            _S167 = true;

#line 4349
        }
        else
        {

#line 4349
            _S167 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4349
        }

#line 4349
        if(_S167)
        {

#line 4350
            axis_3 = axis_3 + 1U;

#line 4347
            continue;
        }

#line 4347
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S166] - 1.0f));

#line 4347
        axis_3 = axis_3 + 1U;

#line 4347
    }

#line 4354
    return reach_0;
}


#line 4374
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4374
    uint level_0 = 0U;

    for(;;)
    {

#line 4376
        uint _S168 = level_0 + 1U;

#line 4376
        if(_S168 < levels_0)
        {
        }
        else
        {

#line 4376
            break;
        }
        float _S169 = float(level_0);

#line 4378
        float at_3 = reach_1 * exp2(- _S169);
        if(at_3 < 1.0f)
        {

#line 4380
            return float2(_S169, saturate((1.0f - at_3) / 0.25f));
        }

#line 4376
        level_0 = _S168;

#line 4376
    }

#line 4382
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4162
uint probe_row_0(uint level_1, uint3 cell_1, KernelContext_0 thread* kernelContext_27)
{


    return min(kernelContext_27->frame_0->probe_levels_0.y * level_1 + (cell_1.z * kernelContext_27->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_27->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_27->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4027
float sign_not_zero_0(float value_0)
{

#line 4027
    float _S170;

    if(value_0 >= 0.0f)
    {

#line 4029
        _S170 = 1.0f;

#line 4029
    }
    else
    {

#line 4029
        _S170 = -1.0f;

#line 4029
    }

#line 4029
    return _S170;
}


#line 4046
float2 oct_encode_0(float3 direction_1)
{
    float _S171 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S171) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4049
    float2 p_1;
    if(_S171 < 0.0f)
    {
        float _S172 = p_0.y;

#line 4052
        float _S173 = p_0.x;

#line 4052
        p_1 = float2((1.0f - abs(_S172)) * sign_not_zero_0(_S173), (1.0f - abs(_S173)) * sign_not_zero_0(_S172));

#line 4050
    }
    else
    {

#line 4050
        p_1 = p_0;

#line 4050
    }

#line 4055
    return p_1;
}


#line 4075
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_28)
{

#line 4075
    texture2d_array<float, access::sample> _S174 = kernelContext_28->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S174).get_width(0)),(*((&height_3)) = (_S174).get_height(0)),(*((&layers_0)) = (_S174).get_array_size());

#line 4080
    float2 _S175 = float2(0.5f) ;

#line 4080
    float2 _S176 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S175 + _S175) * float2(16.0f)  + _S176 - _S175;
    float2 _S177 = float2(float(width_3), float(height_3)) - _S176;

#line 4084
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S177);
    float2 high_2 = min(low_2 + _S176, _S177);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S178 = int(low_2.x);

#line 4089
    int _S179 = int(low_2.y);

#line 4089
    int4 _S180 = int4(_S178, _S179, layer_1, int(0));
    int _S181 = int(high_2.x);

#line 4090
    int4 _S182 = int4(_S181, _S179, layer_1, int(0));
    int _S183 = int(high_2.y);

#line 4091
    int4 _S184 = int4(_S178, _S183, layer_1, int(0));
    int4 _S185 = int4(_S181, _S183, layer_1, int(0));
    float2 _S186 = float2(weight_2.x) ;

#line 4093
    return mix(mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S180)).xy), uint(((_S180)).z), uint(((_S180)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S182)).xy), uint(((_S182)).z), uint(((_S182)).w))).xy, _S186), mix(((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S184)).xy), uint(((_S184)).z), uint(((_S184)).w))).xy, ((kernelContext_28->probe_visibility_0).read(vec<uint,2>(((_S185)).xy), uint(((_S185)).z), uint(((_S185)).w))).xy, _S186), float2(weight_2.y) );
}


#line 4121
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_7, KernelContext_0 thread* kernelContext_29)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_7 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4124
    float2 _S187 = probe_moments_0(index_6, - to_probe_0, kernelContext_29);

#line 4130
    float _S188 = _S187.x;

#line 4130
    float _S189 = max(_S187.y - _S188 * _S188, 0.0f);
    float behind_0 = to_surface_0 - _S188;
    float bound_0 = _S189 / (_S189 + behind_0 * behind_0);

#line 4132
    float _S190;
    if(to_surface_0 <= _S188)
    {

#line 4133
        _S190 = 1.0f;

#line 4133
    }
    else
    {

#line 4133
        _S190 = bound_0 * bound_0 * bound_0;

#line 4133
    }

#line 4133
    return _S190;
}


#line 4143
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_8, KernelContext_0 thread* kernelContext_30)
{

#line 4143
    float _S191 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_8, kernelContext_30);

    return max(_S191, 0.00009999999747379f);
}


#line 1081
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4176
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4203
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_2, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_9, KernelContext_0 thread* kernelContext_31)
{

#line 4204
    uint _S192 = probe_row_0(level_2, cell_2, kernelContext_31);


    GpuProbe_natural_0 stored_0 = kernelContext_31->probes_0[_S192];

#line 4207
    float _S193 = probe_weight_0(_S192, origin_1 + float3(cell_2) * spacing_0, world_position_12, normal_9, kernelContext_31);



    thread WeightedProbe_0 corner_2;

#line 4211
    float4 _S194 = float4(_S193) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S194;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S194;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S194;
    (&corner_2)->weight_3 = _S193;
    return corner_2;
}


#line 4187
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S195 = float4(t_1) ;

#line 4190
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S195);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S195);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S195);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4275
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_10, KernelContext_0 thread* kernelContext_32)
{

#line 4275
    float3 _S196 = float3(1.0f) ;

#line 4280
    float3 _S197 = float3(0.0f, 0.0f, 0.0f);

#line 4280
    float3 last_1 = max(float3(kernelContext_32->frame_0->probe_counts_0.xyz) - _S196, _S197);



    float3 origin_2 = kernelContext_32->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_32->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S197, last_1);
    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S198 = uint3(base_2);



    uint3 _S199 = uint3(min(base_2 + _S196, last_1));

#line 4300
    float _S200 = inv_0.x;

#line 4300
    float _S201;

#line 4300
    if(_S200 != 0.0f)
    {

#line 4300
        _S201 = 1.0f / _S200;

#line 4300
    }
    else
    {

#line 4300
        _S201 = 0.0f;

#line 4300
    }
    float _S202 = inv_0.y;

#line 4301
    float _S203;

#line 4301
    if(_S202 != 0.0f)
    {

#line 4301
        _S203 = 1.0f / _S202;

#line 4301
    }
    else
    {

#line 4301
        _S203 = 0.0f;

#line 4301
    }
    float _S204 = inv_0.z;

#line 4302
    float _S205;

#line 4302
    if(_S204 != 0.0f)
    {

#line 4302
        _S205 = 1.0f / _S204;

#line 4302
    }
    else
    {

#line 4302
        _S205 = 0.0f;

#line 4302
    }

#line 4300
    float3 spacing_1 = float3(_S201, _S203, _S205);

#line 4309
    uint _S206 = _S198.x;

#line 4309
    uint _S207 = _S198.y;

#line 4309
    uint _S208 = _S198.z;

#line 4309
    WeightedProbe_0 _S209 = probe_corner_0(level_3, uint3(_S206, _S207, _S208), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);
    uint _S210 = _S199.x;

#line 4310
    WeightedProbe_0 _S211 = probe_corner_0(level_3, uint3(_S210, _S207, _S208), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4310
    float _S212 = f_0.x;

#line 4310
    thread WeightedProbe_0 _S213 = _S209;

#line 4310
    thread WeightedProbe_0 _S214 = _S211;

#line 4310
    WeightedProbe_0 _S215 = lerp_probe_0(&_S213, &_S214, _S212);
    uint _S216 = _S199.y;

#line 4311
    WeightedProbe_0 _S217 = probe_corner_0(level_3, uint3(_S206, _S216, _S208), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4311
    WeightedProbe_0 _S218 = probe_corner_0(level_3, uint3(_S210, _S216, _S208), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4311
    thread WeightedProbe_0 _S219 = _S217;

#line 4311
    thread WeightedProbe_0 _S220 = _S218;

#line 4311
    WeightedProbe_0 _S221 = lerp_probe_0(&_S219, &_S220, _S212);

    uint _S222 = _S199.z;

#line 4313
    WeightedProbe_0 _S223 = probe_corner_0(level_3, uint3(_S206, _S207, _S222), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4313
    WeightedProbe_0 _S224 = probe_corner_0(level_3, uint3(_S210, _S207, _S222), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4313
    thread WeightedProbe_0 _S225 = _S223;

#line 4313
    thread WeightedProbe_0 _S226 = _S224;

#line 4313
    WeightedProbe_0 _S227 = lerp_probe_0(&_S225, &_S226, _S212);

#line 4313
    WeightedProbe_0 _S228 = probe_corner_0(level_3, uint3(_S206, _S216, _S222), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4313
    WeightedProbe_0 _S229 = probe_corner_0(level_3, uint3(_S210, _S216, _S222), origin_2, spacing_1, world_position_13, normal_10, kernelContext_32);

#line 4313
    thread WeightedProbe_0 _S230 = _S228;

#line 4313
    thread WeightedProbe_0 _S231 = _S229;

#line 4313
    WeightedProbe_0 _S232 = lerp_probe_0(&_S230, &_S231, _S212);



    float _S233 = f_0.y;

#line 4317
    thread WeightedProbe_0 _S234 = _S215;

#line 4317
    thread WeightedProbe_0 _S235 = _S221;

#line 4317
    WeightedProbe_0 _S236 = lerp_probe_0(&_S234, &_S235, _S233);

#line 4317
    thread WeightedProbe_0 _S237 = _S227;

#line 4317
    thread WeightedProbe_0 _S238 = _S232;

#line 4317
    WeightedProbe_0 _S239 = lerp_probe_0(&_S237, &_S238, _S233);

    float _S240 = f_0.z;

#line 4319
    thread WeightedProbe_0 _S241 = _S236;

#line 4319
    thread WeightedProbe_0 _S242 = _S239;

#line 4319
    WeightedProbe_0 _S243 = lerp_probe_0(&_S241, &_S242, _S240);

    float4 basis_7 = float4(normal_10, 1.0f);
    return max(float3(dot(_S243.sh_0.sh_r_0, basis_7), dot(_S243.sh_0.sh_g_0, basis_7), dot(_S243.sh_0.sh_b_0, basis_7)) / float3(_S243.weight_3) , _S197);
}


#line 4409
float3 probe_irradiance_0(float3 world_position_14, float3 normal_11, KernelContext_0 thread* kernelContext_33)
{

#line 4417
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_33->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_33->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_33->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_33->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 4419
    float3 _S244 = probe_level_irradiance_0(level_4, world_position_14, normal_11, kernelContext_33);


    if(share_0 >= 1.0f)
    {

#line 4423
        return _S244;
    }

#line 4423
    float3 _S245 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_11, kernelContext_33);

    return _S245 * float3((1.0f - share_0))  + _S244 * float3(share_0) ;
}


#line 4871
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 4871
    float3 _S246 = float3(visibility_4) ;

#line 4877
    return min(float3(1.0f) , max(_S246, ((_S246 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S246 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S246));
}


#line 1054
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2550
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S247 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2558
    float kernel_0 = 0.0001984127011383f;

#line 2558
    int term_0 = int(6);

    for(;;)
    {

#line 2560
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2560
            break;
        }
        float _S248 = kernel_0 * _S247 + FOG_KERNEL_0[term_0];

#line 2560
        int term_1 = term_0 - int(1);

#line 2560
        kernel_0 = _S248;

#line 2560
        term_0 = term_1;

#line 2560
    }

#line 2567
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2577
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S249 = - d_0;

#line 2581
        float series_0 = 0.00833333376795053f;

#line 2581
        int term_2 = int(3);

        for(;;)
        {

#line 2583
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2583
                break;
            }
            float _S250 = series_0 * _S249 + FOG_RATIO_KERNEL_0[term_2];

#line 2583
            int term_3 = term_2 - int(1);

#line 2583
            series_0 = _S250;

#line 2583
            term_2 = term_3;

#line 2583
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2611
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2622
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2630
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4466
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4466
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


#line 4913
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S251 [[stage_in]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 4913
    thread KernelContext_0 kernelContext_34;

#line 4913
    (&kernelContext_34)->draw_0 = draw_3;

#line 4913
    (&kernelContext_34)->visible_instances_0 = visible_instances_3;

#line 4913
    (&kernelContext_34)->instances_0 = instances_3;

#line 4913
    (&kernelContext_34)->meshes_0 = meshes_3;

#line 4913
    (&kernelContext_34)->frame_0 = frame_5;

#line 4913
    (&kernelContext_34)->vertices_0 = vertices_3;

#line 4913
    (&kernelContext_34)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4913
    (&kernelContext_34)->materials_0 = materials_3;

#line 4913
    (&kernelContext_34)->normal_textures_0 = normal_textures_3;

#line 4913
    (&kernelContext_34)->base_color_sampler_0 = base_color_sampler_3;

#line 4913
    (&kernelContext_34)->base_color_textures_0 = base_color_textures_3;

#line 4913
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_3;

#line 4913
    (&kernelContext_34)->specular_dfg_0 = specular_dfg_3;

#line 4913
    (&kernelContext_34)->lights_0 = lights_3;

#line 4913
    (&kernelContext_34)->ltc_matrix_0 = ltc_matrix_3;

#line 4913
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_3;

#line 4913
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_3;

#line 4913
    (&kernelContext_34)->contact_shadow_0 = contact_shadow_3;

#line 4913
    (&kernelContext_34)->probes_0 = probes_3;

#line 4913
    (&kernelContext_34)->probe_visibility_0 = probe_visibility_3;

#line 4925
    float3 vertex_normal_0 = normalize(_S251.world_normal_1);

#line 4930
    float2 motion_1 = motion_vector_0(_S251.clip_position_1, _S251.previous_clip_position_1);

#line 4946
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 4948
        float4 _S252 = occlusion_at_0(position_5.xy, &kernelContext_34);



        (&bent_0)->lit_0 = float4(_S252.yzw, 1.0f);


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

#line 5002
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5002
        float4 _S253 = occlusion_at_0(position_5.xy, &kernelContext_34);


        float value_1 = _S253.x;

#line 5004
        thread FragmentOutput_0 occlusion_1;

#line 5013
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S251.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 5030
    thread GpuMaterial_natural_0 _S254 = (&kernelContext_34)->materials_0[_S251.material_5];

#line 5030
    float2 uv_3;

#line 5055
    if(((&_S254)->tiling_0) == 1U)
    {

#line 5055
        uv_3 = physical_tile_uv_0(_S251.world_position_15, vertex_normal_0, (&_S254)->tile_metres_0);

#line 5055
    }
    else
    {

#line 5055
        uv_3 = _S251.uv_2;

#line 5055
    }

#line 5055
    uint _S255 = normal_layer_0(&_S254);

#line 5055
    thread VertexOutput_0 _S256;

#line 5055
    (&_S256)->position_3 = position_5;

#line 5055
    (&_S256)->world_position_1 = _S251.world_position_15;

#line 5055
    (&_S256)->world_normal_0 = _S251.world_normal_1;

#line 5055
    (&_S256)->color_2 = _S251.color_3;

#line 5055
    (&_S256)->material_2 = _S251.material_5;

#line 5055
    (&_S256)->uv_0 = _S251.uv_2;

#line 5055
    (&_S256)->clip_position_0 = _S251.clip_position_1;

#line 5055
    (&_S256)->previous_clip_position_0 = _S251.previous_clip_position_1;

#line 5055
    (&_S256)->world_tangent_0 = _S251.world_tangent_1;

#line 5055
    (&_S256)->frame_3 = _S251.frame_4;

#line 5055
    float3 _S257 = shading_normal_of_0(_S255, (&_S254)->normal_scale_0, &_S256, vertex_normal_0, uv_3, &kernelContext_34);

#line 5062
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5064
        float3 _S258 = float3(0.5f) ;

#line 5076
        (&normals_0)->lit_0 = float4(_S257 * _S258 + _S258, 1.0f);

#line 5082
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_34)->frame_0->camera_position_0.xyz - _S251.world_position_15);



    float3 _S259 = geometric_normal_of_0(_S251.world_position_15, vertex_normal_0);

#line 5091
    uint _S260 = base_color_layer_0(&_S254);

#line 5106
    float3 _S261 = float3(uv_3, float(_S260));
    float4 albedo_1 = _S251.color_3 * float4((&_S254)->base_color_0)  * (((&kernelContext_34)->base_color_textures_0).sample(((&kernelContext_34)->base_color_sampler_0), ((_S261)).xy, uint(((_S261)).z)));

#line 5113
    float metallic_1 = saturate((&_S254)->metallic_0);
    float roughness_2 = clamp((&_S254)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S262 = alpha_0 * alpha_0;

#line 5122
    float3 _S263 = albedo_1.xyz;

#line 5122
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S263, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S263 * float3((1.0f - metallic_1)) ;

#line 5129
    float _S264 = max(dot(_S257, to_eye_1), 0.00009999999747379f);

#line 5139
    float2 _S265 = position_5.xy;

#line 5139
    uint _S266 = froxel_of_0(_S265, (((float4(_S251.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_34);

#line 5139
    uint base_3 = _S266 * 17U;

#line 5144
    uint _S267 = min((&kernelContext_34)->cluster_lights_0[base_3], 16U);

#line 5144
    TableTap_0 _S268 = table_tap_0(_S264, roughness_2, &kernelContext_34);

#line 5144
    thread TableTap_0 _S269 = _S268;

#line 5144
    float2 _S270 = dfg_at_0(&_S269, &kernelContext_34);

#line 5153
    float _S271 = _S270.x;

#line 5153
    float _S272 = _S270.y;

#line 5153
    float3 _S273 = f0_2 * float3(_S271)  + float3(_S272) ;

#line 5159
    float3 _S274 = float3(0.0f, 0.0f, 0.0f);

#line 5159
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5159
    uint slot_0 = 0U;

#line 5159
    float3 direct_0 = _S274;

#line 5159
    float3 gloss_0 = _S274;

#line 5169
    for(;;)
    {

#line 5169
        if(slot_0 < _S267)
        {
        }
        else
        {

#line 5169
            break;
        }

#line 5169
        thread GpuLight_natural_0 _S275 = (&kernelContext_34)->lights_0[(&kernelContext_34)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 5169
        uint _S276 = (&_S275)->kind_0;

#line 5178
        bool _S277 = ((&_S275)->kind_0) == 0U;

#line 5178
        float3 to_light_7;

#line 5178
        float reach_2;

#line 5178
        if(_S277)
        {

#line 5178
            to_light_7 = normalize((float4((&_S275)->direction_0) ).xyz);

#line 5178
            reach_2 = 1.0f;

#line 5178
        }
        else
        {


            if(_S276 == 3U)
            {

#line 5183
                float4 _S278 = float4((&_S275)->position_0) ;

#line 5191
                float3 offset_0 = _S278.xyz - _S251.world_position_15;
                float distance_3 = length(offset_0);

                float _S279 = range_window_0(distance_3, _S278.w);

#line 5194
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5194
                reach_2 = _S279;

#line 5183
            }
            else
            {

#line 5183
                float4 _S280 = float4((&_S275)->position_0) ;

#line 5198
                float3 offset_1 = _S280.xyz - _S251.world_position_15;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S280.w);
                if(_S276 == 2U)
                {

#line 5202
                    float4 _S281 = float4((&_S275)->direction_0) ;

#line 5202
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S281.xyz, _S281.w, (&_S275)->cos_inner_0);

#line 5202
                }
                else
                {

#line 5202
                    reach_2 = reach_3;

#line 5202
                }

#line 5202
                to_light_7 = to_light_8;

#line 5183
            }

#line 5178
        }

#line 5211
        float n_dot_l_5 = dot(_S257, to_light_7);

#line 5211
        float3 specular_0;

#line 5211
        float diffuse_0;


        if(_S276 == 3U)
        {

#line 5224
            thread array<float3, int(4)> corners_2;

#line 5224
            rect_corners_0(&_S275, _S251.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S257, to_eye_1, _S264);

#line 5226
            thread array<float3, int(4)> _S282 = corners_2;

#line 5226
            float _S283 = ltc_irradiance_0(to_local_0, &_S282);

#line 5226
            thread TableTap_0 _S284 = _S268;

#line 5226
            float4 _S285 = ltc_at_0(&_S284, &kernelContext_34);

            matrix<float,int(3),int(3)>  _S286 = (((to_local_0) * (ltc_transform_0(_S285))));

#line 5228
            thread array<float3, int(4)> _S287 = corners_2;

#line 5228
            float _S288 = ltc_irradiance_0(_S286, &_S287);
            float3 _S289 = float3(_S288)  * _S273;

#line 5229
            diffuse_0 = _S283;

#line 5229
            specular_0 = _S289;

#line 5214
        }
        else
        {

#line 5234
            float _S290 = max(n_dot_l_5, 0.0f);

#line 5241
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5249
            float3 specular_1 = ggx_lobe_0(_S262, f0_2, _S290, _S264, max(dot(_S257, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S290) ;

#line 5249
            diffuse_0 = _S290;

#line 5249
            specular_0 = specular_1;

#line 5214
        }

#line 5214
        float3 specular_2;

#line 5257
        if((((&_S275)->flags_3) & 1U) != 0U)
        {

#line 5257
            specular_2 = _S274;

#line 5257
        }
        else
        {

#line 5257
            specular_2 = specular_0;

#line 5257
        }

#line 5257
        float reach_4;

#line 5275
        if(_S277)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 5278
            float _S291 = sun_visibility_0(_S251.world_position_15, to_light_7, n_dot_l_5, _S259, _S265, &sun_cascade_0, &sun_fade_0, &kernelContext_34);

#line 5278
            float _S292 = contact_at_0(_S265, &kernelContext_34);

#line 5287
            float _S293 = _S291 * _S292;

#line 5287
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 5287
            reach_4 = _S293;

#line 5275
        }
        else
        {

#line 5292
            if(_S276 == 1U)
            {

#line 5292
                uint _S294 = (&_S275)->shadow_tile_0;

#line 5304
                if(((&_S275)->shadow_tile_0) <= 8U)
                {

#line 5304
                    float _S295 = point_visibility_0(&_S275, _S294, _S251.world_position_15, to_light_7, n_dot_l_5, _S259, _S265, &kernelContext_34);

#line 5304
                    reach_4 = reach_2 * _S295;

#line 5304
                }
                else
                {

#line 5304
                    reach_4 = reach_2;

#line 5304
                }

#line 5292
            }
            else
            {

#line 5292
                uint _S296 = (&_S275)->shadow_tile_0;

#line 5310
                if(((&_S275)->shadow_tile_0) < 14U)
                {

#line 5310
                    float _S297 = spot_visibility_0(&_S275, _S296, _S251.world_position_15, to_light_7, n_dot_l_5, _S259, _S265, &kernelContext_34);

#line 5310
                    reach_4 = reach_2 * _S297;

#line 5310
                }
                else
                {

#line 5310
                    reach_4 = reach_2;

#line 5310
                }

#line 5292
            }

#line 5275
        }

#line 5318
        float3 _S298 = (float4((&_S275)->color_0) ).xyz;

#line 5318
        float3 direct_1 = direct_0 + _S298 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S298 * (specular_2 * float3(reach_4) );

#line 5169
        slot_0 = slot_0 + 1U;

#line 5169
        direct_0 = direct_1;

#line 5169
        gloss_0 = gloss_1;

#line 5169
    }

#line 5333
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S271 + _S272);

#line 5333
    float4 _S299 = occlusion_at_0(_S265, &kernelContext_34);

#line 5352
    float occluded_0 = _S299.x;

#line 5361
    float3 bent_normal_0 = bent_normal_at_0(_S299, _S257);

#line 5384
    float3 _S300 = frame_5->ambient_0.xyz;

#line 5384
    float3 _S301 = sky_irradiance_0(bent_normal_0, &kernelContext_34);

#line 5384
    float3 _S302 = _S300 + _S301;

#line 5384
    float3 _S303 = probe_irradiance_0(_S251.world_position_15, bent_normal_0, &kernelContext_34);

#line 5420
    float3 lit_1 = diffuse_albedo_0 * ((_S302 + _S303) * multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) + direct_0) + gloss_2;

#line 5420
    float3 _S304 = emissive_of_0(&_S254);

#line 5456
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_34)->frame_0->fog_params_0.x, (&kernelContext_34)->frame_0->fog_params_0.y, (&kernelContext_34)->frame_0->camera_position_0.y - (&kernelContext_34)->frame_0->fog_params_0.z, _S251.world_position_15.y - (&kernelContext_34)->frame_0->fog_params_0.z, length((&kernelContext_34)->frame_0->camera_position_0.xyz - _S251.world_position_15)));
    float3 lit_2 = (lit_1 + _S304) * float3(fog_survives_0)  + (&kernelContext_34)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;



    float _S305 = albedo_1.w;

#line 5463
    (&output_2)->lit_0 = float4(lit_2, _S305);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 5476
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, _S305);

#line 5485
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 5476
    }

#line 5487
    return output_2;
}


#line 5518
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_12 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 5518
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


#line 5561
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_1 _S306 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 5561
    thread KernelContext_0 kernelContext_35;

#line 5561
    (&kernelContext_35)->draw_0 = draw_4;

#line 5561
    (&kernelContext_35)->visible_instances_0 = visible_instances_4;

#line 5561
    (&kernelContext_35)->instances_0 = instances_4;

#line 5561
    (&kernelContext_35)->meshes_0 = meshes_4;

#line 5561
    (&kernelContext_35)->frame_0 = frame_7;

#line 5561
    (&kernelContext_35)->vertices_0 = vertices_4;

#line 5561
    (&kernelContext_35)->ambient_occlusion_0 = ambient_occlusion_4;

#line 5561
    (&kernelContext_35)->materials_0 = materials_4;

#line 5561
    (&kernelContext_35)->normal_textures_0 = normal_textures_4;

#line 5561
    (&kernelContext_35)->base_color_sampler_0 = base_color_sampler_4;

#line 5561
    (&kernelContext_35)->base_color_textures_0 = base_color_textures_4;

#line 5561
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_4;

#line 5561
    (&kernelContext_35)->specular_dfg_0 = specular_dfg_4;

#line 5561
    (&kernelContext_35)->lights_0 = lights_4;

#line 5561
    (&kernelContext_35)->ltc_matrix_0 = ltc_matrix_4;

#line 5561
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_4;

#line 5561
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_4;

#line 5561
    (&kernelContext_35)->contact_shadow_0 = contact_shadow_4;

#line 5561
    (&kernelContext_35)->probes_0 = probes_4;

#line 5561
    (&kernelContext_35)->probe_visibility_0 = probe_visibility_4;

#line 5566
    float3 vertex_normal_1 = normalize(_S306.world_normal_2);

#line 5566
    thread GpuMaterial_natural_0 _S307 = materials_4[_S306.material_6];

#line 5566
    float2 uv_5;

#line 5573
    if(((&_S307)->tiling_0) == 1U)
    {

#line 5573
        uv_5 = physical_tile_uv_0(_S306.world_position_16, vertex_normal_1, (&_S307)->tile_metres_0);

#line 5573
    }
    else
    {

#line 5573
        uv_5 = _S306.uv_4;

#line 5573
    }

#line 5573
    uint _S308 = base_color_layer_0(&_S307);

#line 5578
    float3 _S309 = float3(uv_5, float(_S308));


    thread RsmOutput_0 written_0;



    (&written_0)->albedo_2 = float4((_S306.color_4 * float4((&_S307)->base_color_0)  * (((&kernelContext_35)->base_color_textures_0).sample(((&kernelContext_35)->base_color_sampler_0), ((_S309)).xy, uint(((_S309)).z)))).xyz * float3((1.0f - saturate((&_S307)->metallic_0))) , 1.0f);

#line 5585
    float3 _S310 = float3(0.5f) ;
    (&written_0)->normal_12 = float4(vertex_normal_1 * _S310 + _S310, 1.0f);
    (&written_0)->world_0 = float4(_S306.world_position_16, 1.0f);
    return written_0;
}


#line 5588
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


#line 5588
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 5588
    thread KernelContext_0 kernelContext_36;

#line 5588
    (&kernelContext_36)->draw_0 = draw_5;

#line 5588
    (&kernelContext_36)->visible_instances_0 = visible_instances_5;

#line 5588
    (&kernelContext_36)->instances_0 = instances_5;

#line 5588
    (&kernelContext_36)->meshes_0 = meshes_5;

#line 5588
    (&kernelContext_36)->frame_0 = frame_9;

#line 5588
    (&kernelContext_36)->vertices_0 = vertices_5;

#line 5588
    (&kernelContext_36)->ambient_occlusion_0 = ambient_occlusion_5;

#line 5588
    (&kernelContext_36)->materials_0 = materials_5;

#line 5588
    (&kernelContext_36)->normal_textures_0 = normal_textures_5;

#line 5588
    (&kernelContext_36)->base_color_sampler_0 = base_color_sampler_5;

#line 5588
    (&kernelContext_36)->base_color_textures_0 = base_color_textures_5;

#line 5588
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_5;

#line 5588
    (&kernelContext_36)->specular_dfg_0 = specular_dfg_5;

#line 5588
    (&kernelContext_36)->lights_0 = lights_5;

#line 5588
    (&kernelContext_36)->ltc_matrix_0 = ltc_matrix_5;

#line 5588
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_5;

#line 5588
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_5;

#line 5588
    (&kernelContext_36)->contact_shadow_0 = contact_shadow_5;

#line 5588
    (&kernelContext_36)->probes_0 = probes_5;

#line 5588
    (&kernelContext_36)->probe_visibility_0 = probe_visibility_5;

#line 5588
    GpuInstance_natural_0 device* _S311 = instances_5+visible_instances_5[draw_5->base_0 + instance_id_1];

#line 1840
    GpuMesh_0 mesh_3 = meshes_5[draw_5->mesh_0];

#line 1848
    bool _S312 = ((_S311->flags_0) & 2U) != 0U;

#line 1848
    uint base_vertex_3;
    if(_S312)
    {

#line 1849
        base_vertex_3 = _S311->base_vertex_0;

#line 1849
    }
    else
    {

#line 1849
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1849
    }

#line 1849
    MeshVertex_0 _S313 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_36);

#line 1849
    uint previous_base_0;

#line 1862
    if(_S312)
    {

#line 1862
        previous_base_0 = _S311->previous_base_vertex_0;

#line 1862
    }
    else
    {

#line 1862
        previous_base_0 = base_vertex_3;

#line 1862
    }

#line 1862
    float3 _S314 = load_position_0(index_8 + previous_base_0, &kernelContext_36);

#line 1862
    matrix<float,int(4),int(4)>  _S315 = matrix<float,int(4),int(4)> (_S311->transform_0.data_0[int(0)][int(0)], _S311->transform_0.data_0[int(1)][int(0)], _S311->transform_0.data_0[int(2)][int(0)], _S311->transform_0.data_0[int(3)][int(0)], _S311->transform_0.data_0[int(0)][int(1)], _S311->transform_0.data_0[int(1)][int(1)], _S311->transform_0.data_0[int(2)][int(1)], _S311->transform_0.data_0[int(3)][int(1)], _S311->transform_0.data_0[int(0)][int(2)], _S311->transform_0.data_0[int(1)][int(2)], _S311->transform_0.data_0[int(2)][int(2)], _S311->transform_0.data_0[int(3)][int(2)], _S311->transform_0.data_0[int(0)][int(3)], _S311->transform_0.data_0[int(1)][int(3)], _S311->transform_0.data_0[int(2)][int(3)], _S311->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S313.position_1, 1.0f)) * (_S315)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_36)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 1876
    matrix<float,int(3),int(3)>  _S316 = matrix<float,int(3),int(3)> (_S315[int(0)].xyz, _S315[int(1)].xyz, _S315[int(2)].xyz);

#line 1876
    (&output_3)->world_normal_0 = (((_S313.basis_1.normal_0) * (normal_basis_0(_S316))));

#line 1882
    (&output_3)->world_tangent_0 = (((_S313.basis_1.tangent_1) * (_S316)));

#line 1882
    thread TangentFrame_0 _S317 = _S313.basis_1;

#line 1882
    uint _S318 = frame_word_0(mesh_3.flags_1, &_S317);
    (&output_3)->frame_3 = _S318;

#line 1883
    float4 _S319;

#line 1890
    if(((&kernelContext_36)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1890
        _S319 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1890
    }
    else
    {

#line 1890
        _S319 = _S313.color_1;

#line 1890
    }

#line 1889
    (&output_3)->color_2 = _S319;

#line 1896
    (&output_3)->material_2 = _S311->material_0;
    (&output_3)->uv_0 = _S313.uv0_0;

#line 1903
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S314, 1.0f)) * (matrix<float,int(4),int(4)> (_S311->previous_transform_0.data_0[int(0)][int(0)], _S311->previous_transform_0.data_0[int(1)][int(0)], _S311->previous_transform_0.data_0[int(2)][int(0)], _S311->previous_transform_0.data_0[int(3)][int(0)], _S311->previous_transform_0.data_0[int(0)][int(1)], _S311->previous_transform_0.data_0[int(1)][int(1)], _S311->previous_transform_0.data_0[int(2)][int(1)], _S311->previous_transform_0.data_0[int(3)][int(1)], _S311->previous_transform_0.data_0[int(0)][int(2)], _S311->previous_transform_0.data_0[int(1)][int(2)], _S311->previous_transform_0.data_0[int(2)][int(2)], _S311->previous_transform_0.data_0[int(3)][int(2)], _S311->previous_transform_0.data_0[int(0)][int(3)], _S311->previous_transform_0.data_0[int(1)][int(3)], _S311->previous_transform_0.data_0[int(2)][int(3)], _S311->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_36)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S320 = output_3;

#line 1907
    thread vertexMain_Result_0 _S321;

#line 1907
    (&_S321)->position_7 = _S320.position_3;

#line 1907
    (&_S321)->world_position_17 = _S320.world_position_1;

#line 1907
    (&_S321)->world_normal_3 = _S320.world_normal_0;

#line 1907
    (&_S321)->color_5 = _S320.color_2;

#line 1907
    (&_S321)->material_7 = _S320.material_2;

#line 1907
    (&_S321)->uv_6 = _S320.uv_0;

#line 1907
    (&_S321)->clip_position_3 = _S320.clip_position_0;

#line 1907
    (&_S321)->previous_clip_position_3 = _S320.previous_clip_position_0;

#line 1907
    (&_S321)->world_tangent_3 = _S320.world_tangent_0;

#line 1907
    (&_S321)->frame_8 = _S320.frame_3;

#line 1907
    return _S321;
}

