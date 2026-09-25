#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2949 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f } };

#line 2944
constant array<float, int(8)> FOG_KERNEL_0 = { { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f } };

#line 3946
constant array<float3, int(2)> CASCADE_TINTS_0 = { { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) } };

#line 3429
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) } };

#line 3216
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 3276
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

#line 3291
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) } };

#line 3319
constant array<uint, int(16)> SHADOW_DITHER_0 = { { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U } };

#line 1364
struct DrawConstants_0
{
    uint start_at_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1432
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1432
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


#line 908
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


#line 1390
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1390
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3335 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 397 "shaders/mesh.slang"
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


#line 1460
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


#line 1612
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


#line 1692
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 1884
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
    texture2d_array<float, access::sample> mro_textures_0;
    texture2d_array<float, access::sample> emissive_textures_0;
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


#line 1409
float3 load_position_0(uint at_0, KernelContext_0 thread* kernelContext_0)
{
    uint word_0 = at_0 * 3U;
    return float3((as_type<float>((kernelContext_0->vertices_0[word_0]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 1U]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 2U]))));
}


#line 230
float dequantise_snorm_0(int lane_0)
{
    return max(float(lane_0) / 32767.0f, -1.0f);
}


float4 unpack_snorm16x4_0(uint low_0, uint high_0)
{
    return float4(dequantise_snorm_0((as_type<int>((low_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((low_0))) >> 16U), dequantise_snorm_0((as_type<int>((high_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((high_0))) >> 16U));
}


#line 262
float3 rotate_by_0(float4 q_0, float3 v_0)
{
    float3 _S1 = q_0.xyz;

#line 264
    float3 t_0 = float3(2.0f)  * cross(_S1, v_0);
    return v_0 + float3(q_0.w)  * t_0 + cross(_S1, t_0);
}


#line 220
struct TangentFrame_0
{
    float3 tangent_1;
    float3 bitangent_0;
    float3 normal_0;
};


#line 276
TangentFrame_0 decode_qtangent_0(float4 lanes_0)
{
    float4 q_1 = normalize(lanes_0);
    thread TangentFrame_0 basis_0;
    float3 _S2 = rotate_by_0(q_1, float3(1.0f, 0.0f, 0.0f));

#line 280
    (&basis_0)->tangent_1 = _S2;
    float3 _S3 = rotate_by_0(q_1, float3(0.0f, 0.0f, 1.0f));

#line 281
    (&basis_0)->normal_0 = _S3;
    float3 _S4 = cross(_S3, _S2);

#line 282
    float _S5;

#line 282
    if((lanes_0.w) < 0.0f)
    {

#line 282
        _S5 = -1.0f;

#line 282
    }
    else
    {

#line 282
        _S5 = 1.0f;

#line 282
    }

#line 282
    (&basis_0)->bitangent_0 = _S4 * float3(_S5) ;
    return basis_0;
}


#line 245
float2 unpack_unorm16x2_0(uint word_1)
{
    return float2(float(word_1 & 65535U), float(word_1 >> 16U)) / float2(65535.0f) ;
}


float4 unpack_rgba8_0(uint word_2)
{
    return float4(float(word_2 & 255U), float((word_2 >> 8U) & 255U), float((word_2 >> 16U) & 255U), float(word_2 >> 24U)) / float4(255.0f) ;
}


#line 291
struct MeshVertex_0
{
    float3 position_1;
    TangentFrame_0 basis_1;
    float2 uv0_0;
    float4 color_1;
};


#line 1420
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1423
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 2046
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 2179
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 2179
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 2181
        word_4 = 1U;

#line 2181
    }
    else
    {

#line 2181
        word_4 = 0U;

#line 2181
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 2185
        word_4 = word_4 | 2U;

#line 2185
    }

#line 2184
    return word_4;
}


#line 2184
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 2301
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], uint base_instance_0 [[base_instance]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_1 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_1 [[texture(9)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 2301
    thread KernelContext_0 kernelContext_2;

#line 2301
    (&kernelContext_2)->draw_0 = draw_1;

#line 2301
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 2301
    (&kernelContext_2)->instances_0 = instances_1;

#line 2301
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 2301
    (&kernelContext_2)->frame_0 = frame_1;

#line 2301
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 2301
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2301
    (&kernelContext_2)->materials_0 = materials_1;

#line 2301
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 2301
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 2301
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 2301
    (&kernelContext_2)->mro_textures_0 = mro_textures_1;

#line 2301
    (&kernelContext_2)->emissive_textures_0 = emissive_textures_1;

#line 2301
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 2301
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 2301
    (&kernelContext_2)->lights_0 = lights_1;

#line 2301
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 2301
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 2301
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 2301
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 2301
    (&kernelContext_2)->probes_0 = probes_1;

#line 2301
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 2301
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[visible_instances_1[draw_1->start_at_0] + (instance_id_0 - base_instance_0)];



    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 2305
    uint base_vertex_2;

#line 2311
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 2311
        base_vertex_2 = _S7->base_vertex_0;

#line 2311
    }
    else
    {

#line 2311
        base_vertex_2 = mesh_2.base_vertex_1;

#line 2311
    }

#line 2311
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 2311
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 2311
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 2314
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 2335
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_2 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_2 [[texture(9)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 2335
    thread KernelContext_0 kernelContext_3;

#line 2335
    (&kernelContext_3)->draw_0 = draw_2;

#line 2335
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 2335
    (&kernelContext_3)->instances_0 = instances_2;

#line 2335
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 2335
    (&kernelContext_3)->frame_0 = frame_2;

#line 2335
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 2335
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 2335
    (&kernelContext_3)->materials_0 = materials_2;

#line 2335
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 2335
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 2335
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 2335
    (&kernelContext_3)->mro_textures_0 = mro_textures_2;

#line 2335
    (&kernelContext_3)->emissive_textures_0 = emissive_textures_2;

#line 2335
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 2335
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 2335
    (&kernelContext_3)->lights_0 = lights_2;

#line 2335
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 2335
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 2335
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 2335
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 2335
    (&kernelContext_3)->probes_0 = probes_2;

#line 2335
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 2335
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 5351
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 5353
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 5227
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 5227
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 5233
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4961
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4965
    float _S16 = axis_0.y;

#line 4965
    bool _S17;

#line 4965
    if(_S15 >= _S16)
    {

#line 4965
        _S17 = _S15 >= (axis_0.z);

#line 4965
    }
    else
    {

#line 4965
        _S17 = false;

#line 4965
    }

#line 4965
    float2 planar_0;

#line 4965
    if(_S17)
    {

#line 4965
        planar_0 = world_position_0.zy;

#line 4965
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4969
            planar_0 = world_position_0.xz;

#line 4969
        }
        else
        {

#line 4969
            planar_0 = world_position_0.xy;

#line 4969
        }

#line 4965
    }

#line 4977
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1094
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) & 65535U;
}


#line 1545
float4 base_color_texel_0(const GpuMaterial_natural_0 thread* material_2, float2 uv_0, KernelContext_0 thread* kernelContext_5)
{

#line 1545
    uint _S18 = base_color_layer_0(material_2);


    bool named_0 = _S18 != 65535U;

#line 1548
    uint _S19;

    if(named_0)
    {

#line 1550
        _S19 = _S18;

#line 1550
    }
    else
    {

#line 1550
        _S19 = 0U;

#line 1550
    }

#line 1550
    float3 _S20 = float3(uv_0, float(_S19));

#line 1549
    float4 texel_0 = ((kernelContext_5->base_color_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S20)).xy, uint(((_S20)).z)));

#line 1549
    float4 _S21;

    if(named_0)
    {

#line 1551
        _S21 = texel_0;

#line 1551
    }
    else
    {

#line 1551
        _S21 = float4(1.0f, 1.0f, 1.0f, 1.0f);

#line 1551
    }

#line 1551
    return _S21;
}


#line 1203
bool alpha_masked_0(const GpuMaterial_natural_0 thread* material_3, float alpha_0)
{

#line 1203
    bool _S22;

    if(((material_3->flags_2) & 1U) != 0U)
    {

#line 1205
        _S22 = alpha_0 < (material_3->alpha_cutoff_0);

#line 1205
    }
    else
    {

#line 1205
        _S22 = false;

#line 1205
    }

#line 1205
    return _S22;
}


#line 1238
float3 double_sided_normal_0(const GpuMaterial_natural_0 thread* material_4, float3 normal_2, bool front_facing_0)
{

#line 1238
    bool _S23;

    if(((material_4->flags_2) & 2U) != 0U)
    {

#line 1240
        _S23 = !front_facing_0;

#line 1240
    }
    else
    {

#line 1240
        _S23 = false;

#line 1240
    }

#line 1240
    float3 _S24;

#line 1240
    if(_S23)
    {

#line 1240
        _S24 = - normal_2;

#line 1240
    }
    else
    {

#line 1240
        _S24 = normal_2;

#line 1240
    }

#line 1240
    return _S24;
}


#line 1109
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_5)
{
    return (material_5->color_normal_pages_0) >> 16U;
}


#line 4998
float3 orthonormal_tangent_0(float3 normal_3)
{
    float _S25 = normal_3.z;

#line 5000
    float sign_z_0;

#line 5000
    if(_S25 >= 0.0f)
    {

#line 5000
        sign_z_0 = 1.0f;

#line 5000
    }
    else
    {

#line 5000
        sign_z_0 = -1.0f;

#line 5000
    }
    float a_0 = -1.0f / (sign_z_0 + _S25);
    float _S26 = normal_3.x;

#line 5002
    float _S27 = sign_z_0 * _S26;

#line 5002
    return float3(1.0f + _S27 * _S26 * a_0, _S27 * normal_3.y * a_0, - sign_z_0 * _S26);
}


#line 5052
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_4)
{
    float _S28 = duvdy_0.y;

#line 5054
    float _S29 = duvdx_0.y;

#line 5054
    float winding_0;
    if((duvdx_0.x * _S28 - duvdy_0.x * _S29) < 0.0f)
    {

#line 5055
        winding_0 = -1.0f;

#line 5055
    }
    else
    {

#line 5055
        winding_0 = 1.0f;

#line 5055
    }
    float3 tangent_2 = (float3(_S28)  * dpdx_0 - float3(_S29)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_4;

#line 5064
    float3 tangent_3 = tangent_2 - normal_4 * float3(dot(normal_4, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 5065
    float3 _S30;

#line 5074
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 5074
        _S30 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 5074
    }
    else
    {

#line 5074
        _S30 = orthonormal_tangent_0(normal_4);

#line 5074
    }

#line 5074
    (&basis_4)->tangent_1 = _S30;

    (&basis_4)->bitangent_0 = cross(normal_4, _S30);
    return basis_4;
}


#line 2063
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


#line 5134
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_5, float2 uv_2, KernelContext_0 thread* kernelContext_6)
{

#line 5146
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_2);
    float2 duvdy_1 = dfdy(uv_2);

    if(layer_0 == 65535U)
    {
        return normal_5;
    }

    thread TangentFrame_0 basis_5;

#line 5156
    uint _S31 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 5165
        (&basis_5)->normal_0 = normal_5;
        float3 tangent_4 = input_0->world_tangent_0 - normal_5 * float3(dot(normal_5, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 5167
        float3 _S32;

#line 5172
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 5172
            _S32 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 5172
        }
        else
        {

#line 5172
            _S32 = orthonormal_tangent_0(normal_5);

#line 5172
        }

#line 5172
        (&basis_5)->tangent_1 = _S32;

#line 5178
        float3 _S33 = cross((&basis_5)->normal_0, _S32);

#line 5178
        float _S34;
        if((_S31 & 2U) != 0U)
        {

#line 5179
            _S34 = -1.0f;

#line 5179
        }
        else
        {

#line 5179
            _S34 = 1.0f;

#line 5179
        }

#line 5178
        (&basis_5)->bitangent_0 = _S33 * float3(_S34) ;

#line 5157
    }
    else
    {

#line 5183
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_5);

#line 5157
    }

#line 5187
    float3 _S35 = float3(uv_2, float(layer_0));
    float3 _S36 = ((kernelContext_6->normal_textures_0).sample((kernelContext_6->base_color_sampler_0), ((_S35)).xy, uint(((_S35)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 5188
    thread float3 tangent_space_0 = _S36;
    tangent_space_0.xy = _S36.xy * float2(normal_scale_1) ;

#line 5194
    float3 _S37 = normalize(tangent_space_0);

#line 5194
    tangent_space_0 = _S37;
    return normalize(float3(_S37.x)  * (&basis_5)->tangent_1 + float3(_S37.y)  * (&basis_5)->bitangent_0 + float3(_S37.z)  * (&basis_5)->normal_0);
}


#line 3084
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 3095
    float3 _S38;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 3096
        _S38 = - facet_1;

#line 3096
    }
    else
    {

#line 3096
        _S38 = facet_1;

#line 3096
    }

#line 3096
    return _S38;
}


#line 1127
uint mro_layer_0(const GpuMaterial_natural_0 thread* material_7)
{
    return (material_7->mro_emissive_pages_0) & 65535U;
}


#line 1953
float4 mro_texel_0(const GpuMaterial_natural_0 thread* material_8, float2 uv_3, KernelContext_0 thread* kernelContext_7)
{
    float2 duvdx_2 = dfdx(uv_3);
    float2 duvdy_2 = dfdy(uv_3);

#line 1956
    uint _S39 = mro_layer_0(material_8);

    if(_S39 == 65535U)
    {
        return float4(1.0f, 1.0f, 1.0f, 1.0f);
    }

    float3 _S40 = float3(uv_3, float(_S39));

#line 1962
    return ((kernelContext_7->mro_textures_0).sample((kernelContext_7->base_color_sampler_0), ((_S40)).xy, uint(((_S40)).z), gradient2d((duvdx_2), (duvdy_2))));
}


#line 1139
uint emissive_layer_0(const GpuMaterial_natural_0 thread* material_9)
{
    return (material_9->mro_emissive_pages_0) >> 16U;
}


#line 1972
float4 emissive_texel_0(const GpuMaterial_natural_0 thread* material_10, float2 uv_4, KernelContext_0 thread* kernelContext_8)
{
    float2 duvdx_3 = dfdx(uv_4);
    float2 duvdy_3 = dfdy(uv_4);

#line 1975
    uint _S41 = emissive_layer_0(material_10);

    if(_S41 == 65535U)
    {
        return float4(1.0f, 1.0f, 1.0f, 1.0f);
    }

    float3 _S42 = float3(uv_4, float(_S41));

#line 1981
    return ((kernelContext_8->emissive_textures_0).sample((kernelContext_8->base_color_sampler_0), ((_S42)).xy, uint(((_S42)).z), gradient2d((duvdx_3), (duvdy_3))));
}


#line 1999
float metallic_of_0(const GpuMaterial_natural_0 thread* material_11, float4 mro_0)
{
    return saturate(material_11->metallic_0 * mro_0.z);
}


#line 2489
float specular_aa_kernel_0(float3 normal_6)
{
    float3 dndx_0 = dfdx(normal_6);
    float3 dndy_0 = dfdy(normal_6);


    return min(2.0f * (0.25f * (dot(dndx_0, dndx_0) + dot(dndy_0, dndy_0))), 0.18000000715255737f);
}


#line 4383
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_9)
{
    uint _S43 = max(kernelContext_9->frame_0->cluster_grid_0.x, 1U);
    uint _S44 = max(kernelContext_9->frame_0->cluster_grid_0.y, 1U);
    uint _S45 = max(kernelContext_9->frame_0->cluster_grid_0.z, 1U);
    uint _S46 = max(kernelContext_9->frame_0->cluster_grid_0.w, 1U);

#line 4393
    uint _S47 = uint(pixel_0.x) / _S46;

#line 4393
    uint _S48 = min(_S47, _S43 - 1U);
    uint _S49 = uint(pixel_0.y) / _S46;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4404
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S45 - 1U))) * _S44 + min(_S49, _S44 - 1U)) * _S43 + _S48;
}


#line 2516
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2537
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_10)
{

#line 2537
    texture2d<float, access::sample> _S50 = kernelContext_10->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S50).get_width(0)),(*((&height_1)) = (_S50).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2543
    float2 _S51 = float2(1.0f) ;
    float2 _S52 = extent_1 - _S51;

#line 2544
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S52);
    float2 high_1 = min(low_1 + _S51, _S52);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2562
float2 decode_dfg_pair_0(float4 texel_1)
{
    return float2(texel_1.x * 65280.0f + texel_1.y * 255.0f, texel_1.z * 65280.0f + texel_1.w * 255.0f) / float2(65535.0f) ;
}


#line 2574
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_11)
{
    int _S53 = tap_1->lo_0.x;

#line 2576
    int _S54 = tap_1->lo_0.y;

#line 2576
    int3 _S55 = int3(_S53, _S54, int(0));
    int _S56 = tap_1->hi_0.x;

#line 2577
    int3 _S57 = int3(_S56, _S54, int(0));
    float2 _S58 = float2(tap_1->weight_0.x) ;
    int _S59 = tap_1->hi_0.y;

#line 2579
    int3 _S60 = int3(_S53, _S59, int(0));
    int3 _S61 = int3(_S56, _S59, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_11->specular_dfg_0).read(vec<uint,2>(((_S55)).xy), uint(((_S55)).z)))), decode_dfg_pair_0(((kernelContext_11->specular_dfg_0).read(vec<uint,2>(((_S57)).xy), uint(((_S57)).z)))), _S58), mix(decode_dfg_pair_0(((kernelContext_11->specular_dfg_0).read(vec<uint,2>(((_S60)).xy), uint(((_S60)).z)))), decode_dfg_pair_0(((kernelContext_11->specular_dfg_0).read(vec<uint,2>(((_S61)).xy), uint(((_S61)).z)))), _S58), float2(tap_1->weight_0.y) );
}


#line 4334
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 4350
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 4362
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 4369
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2903
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2903
    float4 _S62 = float4(light_0->tangent_0) ;

    float3 _S63 = _S62.xyz;

#line 2905
    float3 across_0 = _S63 * float3(_S62.w) ;

#line 2905
    float4 _S64 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S63, _S64.xyz) * float3(_S64.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S65 = centre_0 - across_0;

#line 2908
    (*corners_0)[int(0)] = _S65 - down_0;
    float3 _S66 = centre_0 + across_0;

#line 2909
    (*corners_0)[int(1)] = _S66 - down_0;
    (*corners_0)[int(2)] = _S66 + down_0;
    (*corners_0)[int(3)] = _S65 + down_0;
    return;
}


#line 2661
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_7, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_7 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2664
    float3 seed_0;
    if((abs(normal_7.z)) < 0.89999997615814209f)
    {

#line 2665
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2665
    }
    else
    {

#line 2665
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2665
    }

#line 2665
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2666
        tangent_5 = across_1 / float3(span_0) ;

#line 2666
    }
    else
    {

#line 2666
        tangent_5 = normalize(cross(seed_0, normal_7));

#line 2666
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_7, tangent_5), normal_7);
}


#line 2642
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2732
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2732
    float3 _S67 = polygon_0->corner_0[int(0)];

#line 2732
    float3 _S68 = polygon_0->corner_0[int(1)];

#line 2732
    float3 _S69 = polygon_0->corner_0[int(2)];

#line 2732
    float3 _S70 = polygon_0->corner_0[int(3)];

#line 2738
    float3 _S71 = float3(0.0f, 0.0f, 0.0f);


    float _S72 = polygon_0->corner_0[int(0)].z;

#line 2741
    int count_1;

#line 2741
    if(_S72 > 0.0f)
    {

#line 2741
        count_1 = int(1);

#line 2741
    }
    else
    {

#line 2741
        count_1 = int(0);

#line 2741
    }
    float _S73 = _S68.z;

#line 2742
    int _S74;

#line 2742
    if(_S73 > 0.0f)
    {

#line 2742
        _S74 = int(2);

#line 2742
    }
    else
    {

#line 2742
        _S74 = int(0);

#line 2742
    }

#line 2742
    int config_0 = count_1 + _S74;
    float _S75 = _S69.z;

#line 2743
    if(_S75 > 0.0f)
    {

#line 2743
        count_1 = int(4);

#line 2743
    }
    else
    {

#line 2743
        count_1 = int(0);

#line 2743
    }

#line 2743
    int config_1 = config_0 + count_1;
    float _S76 = _S70.z;

#line 2744
    if(_S76 > 0.0f)
    {

#line 2744
        count_1 = int(8);

#line 2744
    }
    else
    {

#line 2744
        count_1 = int(0);

#line 2744
    }

#line 2744
    int config_2 = config_1 + count_1;

#line 2744
    float3 l0_0;

#line 2744
    float3 l1_0;

#line 2744
    float3 l2_0;

#line 2744
    float3 l3_0;

#line 2744
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2747
        float3 _S77 = float3(_S72) ;


        float3 _S78 = float3(- _S73)  * _S67 + _S77 * _S68;
        float3 _S79 = float3(- _S76)  * _S67 + _S77 * _S70;

#line 2751
        count_1 = int(3);

#line 2751
        l0_0 = _S67;

#line 2751
        l1_0 = _S78;

#line 2751
        l2_0 = _S79;

#line 2751
        l3_0 = _S70;

#line 2751
        l4_0 = _S71;

#line 2747
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2753
            float3 _S80 = float3(_S73) ;


            float3 _S81 = float3(- _S72)  * _S68 + _S80 * _S67;
            float3 _S82 = float3(- _S75)  * _S68 + _S80 * _S69;

#line 2757
            count_1 = int(3);

#line 2757
            l0_0 = _S81;

#line 2757
            l1_0 = _S68;

#line 2757
            l2_0 = _S82;

#line 2757
            l3_0 = _S70;

#line 2757
            l4_0 = _S71;

#line 2753
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S83 = float3(- _S75)  * _S68 + float3(_S73)  * _S69;
                float3 _S84 = float3(- _S76)  * _S67 + float3(_S72)  * _S70;

#line 2763
                count_1 = int(4);

#line 2763
                l0_0 = _S67;

#line 2763
                l1_0 = _S68;

#line 2763
                l2_0 = _S83;

#line 2763
                l3_0 = _S84;

#line 2763
                l4_0 = _S71;

#line 2759
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2765
                    float3 _S85 = float3(_S75) ;


                    float3 _S86 = float3(- _S76)  * _S69 + _S85 * _S70;
                    float3 _S87 = float3(- _S73)  * _S69 + _S85 * _S68;

#line 2769
                    count_1 = int(3);

#line 2769
                    l0_0 = _S86;

#line 2769
                    l1_0 = _S87;

#line 2769
                    l2_0 = _S69;

#line 2769
                    l3_0 = _S70;

#line 2769
                    l4_0 = _S71;

#line 2765
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S88 = float3(- _S72)  * _S68 + float3(_S73)  * _S67;
                        float3 _S89 = float3(- _S76)  * _S69 + float3(_S75)  * _S70;

#line 2775
                        count_1 = int(4);

#line 2775
                        l0_0 = _S88;

#line 2775
                        l1_0 = _S68;

#line 2775
                        l2_0 = _S69;

#line 2775
                        l3_0 = _S89;

#line 2775
                        l4_0 = _S71;

#line 2771
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2777
                            float3 _S90 = float3(- _S76) ;


                            float3 _S91 = _S90 * _S67 + float3(_S72)  * _S70;
                            float3 _S92 = _S90 * _S69 + float3(_S75)  * _S70;

#line 2781
                            count_1 = int(5);

#line 2781
                            l0_0 = _S67;

#line 2781
                            l1_0 = _S68;

#line 2781
                            l2_0 = _S69;

#line 2781
                            l3_0 = _S92;

#line 2781
                            l4_0 = _S91;

#line 2777
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2783
                                float3 _S93 = float3(_S76) ;


                                float3 _S94 = float3(- _S72)  * _S70 + _S93 * _S67;
                                float3 _S95 = float3(- _S75)  * _S70 + _S93 * _S69;

#line 2787
                                count_1 = int(3);

#line 2787
                                l0_0 = _S94;

#line 2787
                                l1_0 = _S95;

#line 2787
                                l2_0 = _S70;

#line 2787
                                l3_0 = _S70;

#line 2787
                                l4_0 = _S71;

#line 2783
                            }
                            else
                            {

#line 2790
                                if(config_2 == int(9))
                                {

                                    float3 _S96 = float3(- _S73)  * _S67 + float3(_S72)  * _S68;
                                    float3 _S97 = float3(- _S75)  * _S70 + float3(_S76)  * _S69;

#line 2794
                                    count_1 = int(4);

#line 2794
                                    l0_0 = _S67;

#line 2794
                                    l1_0 = _S96;

#line 2794
                                    l2_0 = _S97;

#line 2794
                                    l3_0 = _S70;

#line 2794
                                    l4_0 = _S71;

#line 2790
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S98 = float3(- _S76)  * _S69 + float3(_S75)  * _S70;
                                        float3 _S99 = float3(- _S75)  * _S68 + float3(_S73)  * _S69;

#line 2801
                                        count_1 = int(5);

#line 2801
                                        l0_0 = _S67;

#line 2801
                                        l1_0 = _S68;

#line 2801
                                        l2_0 = _S99;

#line 2801
                                        l3_0 = _S98;

#line 2801
                                        l4_0 = _S70;

#line 2796
                                    }
                                    else
                                    {

#line 2803
                                        if(config_2 == int(12))
                                        {

                                            float3 _S100 = float3(- _S73)  * _S69 + float3(_S75)  * _S68;
                                            float3 _S101 = float3(- _S72)  * _S70 + float3(_S76)  * _S67;

#line 2807
                                            count_1 = int(4);

#line 2807
                                            l0_0 = _S101;

#line 2807
                                            l1_0 = _S100;

#line 2807
                                            l2_0 = _S69;

#line 2807
                                            l3_0 = _S70;

#line 2807
                                            l4_0 = _S71;

#line 2803
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S102 = float3(- _S75)  * _S68 + float3(_S73)  * _S69;
                                                float3 _S103 = float3(- _S73)  * _S67 + float3(_S72)  * _S68;

#line 2815
                                                count_1 = int(5);

#line 2815
                                                l0_0 = _S67;

#line 2815
                                                l1_0 = _S103;

#line 2815
                                                l2_0 = _S102;

#line 2815
                                                l3_0 = _S69;

#line 2815
                                                l4_0 = _S70;

#line 2809
                                            }
                                            else
                                            {

#line 2817
                                                if(config_2 == int(14))
                                                {

#line 2817
                                                    float3 _S104 = float3(- _S72) ;


                                                    float3 _S105 = _S104 * _S70 + float3(_S76)  * _S67;
                                                    float3 _S106 = _S104 * _S68 + float3(_S73)  * _S67;

#line 2821
                                                    count_1 = int(5);

#line 2821
                                                    l0_0 = _S106;

#line 2821
                                                    l1_0 = _S105;

#line 2817
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2823
                                                        count_1 = int(4);

#line 2823
                                                    }
                                                    else
                                                    {

#line 2823
                                                        count_1 = int(0);

#line 2823
                                                    }

#line 2823
                                                    l0_0 = _S67;

#line 2823
                                                    l1_0 = _S71;

#line 2817
                                                }

#line 2738
                                                float3 _S107 = l1_0;

#line 2738
                                                l1_0 = _S68;

#line 2738
                                                l2_0 = _S69;

#line 2738
                                                l3_0 = _S70;

#line 2738
                                                l4_0 = _S107;

#line 2809
                                            }

#line 2803
                                        }

#line 2796
                                    }

#line 2790
                                }

#line 2783
                            }

#line 2777
                        }

#line 2771
                    }

#line 2765
                }

#line 2759
            }

#line 2753
        }

#line 2747
    }

#line 2831
    if(count_1 <= int(3))
    {

#line 2831
        l3_0 = l0_0;

#line 2831
        l4_0 = l0_0;

#line 2831
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2836
            l4_0 = l0_0;

#line 2836
        }

#line 2831
    }

#line 2841
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2704
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2710
    float weight_1;

#line 2715
    if(cosine_0 > 0.0f)
    {

#line 2715
        weight_1 = fit_0;

#line 2715
    }
    else
    {

#line 2715
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2715
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2861
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2863
    int corner_1 = int(0);
    for(;;)
    {

#line 2864
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2864
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2864
        corner_1 = corner_1 + int(1);

#line 2864
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2869
    thread LtcPolygon_0 _S108 = polygon_1;

#line 2869
    LtcPolygon_0 _S109 = ltc_clip_0(&_S108);
    polygon_1 = _S109;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2873
    int at_2 = int(0);

    for(;;)
    {

#line 2875
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2875
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2875
        at_2 = at_2 + int(1);

#line 2875
    }

#line 2882
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2882
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2883
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2883
    }
    else
    {

#line 2883
        sum_1 = sum_0;

#line 2883
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2887
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2887
    }

#line 2894
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2590
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_12)
{
    int _S110 = tap_2->lo_0.x;

#line 2592
    int _S111 = tap_2->lo_0.y;

#line 2592
    int3 _S112 = int3(_S110, _S111, int(0));
    int _S113 = tap_2->hi_0.x;

#line 2593
    int3 _S114 = int3(_S113, _S111, int(0));
    float4 _S115 = float4(tap_2->weight_0.x) ;
    int _S116 = tap_2->hi_0.y;

#line 2595
    int3 _S117 = int3(_S110, _S116, int(0));
    int3 _S118 = int3(_S113, _S116, int(0));

    return mix(mix(((kernelContext_12->ltc_matrix_0).read(vec<uint,2>(((_S112)).xy), uint(((_S112)).z))), ((kernelContext_12->ltc_matrix_0).read(vec<uint,2>(((_S114)).xy), uint(((_S114)).z))), _S115), mix(((kernelContext_12->ltc_matrix_0).read(vec<uint,2>(((_S117)).xy), uint(((_S117)).z))), ((kernelContext_12->ltc_matrix_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z))), _S115), float4(tap_2->weight_0.y) );
}


#line 2677
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2414
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2421
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2428
    float _S119 = 1.0f - alpha2_0;

#line 2433
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S119 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S119 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3506
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3506
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3566
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3538
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_15)
{
    return rect_1.x / kernelContext_15->frame_0->shadow_params_0.x;
}


#line 3135
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3493
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_16)
{

#line 3493
    uint _S120;

    if(uint(pixel_1.x) < (kernelContext_16->frame_0->shadow_filter_0.z))
    {

#line 3495
        _S120 = kernelContext_16->frame_0->shadow_filter_0.x;

#line 3495
    }
    else
    {

#line 3495
        _S120 = kernelContext_16->frame_0->shadow_filter_0.y;

#line 3495
    }

#line 3495
    return _S120;
}


#line 3518
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_17)
{
    return kernelContext_17->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3518
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_18)
{
    return kernelContext_18->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 383
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3588
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_19)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S121 = spoke_0.x;

#line 3593
    float _S122 = rotation_0.x;

#line 3593
    float _S123 = spoke_0.y;

#line 3593
    float _S124 = rotation_0.y;


    float _S125 = ((kernelContext_19->shadow_atlas_0).sample_compare((kernelContext_19->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S121 * _S122 - _S123 * _S124, _S121 * _S124 + _S123 * _S122) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3596
    return _S125;
}


#line 3676
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_20)
{

#line 3676
    float4 _S126 = atlas_rect_1(tile_2, kernelContext_20);


    if(atlas_rect_is_empty_0(_S126))
    {
        return 1.0f;
    }

#line 3681
    float2 _S127 = atlas_step_1(_S126, kernelContext_20);

#line 3681
    int y_1 = int(-1);

#line 3681
    float visibility_0 = 0.0f;

#line 3686
    for(;;)
    {

#line 3686
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3686
            break;
        }

#line 3686
        int x_0 = int(-1);

        for(;;)
        {

#line 3688
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3688
                break;
            }

#line 3688
            float _S128 = tile_tap_0(_S126, _S127, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_20);

            float visibility_1 = visibility_0 + _S128;

#line 3688
            x_0 = x_0 + int(1);

#line 3688
            visibility_0 = visibility_1;

#line 3688
        }

#line 3686
        y_1 = y_1 + int(1);

#line 3686
    }

#line 3694
    return visibility_0 / 9.0f;
}


#line 3451
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3618
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_21)
{
    float2 _S129 = shadow_rotation_0(pixel_3);

#line 3620
    float4 _S130 = atlas_rect_1(tile_3, kernelContext_21);

    if(atlas_rect_is_empty_0(_S130))
    {
        return 1.0f;
    }

#line 3624
    float2 _S131 = atlas_step_1(_S130, kernelContext_21);

#line 3624
    uint spot_0 = 0U;

#line 3624
    float probe_0 = 0.0f;

#line 3629
    for(;;)
    {

#line 3629
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3629
            break;
        }

#line 3629
        float _S132 = tile_tap_0(_S130, _S131, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S129, reference_2, kernelContext_21);

        float probe_1 = probe_0 + _S132;

#line 3629
        spot_0 = spot_0 + 1U;

#line 3629
        probe_0 = probe_1;

#line 3629
    }

#line 3638
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3644
    uint index_2 = 0U;

#line 3644
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3648
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3648
            break;
        }

#line 3648
        float _S133 = tile_tap_0(_S130, _S131, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S129, reference_2, kernelContext_21);

        float visibility_3 = visibility_2 + _S133;

#line 3648
        index_2 = index_2 + 1U;

#line 3648
        visibility_2 = visibility_3;

#line 3648
    }

#line 3653
    return visibility_2 / 32.0f;
}


#line 3729
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_22)
{
    float2 texel_2 = kernelContext_22->frame_0->shadow_params_0.xy;

#line 3731
    float4 _S134 = atlas_rect_0(cascade_0, kernelContext_22);

#line 3731
    float2 _S135 = atlas_step_0(_S134, kernelContext_22);


    float2 _S136 = float2(0.5f, 0.5f) * _S135;


    float2 _S137 = float2(1.0f, 1.0f);

#line 3737
    float2 _S138 = _S137 / texel_2;

#line 3737
    uint index_3 = 0U;

#line 3737
    float sum_2 = 0.0f;

#line 3737
    float found_0 = 0.0f;



    for(;;)
    {

#line 3741
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3741
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S139 = spoke_1.x;

#line 3744
        float _S140 = rotation_1.x;

#line 3744
        float _S141 = spoke_1.y;

#line 3744
        float _S142 = rotation_1.y;

#line 3752
        int3 _S143 = int3(int2(min(atlas_uv_0(_S134, clamp(tile_uv_4 + float2(_S139 * _S140 - _S141 * _S142, _S139 * _S142 + _S141 * _S140) * _S135, _S136, float2(1.0f)  - _S136)) * _S138, _S138 - _S137)), int(0));

#line 3752
        float depth_1 = ((kernelContext_22->shadow_atlas_0).read(vec<uint,2>(((_S143)).xy), uint(((_S143)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3756
            sum_2 = sum_2 + depth_1;

#line 3756
            found_0 = found_1;

#line 3753
        }

#line 3741
        index_3 = index_3 + 1U;

#line 3741
    }

#line 3760
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3771
    float _S144 = 2.0f * kernelContext_22->frame_0->cascade_far_0[cascade_0];

#line 3771
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S144 + 40.0f);

#line 3771
    float _S145 = tile_texels_0(_S134, kernelContext_22);

    return clamp(separation_0 * 0.01999999955296516f / (_S144 / _S145), 2.0f, 8.0f);
}


#line 3825
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_23)
{

#line 3826
    float4 _S146 = atlas_rect_0(cascade_1, kernelContext_23);

#line 3860
    if(atlas_rect_is_empty_0(_S146))
    {


        return 1.0f;
    }
    float _S147 = 2.0f * kernelContext_23->frame_0->cascade_far_0[cascade_1];

#line 3866
    float _S148 = tile_texels_0(_S146, kernelContext_23);

#line 3866
    float texel_world_0 = _S147 / _S148;

#line 3873
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_23->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_23->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_23->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3877
    bool _S149;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3878
        _S149 = true;

#line 3878
    }
    else
    {

#line 3878
        _S149 = (ndc_0.z) <= 0.0f;

#line 3878
    }

#line 3878
    if(_S149)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3888
    uint _S150 = shadow_filter_mode_0(pixel_4, kernelContext_23);

#line 3905
    if(_S150 == 2U)
    {

#line 3905
        float _S151 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_23);

        return _S151;
    }
    if(_S150 == 1U)
    {

#line 3909
        float _S152 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_23);



        return _S152;
    }

    float _S153 = ndc_0.z;

#line 3916
    float _S154 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S153, shadow_rotation_0(pixel_4), kernelContext_23);

#line 3916
    float _S155 = tile_pcf_0(cascade_1, tile_uv_5, _S153, pixel_4, _S154, kernelContext_23);
    return _S155;
}


#line 3996
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_24)
{
    uint cascade_2;

#line 3998
    bool covered_0;

#line 4007
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 4019
    float eye_distance_0 = length(world_position_5 - kernelContext_24->frame_0->camera_position_0.xyz);

#line 4019
    uint index_4 = 0U;

#line 4027
    for(;;)
    {

#line 4027
        if(index_4 < 2U)
        {
        }
        else
        {

#line 4027
            covered_0 = false;

#line 4027
            cascade_2 = 1U;

#line 4027
            break;
        }
        if(eye_distance_0 < kernelContext_24->frame_0->cascade_far_0[index_4])
        {

#line 4029
            covered_0 = true;

#line 4029
            cascade_2 = index_4;



            break;
        }

#line 4027
        index_4 = index_4 + 1U;

#line 4027
    }

#line 4036
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 4036
    }

#line 4036
    float _S156 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_24);

#line 4043
    uint _S157 = cascade_2 + 1U;

#line 4043
    if(_S157 >= 2U)
    {



        return _S156;
    }

#line 4056
    float band_0 = kernelContext_24->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_24->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S156;
    }

#line 4064
    float _S158 = cascade_visibility_0(_S157, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_24);

#line 4075
    return mix(_S156, _S158, blend_0);
}


#line 5263
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_25)
{

#line 5263
    texture2d<float, access::sample> _S159 = kernelContext_25->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S159).get_width(0)),(*((&height_2)) = (_S159).get_height(0));

    int3 _S160 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 5269
    return ((kernelContext_25->contact_shadow_0).read(vec<uint,2>(((_S160)).xy), uint(((_S160)).z)).x);
}


#line 3968
float3 cascade_tint_0(uint cascade_3, float blend_1)
{
    if(cascade_3 >= 2U)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }
    uint _S161 = cascade_3 + 1U;

#line 3974
    if(_S161 >= 2U)
    {


        return CASCADE_TINTS_0[cascade_3];
    }
    return mix(CASCADE_TINTS_0[cascade_3], CASCADE_TINTS_0[_S161], float3(blend_1) );
}


#line 4286
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S162 = axis_2.x;

#line 4289
    float _S163 = axis_2.y;

#line 4289
    bool _S164;

#line 4289
    if(_S162 >= _S163)
    {

#line 4289
        _S164 = _S162 >= (axis_2.z);

#line 4289
    }
    else
    {

#line 4289
        _S164 = false;

#line 4289
    }

#line 4289
    uint _S165;

#line 4289
    if(_S164)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 4291
            _S165 = 0U;

#line 4291
        }
        else
        {

#line 4291
            _S165 = 1U;

#line 4291
        }

#line 4291
        return _S165;
    }
    if(_S163 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 4295
            _S165 = 2U;

#line 4295
        }
        else
        {

#line 4295
            _S165 = 3U;

#line 4295
        }

#line 4295
        return _S165;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 4297
        _S165 = 4U;

#line 4297
    }
    else
    {

#line 4297
        _S165 = 5U;

#line 4297
    }

#line 4297
    return _S165;
}


#line 370
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 4182
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_26)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 4185
    float4 _S166 = atlas_rect_0(atlas_0, kernelContext_26);

    if(atlas_rect_is_empty_0(_S166))
    {


        return 1.0f;
    }

#line 4191
    float _S167 = tile_texels_0(_S166, kernelContext_26);

    float texel_world_1 = map_world_0 / _S167;

#line 4203
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_26->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 4210
    float _S168 = clip_1.w;

#line 4210
    if(_S168 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S168) ;

#line 4214
    bool _S169;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 4215
        _S169 = true;

#line 4215
    }
    else
    {

#line 4215
        _S169 = (ndc_1.z) <= 0.0f;

#line 4215
    }

#line 4215
    if(_S169)
    {

#line 4215
        _S169 = true;

#line 4215
    }
    else
    {

#line 4215
        _S169 = (ndc_1.z) > 1.0f;

#line 4215
    }

#line 4215
    if(_S169)
    {

#line 4222
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 4227
    uint _S170 = shadow_filter_mode_0(pixel_6, kernelContext_26);

#line 4236
    if(_S170 == 2U)
    {

#line 4236
        float _S171 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_26);

        return _S171;
    }

#line 4238
    float _S172 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_26);

    return _S172;
}


#line 4305
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_0, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_27)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 4313
    float _S173 = punctual_visibility_0(base_0 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_27);

#line 4319
    return _S173;
}


#line 4247
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_28)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 4254
    float4 _S174 = float4(light_2->direction_0) ;

#line 4261
    float cos_outer_1 = _S174.w;

#line 4261
    float _S175 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S174.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_28);

#line 4268
    return _S175;
}


#line 2618
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 5250
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 5252
    float3 _S176;
    if((length(decoded_0)) < 0.5f)
    {

#line 5253
        _S176 = shading_normal_1;

#line 5253
    }
    else
    {

#line 5253
        _S176 = normalize(decoded_0);

#line 5253
    }

#line 5253
    return _S176;
}


#line 4888
float3 sky_irradiance_0(float3 normal_8, KernelContext_0 thread* kernelContext_29)
{
    float4 basis_6 = float4(normal_8, 1.0f);
    return max(float3(dot(kernelContext_29->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_29->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_29->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4792
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4792
    float reach_0 = 0.0f;

#line 4792
    uint axis_3 = 0U;


    for(;;)
    {

#line 4795
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4795
            break;
        }

#line 4795
        uint _S177 = axis_3;

#line 4795
        bool _S178;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4797
            _S178 = true;

#line 4797
        }
        else
        {

#line 4797
            _S178 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4797
        }

#line 4797
        if(_S178)
        {

#line 4798
            axis_3 = axis_3 + 1U;

#line 4795
            continue;
        }

#line 4795
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S177] - 1.0f));

#line 4795
        axis_3 = axis_3 + 1U;

#line 4795
    }

#line 4802
    return reach_0;
}


#line 4822
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4822
    uint level_0 = 0U;

    for(;;)
    {

#line 4824
        uint _S179 = level_0 + 1U;

#line 4824
        if(_S179 < levels_0)
        {
        }
        else
        {

#line 4824
            break;
        }
        float _S180 = float(level_0);

#line 4826
        float at_3 = reach_1 * exp2(- _S180);
        if(at_3 < 1.0f)
        {

#line 4828
            return float2(_S180, saturate((1.0f - at_3) / 0.25f));
        }

#line 4824
        level_0 = _S179;

#line 4824
    }

#line 4830
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4579
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4581
    uint _S181;
    if(at_4 >= count_2)
    {

#line 4582
        _S181 = at_4 - count_2;

#line 4582
    }
    else
    {

#line 4582
        _S181 = at_4;

#line 4582
    }

#line 4582
    return _S181;
}


#line 4605
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_30)
{
    uint3 counts_0 = kernelContext_30->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_30->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S182 = counts_0.x;
    uint _S183 = counts_0.y;



    return min(kernelContext_30->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S183 + probe_wrap_0(cell_2.y, offset_1.y, _S183)) * _S182 + probe_wrap_0(cell_2.x, offset_1.x, _S182), max(kernelContext_30->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4446
float sign_not_zero_0(float value_0)
{

#line 4446
    float _S184;

    if(value_0 >= 0.0f)
    {

#line 4448
        _S184 = 1.0f;

#line 4448
    }
    else
    {

#line 4448
        _S184 = -1.0f;

#line 4448
    }

#line 4448
    return _S184;
}


#line 4465
float2 oct_encode_0(float3 direction_1)
{
    float _S185 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S185) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4468
    float2 p_1;
    if(_S185 < 0.0f)
    {
        float _S186 = p_0.y;

#line 4471
        float _S187 = p_0.x;

#line 4471
        p_1 = float2((1.0f - abs(_S186)) * sign_not_zero_0(_S187), (1.0f - abs(_S187)) * sign_not_zero_0(_S186));

#line 4469
    }
    else
    {

#line 4469
        p_1 = p_0;

#line 4469
    }

#line 4474
    return p_1;
}


#line 4494
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_31)
{

#line 4494
    texture2d_array<float, access::sample> _S188 = kernelContext_31->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S188).get_width(0)),(*((&height_3)) = (_S188).get_height(0)),(*((&layers_0)) = (_S188).get_array_size());

#line 4499
    float2 _S189 = float2(0.5f) ;

#line 4499
    float2 _S190 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S189 + _S189) * float2(16.0f)  + _S190 - _S189;
    float2 _S191 = float2(float(width_3), float(height_3)) - _S190;

#line 4503
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S191);
    float2 high_2 = min(low_2 + _S190, _S191);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S192 = int(low_2.x);

#line 4508
    int _S193 = int(low_2.y);

#line 4508
    int4 _S194 = int4(_S192, _S193, layer_1, int(0));
    int _S195 = int(high_2.x);

#line 4509
    int4 _S196 = int4(_S195, _S193, layer_1, int(0));
    int _S197 = int(high_2.y);

#line 4510
    int4 _S198 = int4(_S192, _S197, layer_1, int(0));
    int4 _S199 = int4(_S195, _S197, layer_1, int(0));
    float2 _S200 = float2(weight_2.x) ;

#line 4512
    return mix(mix(((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S194)).xy), uint(((_S194)).z), uint(((_S194)).w))).xy, ((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S196)).xy), uint(((_S196)).z), uint(((_S196)).w))).xy, _S200), mix(((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S198)).xy), uint(((_S198)).z), uint(((_S198)).w))).xy, ((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S199)).xy), uint(((_S199)).z), uint(((_S199)).w))).xy, _S200), float2(weight_2.y) );
}


#line 4540
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_9, KernelContext_0 thread* kernelContext_32)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_9 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4543
    float2 _S201 = probe_moments_0(index_6, - to_probe_0, kernelContext_32);

#line 4549
    float _S202 = _S201.x;

#line 4549
    float _S203 = max(_S201.y - _S202 * _S202, 0.0f);
    float behind_0 = to_surface_0 - _S202;
    float bound_0 = _S203 / (_S203 + behind_0 * behind_0);

#line 4551
    float _S204;
    if(to_surface_0 <= _S202)
    {

#line 4552
        _S204 = 1.0f;

#line 4552
    }
    else
    {

#line 4552
        _S204 = bound_0 * bound_0 * bound_0;

#line 4552
    }

#line 4552
    return _S204;
}


#line 4562
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_10, KernelContext_0 thread* kernelContext_33)
{

#line 4562
    float _S205 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_10, kernelContext_33);

    return max(_S205, 0.00009999999747379f);
}


#line 1254
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4624
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4651
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_11, KernelContext_0 thread* kernelContext_34)
{

#line 4652
    uint _S206 = probe_row_0(level_2, cell_3, kernelContext_34);


    GpuProbe_natural_0 stored_0 = kernelContext_34->probes_0[_S206];

#line 4655
    float _S207 = probe_weight_0(_S206, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_11, kernelContext_34);



    thread WeightedProbe_0 corner_2;

#line 4659
    float4 _S208 = float4(_S207) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S208;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S208;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S208;
    (&corner_2)->weight_3 = _S207;
    return corner_2;
}


#line 4635
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S209 = float4(t_1) ;

#line 4638
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S209);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S209);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S209);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4723
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_12, KernelContext_0 thread* kernelContext_35)
{

#line 4723
    float3 _S210 = float3(1.0f) ;

#line 4728
    float3 _S211 = float3(0.0f, 0.0f, 0.0f);

#line 4728
    float3 last_1 = max(float3(kernelContext_35->frame_0->probe_counts_0.xyz) - _S210, _S211);



    float3 origin_2 = kernelContext_35->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_35->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S211, last_1);
    float3 base_1 = floor(grid_0);
    float3 f_0 = grid_0 - base_1;

    uint3 _S212 = uint3(base_1);



    uint3 _S213 = uint3(min(base_1 + _S210, last_1));

#line 4748
    float _S214 = inv_0.x;

#line 4748
    float _S215;

#line 4748
    if(_S214 != 0.0f)
    {

#line 4748
        _S215 = 1.0f / _S214;

#line 4748
    }
    else
    {

#line 4748
        _S215 = 0.0f;

#line 4748
    }
    float _S216 = inv_0.y;

#line 4749
    float _S217;

#line 4749
    if(_S216 != 0.0f)
    {

#line 4749
        _S217 = 1.0f / _S216;

#line 4749
    }
    else
    {

#line 4749
        _S217 = 0.0f;

#line 4749
    }
    float _S218 = inv_0.z;

#line 4750
    float _S219;

#line 4750
    if(_S218 != 0.0f)
    {

#line 4750
        _S219 = 1.0f / _S218;

#line 4750
    }
    else
    {

#line 4750
        _S219 = 0.0f;

#line 4750
    }

#line 4748
    float3 spacing_1 = float3(_S215, _S217, _S219);

#line 4757
    uint _S220 = _S212.x;

#line 4757
    uint _S221 = _S212.y;

#line 4757
    uint _S222 = _S212.z;

#line 4757
    WeightedProbe_0 _S223 = probe_corner_0(level_3, uint3(_S220, _S221, _S222), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);
    uint _S224 = _S213.x;

#line 4758
    WeightedProbe_0 _S225 = probe_corner_0(level_3, uint3(_S224, _S221, _S222), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4758
    float _S226 = f_0.x;

#line 4758
    thread WeightedProbe_0 _S227 = _S223;

#line 4758
    thread WeightedProbe_0 _S228 = _S225;

#line 4758
    WeightedProbe_0 _S229 = lerp_probe_0(&_S227, &_S228, _S226);
    uint _S230 = _S213.y;

#line 4759
    WeightedProbe_0 _S231 = probe_corner_0(level_3, uint3(_S220, _S230, _S222), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4759
    WeightedProbe_0 _S232 = probe_corner_0(level_3, uint3(_S224, _S230, _S222), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4759
    thread WeightedProbe_0 _S233 = _S231;

#line 4759
    thread WeightedProbe_0 _S234 = _S232;

#line 4759
    WeightedProbe_0 _S235 = lerp_probe_0(&_S233, &_S234, _S226);

    uint _S236 = _S213.z;

#line 4761
    WeightedProbe_0 _S237 = probe_corner_0(level_3, uint3(_S220, _S221, _S236), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4761
    WeightedProbe_0 _S238 = probe_corner_0(level_3, uint3(_S224, _S221, _S236), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4761
    thread WeightedProbe_0 _S239 = _S237;

#line 4761
    thread WeightedProbe_0 _S240 = _S238;

#line 4761
    WeightedProbe_0 _S241 = lerp_probe_0(&_S239, &_S240, _S226);

#line 4761
    WeightedProbe_0 _S242 = probe_corner_0(level_3, uint3(_S220, _S230, _S236), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4761
    WeightedProbe_0 _S243 = probe_corner_0(level_3, uint3(_S224, _S230, _S236), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4761
    thread WeightedProbe_0 _S244 = _S242;

#line 4761
    thread WeightedProbe_0 _S245 = _S243;

#line 4761
    WeightedProbe_0 _S246 = lerp_probe_0(&_S244, &_S245, _S226);



    float _S247 = f_0.y;

#line 4765
    thread WeightedProbe_0 _S248 = _S229;

#line 4765
    thread WeightedProbe_0 _S249 = _S235;

#line 4765
    WeightedProbe_0 _S250 = lerp_probe_0(&_S248, &_S249, _S247);

#line 4765
    thread WeightedProbe_0 _S251 = _S241;

#line 4765
    thread WeightedProbe_0 _S252 = _S246;

#line 4765
    WeightedProbe_0 _S253 = lerp_probe_0(&_S251, &_S252, _S247);

    float _S254 = f_0.z;

#line 4767
    thread WeightedProbe_0 _S255 = _S250;

#line 4767
    thread WeightedProbe_0 _S256 = _S253;

#line 4767
    WeightedProbe_0 _S257 = lerp_probe_0(&_S255, &_S256, _S254);

    float4 basis_7 = float4(normal_12, 1.0f);
    return max(float3(dot(_S257.sh_0.sh_r_0, basis_7), dot(_S257.sh_0.sh_g_0, basis_7), dot(_S257.sh_0.sh_b_0, basis_7)) / float3(_S257.weight_3) , _S211);
}


#line 4857
float3 probe_irradiance_0(float3 world_position_14, float3 normal_13, KernelContext_0 thread* kernelContext_36)
{

#line 4865
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_36->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_36->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_36->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_36->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 4867
    float3 _S258 = probe_level_irradiance_0(level_4, world_position_14, normal_13, kernelContext_36);


    if(share_0 >= 1.0f)
    {

#line 4871
        return _S258;
    }

#line 4871
    float3 _S259 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_13, kernelContext_36);

    return _S259 * float3((1.0f - share_0))  + _S258 * float3(share_0) ;
}


#line 5319
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 5319
    float3 _S260 = float3(visibility_4) ;

#line 5325
    return min(float3(1.0f) , max(_S260, ((_S260 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S260 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S260));
}


#line 1149
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_12)
{
    return float3(material_12->emissive_r_0, material_12->emissive_g_0, material_12->emissive_b_0);
}


#line 2969
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S261 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2977
    float kernel_0 = 0.0001984127011383f;

#line 2977
    int term_0 = int(6);

    for(;;)
    {

#line 2979
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2979
            break;
        }
        float _S262 = kernel_0 * _S261 + FOG_KERNEL_0[term_0];

#line 2979
        int term_1 = term_0 - int(1);

#line 2979
        kernel_0 = _S262;

#line 2979
        term_0 = term_1;

#line 2979
    }

#line 2986
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2996
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S263 = - d_0;

#line 3000
        float series_0 = 0.00833333376795053f;

#line 3000
        int term_2 = int(3);

        for(;;)
        {

#line 3002
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 3002
                break;
            }
            float _S264 = series_0 * _S263 + FOG_RATIO_KERNEL_0[term_2];

#line 3002
            int term_3 = term_2 - int(1);

#line 3002
            series_0 = _S264;

#line 3002
            term_2 = term_3;

#line 3002
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 3030
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 3041
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 3049
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4914
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4914
struct pixelInput_0
{
    float3 world_position_15 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_1 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_3 [[user(CRCBL_COLOR)]];
    [[flat]] uint material_13 [[user(CRCBL_MATERIAL)]];
    float2 uv_5 [[user(CRCBL_UV)]];
    float4 clip_position_1 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_1 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_1 [[user(CRCBL_WORLD_TANGENT)]];
    [[flat]] uint frame_4 [[user(CRCBL_FRAME)]];
};


#line 5361
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S265 [[stage_in]], bool front_facing_1 [[front_facing]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_3 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_3 [[texture(9)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 5361
    thread KernelContext_0 kernelContext_37;

#line 5361
    (&kernelContext_37)->draw_0 = draw_3;

#line 5361
    (&kernelContext_37)->visible_instances_0 = visible_instances_3;

#line 5361
    (&kernelContext_37)->instances_0 = instances_3;

#line 5361
    (&kernelContext_37)->meshes_0 = meshes_3;

#line 5361
    (&kernelContext_37)->frame_0 = frame_5;

#line 5361
    (&kernelContext_37)->vertices_0 = vertices_3;

#line 5361
    (&kernelContext_37)->ambient_occlusion_0 = ambient_occlusion_3;

#line 5361
    (&kernelContext_37)->materials_0 = materials_3;

#line 5361
    (&kernelContext_37)->base_color_textures_0 = base_color_textures_3;

#line 5361
    (&kernelContext_37)->base_color_sampler_0 = base_color_sampler_3;

#line 5361
    (&kernelContext_37)->normal_textures_0 = normal_textures_3;

#line 5361
    (&kernelContext_37)->mro_textures_0 = mro_textures_3;

#line 5361
    (&kernelContext_37)->emissive_textures_0 = emissive_textures_3;

#line 5361
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_3;

#line 5361
    (&kernelContext_37)->specular_dfg_0 = specular_dfg_3;

#line 5361
    (&kernelContext_37)->lights_0 = lights_3;

#line 5361
    (&kernelContext_37)->ltc_matrix_0 = ltc_matrix_3;

#line 5361
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_3;

#line 5361
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_3;

#line 5361
    (&kernelContext_37)->contact_shadow_0 = contact_shadow_3;

#line 5361
    (&kernelContext_37)->probes_0 = probes_3;

#line 5361
    (&kernelContext_37)->probe_visibility_0 = probe_visibility_3;

#line 5373
    float3 vertex_normal_0 = normalize(_S265.world_normal_1);

#line 5378
    float2 motion_1 = motion_vector_0(_S265.clip_position_1, _S265.previous_clip_position_1);

#line 5394
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 5396
        float4 _S266 = occlusion_at_0(position_5.xy, &kernelContext_37);



        (&bent_0)->lit_0 = float4(_S266.yzw, 1.0f);


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

#line 5450
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5450
        float4 _S267 = occlusion_at_0(position_5.xy, &kernelContext_37);


        float value_1 = _S267.x;

#line 5452
        thread FragmentOutput_0 occlusion_1;

#line 5461
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S265.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 5478
    thread GpuMaterial_natural_0 _S268 = (&kernelContext_37)->materials_0[_S265.material_13];

#line 5478
    float2 uv_6;

#line 5503
    if(((&_S268)->tiling_0) == 1U)
    {

#line 5503
        uv_6 = physical_tile_uv_0(_S265.world_position_15, vertex_normal_0, (&_S268)->tile_metres_0);

#line 5503
    }
    else
    {

#line 5503
        uv_6 = _S265.uv_5;

#line 5503
    }

#line 5503
    float4 _S269 = base_color_texel_0(&_S268, uv_6, &kernelContext_37);

#line 5525
    float4 albedo_1 = _S265.color_3 * float4((&_S268)->base_color_0)  * _S269;

#line 5525
    bool _S270 = alpha_masked_0(&_S268, albedo_1.w);

#line 5539
    if(_S270)
    {
        discard_fragment();

#line 5539
    }

#line 5539
    float3 _S271 = double_sided_normal_0(&_S268, vertex_normal_0, front_facing_1);

#line 5539
    uint _S272 = normal_layer_0(&_S268);

#line 5539
    thread VertexOutput_0 _S273;

#line 5539
    (&_S273)->position_3 = position_5;

#line 5539
    (&_S273)->world_position_1 = _S265.world_position_15;

#line 5539
    (&_S273)->world_normal_0 = _S265.world_normal_1;

#line 5539
    (&_S273)->color_2 = _S265.color_3;

#line 5539
    (&_S273)->material_6 = _S265.material_13;

#line 5539
    (&_S273)->uv_1 = _S265.uv_5;

#line 5539
    (&_S273)->clip_position_0 = _S265.clip_position_1;

#line 5539
    (&_S273)->previous_clip_position_0 = _S265.previous_clip_position_1;

#line 5539
    (&_S273)->world_tangent_0 = _S265.world_tangent_1;

#line 5539
    (&_S273)->frame_3 = _S265.frame_4;

#line 5539
    float3 _S274 = shading_normal_of_0(_S272, (&_S268)->normal_scale_0, &_S273, _S271, uv_6, &kernelContext_37);

#line 5558
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5560
        float3 _S275 = float3(0.5f) ;

#line 5572
        (&normals_0)->lit_0 = float4(_S274 * _S275 + _S275, 1.0f);

#line 5578
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_37)->frame_0->camera_position_0.xyz - _S265.world_position_15);



    float3 _S276 = geometric_normal_of_0(_S265.world_position_15, _S271);

#line 5587
    float4 _S277 = mro_texel_0(&_S268, uv_6, &kernelContext_37);

#line 5587
    float4 _S278 = emissive_texel_0(&_S268, uv_6, &kernelContext_37);

#line 5587
    float _S279 = metallic_of_0(&_S268, _S277);

#line 5618
    float roughness_2 = clamp((&_S268)->roughness_0 * _S277.y, 0.04500000178813934f, 1.0f);
    float alpha_1 = roughness_2 * roughness_2;

#line 5652
    float _S280 = saturate(alpha_1 * alpha_1 + specular_aa_kernel_0(_S274));

#line 5658
    float3 _S281 = albedo_1.xyz;

#line 5658
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S281, float3(_S279) );
    float3 diffuse_albedo_0 = _S281 * float3((1.0f - _S279)) ;

#line 5665
    float _S282 = max(dot(_S274, to_eye_1), 0.00009999999747379f);

#line 5675
    float2 _S283 = position_5.xy;

#line 5675
    uint _S284 = froxel_of_0(_S283, (((float4(_S265.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_37);

#line 5675
    uint base_2 = _S284 * 17U;

#line 5680
    uint _S285 = min((&kernelContext_37)->cluster_lights_0[base_2], 16U);

#line 5680
    TableTap_0 _S286 = table_tap_0(_S282, roughness_2, &kernelContext_37);

#line 5680
    thread TableTap_0 _S287 = _S286;

#line 5680
    float2 _S288 = dfg_at_0(&_S287, &kernelContext_37);

#line 5689
    float _S289 = _S288.x;

#line 5689
    float _S290 = _S288.y;

#line 5689
    float3 _S291 = f0_2 * float3(_S289)  + float3(_S290) ;

#line 5695
    float3 _S292 = float3(0.0f, 0.0f, 0.0f);

#line 5695
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5695
    uint slot_0 = 0U;

#line 5695
    float3 direct_0 = _S292;

#line 5695
    float3 gloss_0 = _S292;

#line 5705
    for(;;)
    {

#line 5705
        if(slot_0 < _S285)
        {
        }
        else
        {

#line 5705
            break;
        }

#line 5705
        thread GpuLight_natural_0 _S293 = (&kernelContext_37)->lights_0[(&kernelContext_37)->cluster_lights_0[base_2 + 1U + slot_0]];

#line 5705
        uint _S294 = (&_S293)->kind_0;

#line 5714
        bool _S295 = ((&_S293)->kind_0) == 0U;

#line 5714
        float3 to_light_7;

#line 5714
        float reach_2;

#line 5714
        if(_S295)
        {

#line 5714
            to_light_7 = normalize((float4((&_S293)->direction_0) ).xyz);

#line 5714
            reach_2 = 1.0f;

#line 5714
        }
        else
        {


            if(_S294 == 3U)
            {

#line 5719
                float4 _S296 = float4((&_S293)->position_0) ;

#line 5727
                float3 offset_2 = _S296.xyz - _S265.world_position_15;
                float distance_3 = length(offset_2);

                float _S297 = range_window_0(distance_3, _S296.w);

#line 5730
                to_light_7 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5730
                reach_2 = _S297;

#line 5719
            }
            else
            {

#line 5719
                float4 _S298 = float4((&_S293)->position_0) ;

#line 5734
                float3 offset_3 = _S298.xyz - _S265.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_8 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S298.w);
                if(_S294 == 2U)
                {

#line 5738
                    float4 _S299 = float4((&_S293)->direction_0) ;

#line 5738
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S299.xyz, _S299.w, (&_S293)->cos_inner_0);

#line 5738
                }
                else
                {

#line 5738
                    reach_2 = reach_3;

#line 5738
                }

#line 5738
                to_light_7 = to_light_8;

#line 5719
            }

#line 5714
        }

#line 5747
        float n_dot_l_5 = dot(_S274, to_light_7);

#line 5747
        float3 specular_0;

#line 5747
        float diffuse_0;


        if(_S294 == 3U)
        {

#line 5760
            thread array<float3, int(4)> corners_2;

#line 5760
            rect_corners_0(&_S293, _S265.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S274, to_eye_1, _S282);

#line 5762
            thread array<float3, int(4)> _S300 = corners_2;

#line 5762
            float _S301 = ltc_irradiance_0(to_local_0, &_S300);

#line 5762
            thread TableTap_0 _S302 = _S286;

#line 5762
            float4 _S303 = ltc_at_0(&_S302, &kernelContext_37);

            matrix<float,int(3),int(3)>  _S304 = (((to_local_0) * (ltc_transform_0(_S303))));

#line 5764
            thread array<float3, int(4)> _S305 = corners_2;

#line 5764
            float _S306 = ltc_irradiance_0(_S304, &_S305);
            float3 _S307 = float3(_S306)  * _S291;

#line 5765
            diffuse_0 = _S301;

#line 5765
            specular_0 = _S307;

#line 5750
        }
        else
        {

#line 5770
            float _S308 = max(n_dot_l_5, 0.0f);

#line 5777
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5785
            float3 specular_1 = ggx_lobe_0(_S280, f0_2, _S308, _S282, max(dot(_S274, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S308) ;

#line 5785
            diffuse_0 = _S308;

#line 5785
            specular_0 = specular_1;

#line 5750
        }

#line 5750
        float3 specular_2;

#line 5793
        if((((&_S293)->flags_3) & 1U) != 0U)
        {

#line 5793
            specular_2 = _S292;

#line 5793
        }
        else
        {

#line 5793
            specular_2 = specular_0;

#line 5793
        }

#line 5793
        float reach_4;

#line 5811
        if(_S295)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 5814
            float _S309 = sun_visibility_0(_S265.world_position_15, to_light_7, n_dot_l_5, _S276, _S283, &sun_cascade_0, &sun_fade_0, &kernelContext_37);

#line 5814
            float _S310 = contact_at_0(_S283, &kernelContext_37);

#line 5823
            float _S311 = _S309 * _S310;

#line 5823
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 5823
            reach_4 = _S311;

#line 5811
        }
        else
        {

#line 5828
            if(_S294 == 1U)
            {

#line 5828
                uint _S312 = (&_S293)->shadow_tile_0;

#line 5840
                if(((&_S293)->shadow_tile_0) <= 8U)
                {

#line 5840
                    float _S313 = point_visibility_0(&_S293, _S312, _S265.world_position_15, to_light_7, n_dot_l_5, _S276, _S283, &kernelContext_37);

#line 5840
                    reach_4 = reach_2 * _S313;

#line 5840
                }
                else
                {

#line 5840
                    reach_4 = reach_2;

#line 5840
                }

#line 5828
            }
            else
            {

#line 5828
                uint _S314 = (&_S293)->shadow_tile_0;

#line 5846
                if(((&_S293)->shadow_tile_0) < 14U)
                {

#line 5846
                    float _S315 = spot_visibility_0(&_S293, _S314, _S265.world_position_15, to_light_7, n_dot_l_5, _S276, _S283, &kernelContext_37);

#line 5846
                    reach_4 = reach_2 * _S315;

#line 5846
                }
                else
                {

#line 5846
                    reach_4 = reach_2;

#line 5846
                }

#line 5828
            }

#line 5811
        }

#line 5854
        float3 _S316 = (float4((&_S293)->color_0) ).xyz;

#line 5854
        float3 direct_1 = direct_0 + _S316 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S316 * (specular_2 * float3(reach_4) );

#line 5705
        slot_0 = slot_0 + 1U;

#line 5705
        direct_0 = direct_1;

#line 5705
        gloss_0 = gloss_1;

#line 5705
    }

#line 5869
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S289 + _S290);

#line 5869
    float4 _S317 = occlusion_at_0(_S283, &kernelContext_37);

#line 5888
    float occluded_0 = _S317.x;

#line 5897
    float3 bent_normal_0 = bent_normal_at_0(_S317, _S274);

#line 5920
    float3 _S318 = frame_5->ambient_0.xyz;

#line 5920
    float3 _S319 = sky_irradiance_0(bent_normal_0, &kernelContext_37);

#line 5920
    float3 _S320 = _S318 + _S319;

#line 5920
    float3 _S321 = probe_irradiance_0(_S265.world_position_15, bent_normal_0, &kernelContext_37);

#line 5976
    float3 lit_1 = diffuse_albedo_0 * ((_S320 + _S321) * (multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) * float3(_S277.x) ) + direct_0) + gloss_2;

#line 5976
    float3 _S322 = emissive_of_0(&_S268);

#line 6018
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_37)->frame_0->fog_params_0.x, (&kernelContext_37)->frame_0->fog_params_0.y, (&kernelContext_37)->frame_0->camera_position_0.y - (&kernelContext_37)->frame_0->fog_params_0.z, _S265.world_position_15.y - (&kernelContext_37)->frame_0->fog_params_0.z, length((&kernelContext_37)->frame_0->camera_position_0.xyz - _S265.world_position_15)));
    float3 lit_2 = (lit_1 + _S322 * _S278.xyz) * float3(fog_survives_0)  + (&kernelContext_37)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;

#line 6031
    (&output_2)->lit_0 = float4(lit_2, 1.0f);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 6044
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, 1.0f);

#line 6053
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 6044
    }

#line 6055
    return output_2;
}


#line 6055
struct pixelInput_1
{
    float3 world_position_16 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_2 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_4 [[user(CRCBL_COLOR)]];
    [[flat]] uint material_14 [[user(CRCBL_MATERIAL)]];
    float2 uv_7 [[user(CRCBL_UV)]];
    float4 clip_position_2 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_2 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_2 [[user(CRCBL_WORLD_TANGENT)]];
    [[flat]] uint frame_6 [[user(CRCBL_FRAME)]];
};


#line 6088
[[fragment]] void depthMaskedFragmentMain(pixelInput_1 _S323 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_4 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_4 [[texture(9)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 6088
    thread KernelContext_0 kernelContext_38;

#line 6088
    (&kernelContext_38)->draw_0 = draw_4;

#line 6088
    (&kernelContext_38)->visible_instances_0 = visible_instances_4;

#line 6088
    (&kernelContext_38)->instances_0 = instances_4;

#line 6088
    (&kernelContext_38)->meshes_0 = meshes_4;

#line 6088
    (&kernelContext_38)->frame_0 = frame_7;

#line 6088
    (&kernelContext_38)->vertices_0 = vertices_4;

#line 6088
    (&kernelContext_38)->ambient_occlusion_0 = ambient_occlusion_4;

#line 6088
    (&kernelContext_38)->materials_0 = materials_4;

#line 6088
    (&kernelContext_38)->base_color_textures_0 = base_color_textures_4;

#line 6088
    (&kernelContext_38)->base_color_sampler_0 = base_color_sampler_4;

#line 6088
    (&kernelContext_38)->normal_textures_0 = normal_textures_4;

#line 6088
    (&kernelContext_38)->mro_textures_0 = mro_textures_4;

#line 6088
    (&kernelContext_38)->emissive_textures_0 = emissive_textures_4;

#line 6088
    (&kernelContext_38)->cluster_lights_0 = cluster_lights_4;

#line 6088
    (&kernelContext_38)->specular_dfg_0 = specular_dfg_4;

#line 6088
    (&kernelContext_38)->lights_0 = lights_4;

#line 6088
    (&kernelContext_38)->ltc_matrix_0 = ltc_matrix_4;

#line 6088
    (&kernelContext_38)->shadow_atlas_0 = shadow_atlas_4;

#line 6088
    (&kernelContext_38)->shadow_sampler_0 = shadow_sampler_4;

#line 6088
    (&kernelContext_38)->contact_shadow_0 = contact_shadow_4;

#line 6088
    (&kernelContext_38)->probes_0 = probes_4;

#line 6088
    (&kernelContext_38)->probe_visibility_0 = probe_visibility_4;

#line 6088
    thread GpuMaterial_natural_0 _S324 = materials_4[_S323.material_14];

#line 6088
    float2 uv_8;

#line 6097
    if(((&_S324)->tiling_0) == 1U)
    {

#line 6097
        uv_8 = physical_tile_uv_0(_S323.world_position_16, normalize(_S323.world_normal_2), (&_S324)->tile_metres_0);

#line 6097
    }
    else
    {

#line 6097
        uv_8 = _S323.uv_7;

#line 6097
    }

#line 6097
    float4 _S325 = base_color_texel_0(&_S324, uv_8, &kernelContext_38);

#line 6097
    bool _S326 = alpha_masked_0(&_S324, _S323.color_4.w * (float4((&_S324)->base_color_0) ).w * _S325.w);

#line 6106
    if(_S326)
    {
        discard_fragment();

#line 6106
    }



    return;
}


#line 6140
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_14 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 6140
struct pixelInput_2
{
    float3 world_position_17 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_3 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_5 [[user(CRCBL_COLOR)]];
    [[flat]] uint material_15 [[user(CRCBL_MATERIAL)]];
    float2 uv_9 [[user(CRCBL_UV)]];
    float4 clip_position_3 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_3 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_3 [[user(CRCBL_WORLD_TANGENT)]];
    [[flat]] uint frame_8 [[user(CRCBL_FRAME)]];
};


#line 6183
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_2 _S327 [[stage_in]], bool front_facing_2 [[front_facing]], float4 position_7 [[position]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_5 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_5 [[texture(9)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 6183
    thread KernelContext_0 kernelContext_39;

#line 6183
    (&kernelContext_39)->draw_0 = draw_5;

#line 6183
    (&kernelContext_39)->visible_instances_0 = visible_instances_5;

#line 6183
    (&kernelContext_39)->instances_0 = instances_5;

#line 6183
    (&kernelContext_39)->meshes_0 = meshes_5;

#line 6183
    (&kernelContext_39)->frame_0 = frame_9;

#line 6183
    (&kernelContext_39)->vertices_0 = vertices_5;

#line 6183
    (&kernelContext_39)->ambient_occlusion_0 = ambient_occlusion_5;

#line 6183
    (&kernelContext_39)->materials_0 = materials_5;

#line 6183
    (&kernelContext_39)->base_color_textures_0 = base_color_textures_5;

#line 6183
    (&kernelContext_39)->base_color_sampler_0 = base_color_sampler_5;

#line 6183
    (&kernelContext_39)->normal_textures_0 = normal_textures_5;

#line 6183
    (&kernelContext_39)->mro_textures_0 = mro_textures_5;

#line 6183
    (&kernelContext_39)->emissive_textures_0 = emissive_textures_5;

#line 6183
    (&kernelContext_39)->cluster_lights_0 = cluster_lights_5;

#line 6183
    (&kernelContext_39)->specular_dfg_0 = specular_dfg_5;

#line 6183
    (&kernelContext_39)->lights_0 = lights_5;

#line 6183
    (&kernelContext_39)->ltc_matrix_0 = ltc_matrix_5;

#line 6183
    (&kernelContext_39)->shadow_atlas_0 = shadow_atlas_5;

#line 6183
    (&kernelContext_39)->shadow_sampler_0 = shadow_sampler_5;

#line 6183
    (&kernelContext_39)->contact_shadow_0 = contact_shadow_5;

#line 6183
    (&kernelContext_39)->probes_0 = probes_5;

#line 6183
    (&kernelContext_39)->probe_visibility_0 = probe_visibility_5;

#line 6188
    float3 vertex_normal_1 = normalize(_S327.world_normal_3);

#line 6188
    thread GpuMaterial_natural_0 _S328 = materials_5[_S327.material_15];

#line 6188
    float2 uv_10;

#line 6195
    if(((&_S328)->tiling_0) == 1U)
    {

#line 6195
        uv_10 = physical_tile_uv_0(_S327.world_position_17, vertex_normal_1, (&_S328)->tile_metres_0);

#line 6195
    }
    else
    {

#line 6195
        uv_10 = _S327.uv_9;

#line 6195
    }

#line 6195
    float4 _S329 = base_color_texel_0(&_S328, uv_10, &kernelContext_39);

#line 6200
    float4 albedo_3 = _S327.color_5 * float4((&_S328)->base_color_0)  * _S329;

#line 6200
    bool _S330 = alpha_masked_0(&_S328, albedo_3.w);

#line 6206
    if(_S330)
    {
        discard_fragment();

#line 6206
    }

#line 6211
    thread RsmOutput_0 written_0;

#line 6221
    float3 _S331 = albedo_3.xyz;

#line 6221
    float4 _S332 = mro_texel_0(&_S328, uv_10, &kernelContext_39);

#line 6221
    float _S333 = metallic_of_0(&_S328, _S332);

#line 6220
    (&written_0)->albedo_2 = float4(_S331 * float3((1.0f - _S333)) , 1.0f);

#line 6220
    float3 _S334 = double_sided_normal_0(&_S328, vertex_normal_1, front_facing_2);

#line 6220
    float3 _S335 = float3(0.5f) ;

#line 6227
    (&written_0)->normal_14 = float4(_S334 * _S335 + _S335, 1.0f);

    (&written_0)->world_0 = float4(_S327.world_position_17, 1.0f);
    return written_0;
}


#line 6230
struct vertexMain_Result_0
{
    float4 position_8 [[position]];
    float3 world_position_18 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_4 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_6 [[user(CRCBL_COLOR)]];
    uint material_16 [[user(CRCBL_MATERIAL)]];
    float2 uv_11 [[user(CRCBL_UV)]];
    float4 clip_position_4 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_4 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_4 [[user(CRCBL_WORLD_TANGENT)]];
    uint frame_10 [[user(CRCBL_FRAME)]];
};


#line 6230
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], uint base_instance_1 [[base_instance]], DrawConstants_0 constant* draw_6 [[buffer(3)]], uint device* visible_instances_6 [[buffer(5)]], GpuInstance_natural_0 device* instances_6 [[buffer(2)]], GpuMesh_0 device* meshes_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_11 [[buffer(0)]], uint device* vertices_6 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_6 [[texture(2)]], GpuMaterial_natural_0 device* materials_6 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_6 [[texture(0)]], sampler base_color_sampler_6 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_6 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_6 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_6 [[texture(9)]], uint device* cluster_lights_6 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_6 [[texture(3)]], GpuLight_natural_0 device* lights_6 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_6 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_6 [[texture(6)]], GpuProbe_natural_0 device* probes_6 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_6 [[texture(7)]])
{

#line 6230
    thread KernelContext_0 kernelContext_40;

#line 6230
    (&kernelContext_40)->draw_0 = draw_6;

#line 6230
    (&kernelContext_40)->visible_instances_0 = visible_instances_6;

#line 6230
    (&kernelContext_40)->instances_0 = instances_6;

#line 6230
    (&kernelContext_40)->meshes_0 = meshes_6;

#line 6230
    (&kernelContext_40)->frame_0 = frame_11;

#line 6230
    (&kernelContext_40)->vertices_0 = vertices_6;

#line 6230
    (&kernelContext_40)->ambient_occlusion_0 = ambient_occlusion_6;

#line 6230
    (&kernelContext_40)->materials_0 = materials_6;

#line 6230
    (&kernelContext_40)->base_color_textures_0 = base_color_textures_6;

#line 6230
    (&kernelContext_40)->base_color_sampler_0 = base_color_sampler_6;

#line 6230
    (&kernelContext_40)->normal_textures_0 = normal_textures_6;

#line 6230
    (&kernelContext_40)->mro_textures_0 = mro_textures_6;

#line 6230
    (&kernelContext_40)->emissive_textures_0 = emissive_textures_6;

#line 6230
    (&kernelContext_40)->cluster_lights_0 = cluster_lights_6;

#line 6230
    (&kernelContext_40)->specular_dfg_0 = specular_dfg_6;

#line 6230
    (&kernelContext_40)->lights_0 = lights_6;

#line 6230
    (&kernelContext_40)->ltc_matrix_0 = ltc_matrix_6;

#line 6230
    (&kernelContext_40)->shadow_atlas_0 = shadow_atlas_6;

#line 6230
    (&kernelContext_40)->shadow_sampler_0 = shadow_sampler_6;

#line 6230
    (&kernelContext_40)->contact_shadow_0 = contact_shadow_6;

#line 6230
    (&kernelContext_40)->probes_0 = probes_6;

#line 6230
    (&kernelContext_40)->probe_visibility_0 = probe_visibility_6;

#line 6230
    GpuInstance_natural_0 device* _S336 = instances_6+visible_instances_6[visible_instances_6[draw_6->start_at_0] + (instance_id_1 - base_instance_1)];

#line 2199
    GpuMesh_0 mesh_3 = meshes_6[draw_6->mesh_0];

#line 2207
    bool _S337 = ((_S336->flags_0) & 2U) != 0U;

#line 2207
    uint base_vertex_3;
    if(_S337)
    {

#line 2208
        base_vertex_3 = _S336->base_vertex_0;

#line 2208
    }
    else
    {

#line 2208
        base_vertex_3 = mesh_3.base_vertex_1;

#line 2208
    }

#line 2208
    MeshVertex_0 _S338 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_40);

#line 2208
    uint previous_base_0;

#line 2221
    if(_S337)
    {

#line 2221
        previous_base_0 = _S336->previous_base_vertex_0;

#line 2221
    }
    else
    {

#line 2221
        previous_base_0 = base_vertex_3;

#line 2221
    }

#line 2221
    float3 _S339 = load_position_0(index_8 + previous_base_0, &kernelContext_40);

#line 2221
    matrix<float,int(4),int(4)>  _S340 = matrix<float,int(4),int(4)> (_S336->transform_0.data_0[int(0)][int(0)], _S336->transform_0.data_0[int(1)][int(0)], _S336->transform_0.data_0[int(2)][int(0)], _S336->transform_0.data_0[int(3)][int(0)], _S336->transform_0.data_0[int(0)][int(1)], _S336->transform_0.data_0[int(1)][int(1)], _S336->transform_0.data_0[int(2)][int(1)], _S336->transform_0.data_0[int(3)][int(1)], _S336->transform_0.data_0[int(0)][int(2)], _S336->transform_0.data_0[int(1)][int(2)], _S336->transform_0.data_0[int(2)][int(2)], _S336->transform_0.data_0[int(3)][int(2)], _S336->transform_0.data_0[int(0)][int(3)], _S336->transform_0.data_0[int(1)][int(3)], _S336->transform_0.data_0[int(2)][int(3)], _S336->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S338.position_1, 1.0f)) * (_S340)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 2235
    matrix<float,int(3),int(3)>  _S341 = matrix<float,int(3),int(3)> (_S340[int(0)].xyz, _S340[int(1)].xyz, _S340[int(2)].xyz);

#line 2235
    (&output_3)->world_normal_0 = (((_S338.basis_1.normal_0) * (normal_basis_0(_S341))));

#line 2241
    (&output_3)->world_tangent_0 = (((_S338.basis_1.tangent_1) * (_S341)));

#line 2241
    thread TangentFrame_0 _S342 = _S338.basis_1;

#line 2241
    uint _S343 = frame_word_0(mesh_3.flags_1, &_S342);
    (&output_3)->frame_3 = _S343;

#line 2242
    float4 _S344;

#line 2249
    if(((&kernelContext_40)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 2249
        _S344 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 2249
    }
    else
    {

#line 2249
        _S344 = _S338.color_1;

#line 2249
    }

#line 2248
    (&output_3)->color_2 = _S344;

#line 2255
    (&output_3)->material_6 = _S336->material_0;
    (&output_3)->uv_1 = _S338.uv0_0;

#line 2262
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S339, 1.0f)) * (matrix<float,int(4),int(4)> (_S336->previous_transform_0.data_0[int(0)][int(0)], _S336->previous_transform_0.data_0[int(1)][int(0)], _S336->previous_transform_0.data_0[int(2)][int(0)], _S336->previous_transform_0.data_0[int(3)][int(0)], _S336->previous_transform_0.data_0[int(0)][int(1)], _S336->previous_transform_0.data_0[int(1)][int(1)], _S336->previous_transform_0.data_0[int(2)][int(1)], _S336->previous_transform_0.data_0[int(3)][int(1)], _S336->previous_transform_0.data_0[int(0)][int(2)], _S336->previous_transform_0.data_0[int(1)][int(2)], _S336->previous_transform_0.data_0[int(2)][int(2)], _S336->previous_transform_0.data_0[int(3)][int(2)], _S336->previous_transform_0.data_0[int(0)][int(3)], _S336->previous_transform_0.data_0[int(1)][int(3)], _S336->previous_transform_0.data_0[int(2)][int(3)], _S336->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S345 = output_3;

#line 2266
    thread vertexMain_Result_0 _S346;

#line 2266
    (&_S346)->position_8 = _S345.position_3;

#line 2266
    (&_S346)->world_position_18 = _S345.world_position_1;

#line 2266
    (&_S346)->world_normal_4 = _S345.world_normal_0;

#line 2266
    (&_S346)->color_6 = _S345.color_2;

#line 2266
    (&_S346)->material_16 = _S345.material_6;

#line 2266
    (&_S346)->uv_11 = _S345.uv_1;

#line 2266
    (&_S346)->clip_position_4 = _S345.clip_position_0;

#line 2266
    (&_S346)->previous_clip_position_4 = _S345.previous_clip_position_0;

#line 2266
    (&_S346)->world_tangent_4 = _S345.world_tangent_0;

#line 2266
    (&_S346)->frame_10 = _S345.frame_3;

#line 2266
    return _S346;
}

