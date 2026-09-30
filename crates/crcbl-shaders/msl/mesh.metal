#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 3089 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f } };

#line 3084
constant array<float, int(8)> FOG_KERNEL_0 = { { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f } };

#line 4086
constant array<float3, int(2)> CASCADE_TINTS_0 = { { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) } };

#line 3569
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) } };

#line 3356
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 3416
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

#line 3431
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) } };

#line 3459
constant array<uint, int(16)> SHADOW_DITHER_0 = { { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U } };

#line 1510
struct DrawSource_0
{
    uint run_0;
    uint mesh_0;
};


#line 1394
struct DrawConstants_0
{
    uint start_at_0;
    uint mesh_1;
    uint mesh_at_0;
    uint pad1_0;
};


#line 1471
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1471
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_transform_0;
    uint mesh_2;
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


#line 1429
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1429
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


#line 1546
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
    float specular_f0_r_0;
    float specular_f0_g_0;
    float specular_f0_b_0;
    float specular_f90_0;
};


#line 1698
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


#line 1778
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 1970
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


#line 1519
DrawSource_0 draw_source_0(uint draw_index_0, KernelContext_0 thread* kernelContext_0)
{
    thread DrawSource_0 source_0;

#line 1526
    (&source_0)->run_0 = kernelContext_0->visible_instances_0[kernelContext_0->draw_0->start_at_0];
    (&source_0)->mesh_0 = kernelContext_0->draw_0->mesh_1;

    return source_0;
}


#line 1448
float3 load_position_0(uint at_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_0 = at_0 * 3U;
    return float3((as_type<float>((kernelContext_1->vertices_0[word_0]))), (as_type<float>((kernelContext_1->vertices_0[word_0 + 1U]))), (as_type<float>((kernelContext_1->vertices_0[word_0 + 2U]))));
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


#line 1459
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_2)
{
    uint word_3 = kernelContext_2->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1462
    float3 _S6 = load_position_0(at_1, kernelContext_2);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_2->vertices_0[word_3], kernelContext_2->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_2->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_2->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 2132
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 2265
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 2265
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 2267
        word_4 = 1U;

#line 2267
    }
    else
    {

#line 2267
        word_4 = 0U;

#line 2267
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 2271
        word_4 = word_4 | 2U;

#line 2271
    }

#line 2270
    return word_4;
}


#line 2270
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 2388
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], uint base_instance_0 [[base_instance]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_1 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_1 [[texture(9)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 2389
    thread KernelContext_0 kernelContext_3;

#line 2389
    (&kernelContext_3)->draw_0 = draw_1;

#line 2389
    (&kernelContext_3)->visible_instances_0 = visible_instances_1;

#line 2389
    (&kernelContext_3)->instances_0 = instances_1;

#line 2389
    (&kernelContext_3)->meshes_0 = meshes_1;

#line 2389
    (&kernelContext_3)->frame_0 = frame_1;

#line 2389
    (&kernelContext_3)->vertices_0 = vertices_1;

#line 2389
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2389
    (&kernelContext_3)->materials_0 = materials_1;

#line 2389
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_1;

#line 2389
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_1;

#line 2389
    (&kernelContext_3)->normal_textures_0 = normal_textures_1;

#line 2389
    (&kernelContext_3)->mro_textures_0 = mro_textures_1;

#line 2389
    (&kernelContext_3)->emissive_textures_0 = emissive_textures_1;

#line 2389
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_1;

#line 2389
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_1;

#line 2389
    (&kernelContext_3)->lights_0 = lights_1;

#line 2389
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_1;

#line 2389
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_1;

#line 2389
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_1;

#line 2389
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_1;

#line 2389
    (&kernelContext_3)->probes_0 = probes_1;

#line 2389
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_1;

#line 2389
    uint _S7 = instance_id_0 - base_instance_0;

#line 2389
    DrawSource_0 _S8 = draw_source_0(0U, &kernelContext_3);

#line 2389
    GpuInstance_natural_0 device* _S9 = (&kernelContext_3)->instances_0+(&kernelContext_3)->visible_instances_0[_S8.run_0 + _S7];



    GpuMesh_0 mesh_3 = (&kernelContext_3)->meshes_0[_S8.mesh_0];

#line 2393
    uint base_vertex_2;

#line 2399
    if(((_S9->flags_0) & 2U) != 0U)
    {

#line 2399
        base_vertex_2 = _S9->base_vertex_0;

#line 2399
    }
    else
    {

#line 2399
        base_vertex_2 = mesh_3.base_vertex_1;

#line 2399
    }

#line 2399
    matrix<float,int(4),int(4)>  _S10 = matrix<float,int(4),int(4)> (_S9->transform_0.data_0[int(0)][int(0)], _S9->transform_0.data_0[int(1)][int(0)], _S9->transform_0.data_0[int(2)][int(0)], _S9->transform_0.data_0[int(3)][int(0)], _S9->transform_0.data_0[int(0)][int(1)], _S9->transform_0.data_0[int(1)][int(1)], _S9->transform_0.data_0[int(2)][int(1)], _S9->transform_0.data_0[int(3)][int(1)], _S9->transform_0.data_0[int(0)][int(2)], _S9->transform_0.data_0[int(1)][int(2)], _S9->transform_0.data_0[int(2)][int(2)], _S9->transform_0.data_0[int(3)][int(2)], _S9->transform_0.data_0[int(0)][int(3)], _S9->transform_0.data_0[int(1)][int(3)], _S9->transform_0.data_0[int(2)][int(3)], _S9->transform_0.data_0[int(3)][int(3)]);

#line 2399
    float3 _S11 = load_position_0(index_0 + base_vertex_2, &kernelContext_3);

#line 2399
    vertexOutput_0 _S12 = { ((((((float4(_S11, 1.0f)) * (_S10)))) * (matrix<float,int(4),int(4)> ((&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S12;
}


#line 2402
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 2423
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_2 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_2 [[texture(9)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 2423
    thread KernelContext_0 kernelContext_4;

#line 2423
    (&kernelContext_4)->draw_0 = draw_2;

#line 2423
    (&kernelContext_4)->visible_instances_0 = visible_instances_2;

#line 2423
    (&kernelContext_4)->instances_0 = instances_2;

#line 2423
    (&kernelContext_4)->meshes_0 = meshes_2;

#line 2423
    (&kernelContext_4)->frame_0 = frame_2;

#line 2423
    (&kernelContext_4)->vertices_0 = vertices_2;

#line 2423
    (&kernelContext_4)->ambient_occlusion_0 = ambient_occlusion_2;

#line 2423
    (&kernelContext_4)->materials_0 = materials_2;

#line 2423
    (&kernelContext_4)->base_color_textures_0 = base_color_textures_2;

#line 2423
    (&kernelContext_4)->base_color_sampler_0 = base_color_sampler_2;

#line 2423
    (&kernelContext_4)->normal_textures_0 = normal_textures_2;

#line 2423
    (&kernelContext_4)->mro_textures_0 = mro_textures_2;

#line 2423
    (&kernelContext_4)->emissive_textures_0 = emissive_textures_2;

#line 2423
    (&kernelContext_4)->cluster_lights_0 = cluster_lights_2;

#line 2423
    (&kernelContext_4)->specular_dfg_0 = specular_dfg_2;

#line 2423
    (&kernelContext_4)->lights_0 = lights_2;

#line 2423
    (&kernelContext_4)->ltc_matrix_0 = ltc_matrix_2;

#line 2423
    (&kernelContext_4)->shadow_atlas_0 = shadow_atlas_2;

#line 2423
    (&kernelContext_4)->shadow_sampler_0 = shadow_sampler_2;

#line 2423
    (&kernelContext_4)->contact_shadow_0 = contact_shadow_2;

#line 2423
    (&kernelContext_4)->probes_0 = probes_2;

#line 2423
    (&kernelContext_4)->probe_visibility_0 = probe_visibility_2;

#line 2423
    vertexOutput_1 _S13 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S13;
}


#line 5491
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S14 = previous_0.w;

#line 5493
    if(_S14 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S14) ) * float2(0.5f, -0.5f);
}


#line 5367
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_5)
{

#line 5367
    texture2d<float, access::sample> _S15 = kernelContext_5->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S15).get_width(0)),(*((&height_0)) = (_S15).get_height(0));

    int3 _S16 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 5373
    return ((kernelContext_5->ambient_occlusion_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z)));
}


#line 5101
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S17 = axis_0.x;

#line 5105
    float _S18 = axis_0.y;

#line 5105
    bool _S19;

#line 5105
    if(_S17 >= _S18)
    {

#line 5105
        _S19 = _S17 >= (axis_0.z);

#line 5105
    }
    else
    {

#line 5105
        _S19 = false;

#line 5105
    }

#line 5105
    float2 planar_0;

#line 5105
    if(_S19)
    {

#line 5105
        planar_0 = world_position_0.zy;

#line 5105
    }
    else
    {

        if(_S18 >= (axis_0.z))
        {

#line 5109
            planar_0 = world_position_0.xz;

#line 5109
        }
        else
        {

#line 5109
            planar_0 = world_position_0.xy;

#line 5109
        }

#line 5105
    }

#line 5117
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1116
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) & 65535U;
}


#line 1631
float4 base_color_texel_0(const GpuMaterial_natural_0 thread* material_2, float2 uv_0, KernelContext_0 thread* kernelContext_6)
{

#line 1631
    uint _S20 = base_color_layer_0(material_2);


    bool named_0 = _S20 != 65535U;

#line 1634
    uint _S21;

    if(named_0)
    {

#line 1636
        _S21 = _S20;

#line 1636
    }
    else
    {

#line 1636
        _S21 = 0U;

#line 1636
    }

#line 1636
    float3 _S22 = float3(uv_0, float(_S21));

#line 1635
    float4 texel_0 = ((kernelContext_6->base_color_textures_0).sample((kernelContext_6->base_color_sampler_0), ((_S22)).xy, uint(((_S22)).z)));

#line 1635
    float4 _S23;

    if(named_0)
    {

#line 1637
        _S23 = texel_0;

#line 1637
    }
    else
    {

#line 1637
        _S23 = float4(1.0f, 1.0f, 1.0f, 1.0f);

#line 1637
    }

#line 1637
    return _S23;
}


#line 1233
bool alpha_masked_0(const GpuMaterial_natural_0 thread* material_3, float alpha_0)
{

#line 1233
    bool _S24;

    if(((material_3->flags_2) & 1U) != 0U)
    {

#line 1235
        _S24 = alpha_0 < (material_3->alpha_cutoff_0);

#line 1235
    }
    else
    {

#line 1235
        _S24 = false;

#line 1235
    }

#line 1235
    return _S24;
}


#line 1268
float3 double_sided_normal_0(const GpuMaterial_natural_0 thread* material_4, float3 normal_2, bool front_facing_0)
{

#line 1268
    bool _S25;

    if(((material_4->flags_2) & 2U) != 0U)
    {

#line 1270
        _S25 = !front_facing_0;

#line 1270
    }
    else
    {

#line 1270
        _S25 = false;

#line 1270
    }

#line 1270
    float3 _S26;

#line 1270
    if(_S25)
    {

#line 1270
        _S26 = - normal_2;

#line 1270
    }
    else
    {

#line 1270
        _S26 = normal_2;

#line 1270
    }

#line 1270
    return _S26;
}


#line 1131
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_5)
{
    return (material_5->color_normal_pages_0) >> 16U;
}


#line 5138
float3 orthonormal_tangent_0(float3 normal_3)
{
    float _S27 = normal_3.z;

#line 5140
    float sign_z_0;

#line 5140
    if(_S27 >= 0.0f)
    {

#line 5140
        sign_z_0 = 1.0f;

#line 5140
    }
    else
    {

#line 5140
        sign_z_0 = -1.0f;

#line 5140
    }
    float a_0 = -1.0f / (sign_z_0 + _S27);
    float _S28 = normal_3.x;

#line 5142
    float _S29 = sign_z_0 * _S28;

#line 5142
    return float3(1.0f + _S29 * _S28 * a_0, _S29 * normal_3.y * a_0, - sign_z_0 * _S28);
}


#line 5192
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_4)
{
    float _S30 = duvdy_0.y;

#line 5194
    float _S31 = duvdx_0.y;

#line 5194
    float winding_0;
    if((duvdx_0.x * _S30 - duvdy_0.x * _S31) < 0.0f)
    {

#line 5195
        winding_0 = -1.0f;

#line 5195
    }
    else
    {

#line 5195
        winding_0 = 1.0f;

#line 5195
    }
    float3 tangent_2 = (float3(_S30)  * dpdx_0 - float3(_S31)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_4;

#line 5204
    float3 tangent_3 = tangent_2 - normal_4 * float3(dot(normal_4, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 5205
    float3 _S32;

#line 5214
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 5214
        _S32 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 5214
    }
    else
    {

#line 5214
        _S32 = orthonormal_tangent_0(normal_4);

#line 5214
    }

#line 5214
    (&basis_4)->tangent_1 = _S32;

    (&basis_4)->bitangent_0 = cross(normal_4, _S32);
    return basis_4;
}


#line 2149
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


#line 5274
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_5, float2 uv_2, KernelContext_0 thread* kernelContext_7)
{

#line 5286
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_2);
    float2 duvdy_1 = dfdy(uv_2);

    if(layer_0 == 65535U)
    {
        return normal_5;
    }

    thread TangentFrame_0 basis_5;

#line 5296
    uint _S33 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 5305
        (&basis_5)->normal_0 = normal_5;
        float3 tangent_4 = input_0->world_tangent_0 - normal_5 * float3(dot(normal_5, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 5307
        float3 _S34;

#line 5312
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 5312
            _S34 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 5312
        }
        else
        {

#line 5312
            _S34 = orthonormal_tangent_0(normal_5);

#line 5312
        }

#line 5312
        (&basis_5)->tangent_1 = _S34;

#line 5318
        float3 _S35 = cross((&basis_5)->normal_0, _S34);

#line 5318
        float _S36;
        if((_S33 & 2U) != 0U)
        {

#line 5319
            _S36 = -1.0f;

#line 5319
        }
        else
        {

#line 5319
            _S36 = 1.0f;

#line 5319
        }

#line 5318
        (&basis_5)->bitangent_0 = _S35 * float3(_S36) ;

#line 5297
    }
    else
    {

#line 5323
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_5);

#line 5297
    }

#line 5327
    float3 _S37 = float3(uv_2, float(layer_0));
    float3 _S38 = ((kernelContext_7->normal_textures_0).sample((kernelContext_7->base_color_sampler_0), ((_S37)).xy, uint(((_S37)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 5328
    thread float3 tangent_space_0 = _S38;
    tangent_space_0.xy = _S38.xy * float2(normal_scale_1) ;

#line 5334
    float3 _S39 = normalize(tangent_space_0);

#line 5334
    tangent_space_0 = _S39;
    return normalize(float3(_S39.x)  * (&basis_5)->tangent_1 + float3(_S39.y)  * (&basis_5)->bitangent_0 + float3(_S39.z)  * (&basis_5)->normal_0);
}


#line 3224
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 3235
    float3 _S40;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 3236
        _S40 = - facet_1;

#line 3236
    }
    else
    {

#line 3236
        _S40 = facet_1;

#line 3236
    }

#line 3236
    return _S40;
}


#line 1149
uint mro_layer_0(const GpuMaterial_natural_0 thread* material_7)
{
    return (material_7->mro_emissive_pages_0) & 65535U;
}


#line 2039
float4 mro_texel_0(const GpuMaterial_natural_0 thread* material_8, float2 uv_3, KernelContext_0 thread* kernelContext_8)
{
    float2 duvdx_2 = dfdx(uv_3);
    float2 duvdy_2 = dfdy(uv_3);

#line 2042
    uint _S41 = mro_layer_0(material_8);

    if(_S41 == 65535U)
    {
        return float4(1.0f, 1.0f, 1.0f, 1.0f);
    }

    float3 _S42 = float3(uv_3, float(_S41));

#line 2048
    return ((kernelContext_8->mro_textures_0).sample((kernelContext_8->base_color_sampler_0), ((_S42)).xy, uint(((_S42)).z), gradient2d((duvdx_2), (duvdy_2))));
}


#line 1161
uint emissive_layer_0(const GpuMaterial_natural_0 thread* material_9)
{
    return (material_9->mro_emissive_pages_0) >> 16U;
}


#line 2058
float4 emissive_texel_0(const GpuMaterial_natural_0 thread* material_10, float2 uv_4, KernelContext_0 thread* kernelContext_9)
{
    float2 duvdx_3 = dfdx(uv_4);
    float2 duvdy_3 = dfdy(uv_4);

#line 2061
    uint _S43 = emissive_layer_0(material_10);

    if(_S43 == 65535U)
    {
        return float4(1.0f, 1.0f, 1.0f, 1.0f);
    }

    float3 _S44 = float3(uv_4, float(_S43));

#line 2067
    return ((kernelContext_9->emissive_textures_0).sample((kernelContext_9->base_color_sampler_0), ((_S44)).xy, uint(((_S44)).z), gradient2d((duvdx_3), (duvdy_3))));
}


#line 2085
float metallic_of_0(const GpuMaterial_natural_0 thread* material_11, float4 mro_0)
{
    return saturate(material_11->metallic_0 * mro_0.z);
}


#line 2629
float specular_aa_kernel_0(float3 normal_6)
{
    float3 dndx_0 = dfdx(normal_6);
    float3 dndy_0 = dfdy(normal_6);


    return min(2.0f * (0.25f * (dot(dndx_0, dndx_0) + dot(dndy_0, dndy_0))), 0.18000000715255737f);
}


#line 1179
float3 specular_f0_of_0(const GpuMaterial_natural_0 thread* material_12)
{
    return float3(material_12->specular_f0_r_0, material_12->specular_f0_g_0, material_12->specular_f0_b_0);
}


#line 1179
float3 specular_f0_of_1(const GpuMaterial_natural_0 thread* material_13)
{
    return float3(material_13->specular_f0_r_0, material_13->specular_f0_g_0, material_13->specular_f0_b_0);
}


#line 2510
float dielectric_diffuse_weight_0(const GpuMaterial_natural_0 thread* material_14)
{

#line 2510
    float3 _S45 = specular_f0_of_1(material_14);


    float _S46 = max(_S45.x, max(_S45.y, _S45.z));
    if(_S46 == 0.03999999910593033f)
    {
        return 1.0f;
    }
    return max(1.0f - (_S46 - 0.03999999910593033f) / 0.95999997854232788f, 0.0f);
}


#line 4523
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_10)
{
    uint _S47 = max(kernelContext_10->frame_0->cluster_grid_0.x, 1U);
    uint _S48 = max(kernelContext_10->frame_0->cluster_grid_0.y, 1U);
    uint _S49 = max(kernelContext_10->frame_0->cluster_grid_0.z, 1U);
    uint _S50 = max(kernelContext_10->frame_0->cluster_grid_0.w, 1U);

#line 4533
    uint _S51 = uint(pixel_0.x) / _S50;

#line 4533
    uint _S52 = min(_S51, _S47 - 1U);
    uint _S53 = uint(pixel_0.y) / _S50;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4544
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S49 - 1U))) * _S48 + min(_S53, _S48 - 1U)) * _S47 + _S52;
}


#line 2656
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2677
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_11)
{

#line 2677
    texture2d<float, access::sample> _S54 = kernelContext_11->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S54).get_width(0)),(*((&height_1)) = (_S54).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2683
    float2 _S55 = float2(1.0f) ;
    float2 _S56 = extent_1 - _S55;

#line 2684
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S56);
    float2 high_1 = min(low_1 + _S55, _S56);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2702
float2 decode_dfg_pair_0(float4 texel_1)
{
    return float2(texel_1.x * 65280.0f + texel_1.y * 255.0f, texel_1.z * 65280.0f + texel_1.w * 255.0f) / float2(65535.0f) ;
}


#line 2714
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_12)
{
    int _S57 = tap_1->lo_0.x;

#line 2716
    int _S58 = tap_1->lo_0.y;

#line 2716
    int3 _S59 = int3(_S57, _S58, int(0));
    int _S60 = tap_1->hi_0.x;

#line 2717
    int3 _S61 = int3(_S60, _S58, int(0));
    float2 _S62 = float2(tap_1->weight_0.x) ;
    int _S63 = tap_1->hi_0.y;

#line 2719
    int3 _S64 = int3(_S57, _S63, int(0));
    int3 _S65 = int3(_S60, _S63, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S59)).xy), uint(((_S59)).z)))), decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S61)).xy), uint(((_S61)).z)))), _S62), mix(decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S64)).xy), uint(((_S64)).z)))), decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S65)).xy), uint(((_S65)).z)))), _S62), float2(tap_1->weight_0.y) );
}


#line 4474
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 4490
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 4502
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 4509
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 3043
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 3043
    float4 _S66 = float4(light_0->tangent_0) ;

    float3 _S67 = _S66.xyz;

#line 3045
    float3 across_0 = _S67 * float3(_S66.w) ;

#line 3045
    float4 _S68 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S67, _S68.xyz) * float3(_S68.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S69 = centre_0 - across_0;

#line 3048
    (*corners_0)[int(0)] = _S69 - down_0;
    float3 _S70 = centre_0 + across_0;

#line 3049
    (*corners_0)[int(1)] = _S70 - down_0;
    (*corners_0)[int(2)] = _S70 + down_0;
    (*corners_0)[int(3)] = _S69 + down_0;
    return;
}


#line 2801
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_7, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_7 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2804
    float3 seed_0;
    if((abs(normal_7.z)) < 0.89999997615814209f)
    {

#line 2805
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2805
    }
    else
    {

#line 2805
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2805
    }

#line 2805
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2806
        tangent_5 = across_1 / float3(span_0) ;

#line 2806
    }
    else
    {

#line 2806
        tangent_5 = normalize(cross(seed_0, normal_7));

#line 2806
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_7, tangent_5), normal_7);
}


#line 2782
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2872
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2872
    float3 _S71 = polygon_0->corner_0[int(0)];

#line 2872
    float3 _S72 = polygon_0->corner_0[int(1)];

#line 2872
    float3 _S73 = polygon_0->corner_0[int(2)];

#line 2872
    float3 _S74 = polygon_0->corner_0[int(3)];

#line 2878
    float3 _S75 = float3(0.0f, 0.0f, 0.0f);


    float _S76 = polygon_0->corner_0[int(0)].z;

#line 2881
    int count_1;

#line 2881
    if(_S76 > 0.0f)
    {

#line 2881
        count_1 = int(1);

#line 2881
    }
    else
    {

#line 2881
        count_1 = int(0);

#line 2881
    }
    float _S77 = _S72.z;

#line 2882
    int _S78;

#line 2882
    if(_S77 > 0.0f)
    {

#line 2882
        _S78 = int(2);

#line 2882
    }
    else
    {

#line 2882
        _S78 = int(0);

#line 2882
    }

#line 2882
    int config_0 = count_1 + _S78;
    float _S79 = _S73.z;

#line 2883
    if(_S79 > 0.0f)
    {

#line 2883
        count_1 = int(4);

#line 2883
    }
    else
    {

#line 2883
        count_1 = int(0);

#line 2883
    }

#line 2883
    int config_1 = config_0 + count_1;
    float _S80 = _S74.z;

#line 2884
    if(_S80 > 0.0f)
    {

#line 2884
        count_1 = int(8);

#line 2884
    }
    else
    {

#line 2884
        count_1 = int(0);

#line 2884
    }

#line 2884
    int config_2 = config_1 + count_1;

#line 2884
    float3 l0_0;

#line 2884
    float3 l1_0;

#line 2884
    float3 l2_0;

#line 2884
    float3 l3_0;

#line 2884
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2887
        float3 _S81 = float3(_S76) ;


        float3 _S82 = float3(- _S77)  * _S71 + _S81 * _S72;
        float3 _S83 = float3(- _S80)  * _S71 + _S81 * _S74;

#line 2891
        count_1 = int(3);

#line 2891
        l0_0 = _S71;

#line 2891
        l1_0 = _S82;

#line 2891
        l2_0 = _S83;

#line 2891
        l3_0 = _S74;

#line 2891
        l4_0 = _S75;

#line 2887
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2893
            float3 _S84 = float3(_S77) ;


            float3 _S85 = float3(- _S76)  * _S72 + _S84 * _S71;
            float3 _S86 = float3(- _S79)  * _S72 + _S84 * _S73;

#line 2897
            count_1 = int(3);

#line 2897
            l0_0 = _S85;

#line 2897
            l1_0 = _S72;

#line 2897
            l2_0 = _S86;

#line 2897
            l3_0 = _S74;

#line 2897
            l4_0 = _S75;

#line 2893
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S87 = float3(- _S79)  * _S72 + float3(_S77)  * _S73;
                float3 _S88 = float3(- _S80)  * _S71 + float3(_S76)  * _S74;

#line 2903
                count_1 = int(4);

#line 2903
                l0_0 = _S71;

#line 2903
                l1_0 = _S72;

#line 2903
                l2_0 = _S87;

#line 2903
                l3_0 = _S88;

#line 2903
                l4_0 = _S75;

#line 2899
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2905
                    float3 _S89 = float3(_S79) ;


                    float3 _S90 = float3(- _S80)  * _S73 + _S89 * _S74;
                    float3 _S91 = float3(- _S77)  * _S73 + _S89 * _S72;

#line 2909
                    count_1 = int(3);

#line 2909
                    l0_0 = _S90;

#line 2909
                    l1_0 = _S91;

#line 2909
                    l2_0 = _S73;

#line 2909
                    l3_0 = _S74;

#line 2909
                    l4_0 = _S75;

#line 2905
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S92 = float3(- _S76)  * _S72 + float3(_S77)  * _S71;
                        float3 _S93 = float3(- _S80)  * _S73 + float3(_S79)  * _S74;

#line 2915
                        count_1 = int(4);

#line 2915
                        l0_0 = _S92;

#line 2915
                        l1_0 = _S72;

#line 2915
                        l2_0 = _S73;

#line 2915
                        l3_0 = _S93;

#line 2915
                        l4_0 = _S75;

#line 2911
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2917
                            float3 _S94 = float3(- _S80) ;


                            float3 _S95 = _S94 * _S71 + float3(_S76)  * _S74;
                            float3 _S96 = _S94 * _S73 + float3(_S79)  * _S74;

#line 2921
                            count_1 = int(5);

#line 2921
                            l0_0 = _S71;

#line 2921
                            l1_0 = _S72;

#line 2921
                            l2_0 = _S73;

#line 2921
                            l3_0 = _S96;

#line 2921
                            l4_0 = _S95;

#line 2917
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2923
                                float3 _S97 = float3(_S80) ;


                                float3 _S98 = float3(- _S76)  * _S74 + _S97 * _S71;
                                float3 _S99 = float3(- _S79)  * _S74 + _S97 * _S73;

#line 2927
                                count_1 = int(3);

#line 2927
                                l0_0 = _S98;

#line 2927
                                l1_0 = _S99;

#line 2927
                                l2_0 = _S74;

#line 2927
                                l3_0 = _S74;

#line 2927
                                l4_0 = _S75;

#line 2923
                            }
                            else
                            {

#line 2930
                                if(config_2 == int(9))
                                {

                                    float3 _S100 = float3(- _S77)  * _S71 + float3(_S76)  * _S72;
                                    float3 _S101 = float3(- _S79)  * _S74 + float3(_S80)  * _S73;

#line 2934
                                    count_1 = int(4);

#line 2934
                                    l0_0 = _S71;

#line 2934
                                    l1_0 = _S100;

#line 2934
                                    l2_0 = _S101;

#line 2934
                                    l3_0 = _S74;

#line 2934
                                    l4_0 = _S75;

#line 2930
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S102 = float3(- _S80)  * _S73 + float3(_S79)  * _S74;
                                        float3 _S103 = float3(- _S79)  * _S72 + float3(_S77)  * _S73;

#line 2941
                                        count_1 = int(5);

#line 2941
                                        l0_0 = _S71;

#line 2941
                                        l1_0 = _S72;

#line 2941
                                        l2_0 = _S103;

#line 2941
                                        l3_0 = _S102;

#line 2941
                                        l4_0 = _S74;

#line 2936
                                    }
                                    else
                                    {

#line 2943
                                        if(config_2 == int(12))
                                        {

                                            float3 _S104 = float3(- _S77)  * _S73 + float3(_S79)  * _S72;
                                            float3 _S105 = float3(- _S76)  * _S74 + float3(_S80)  * _S71;

#line 2947
                                            count_1 = int(4);

#line 2947
                                            l0_0 = _S105;

#line 2947
                                            l1_0 = _S104;

#line 2947
                                            l2_0 = _S73;

#line 2947
                                            l3_0 = _S74;

#line 2947
                                            l4_0 = _S75;

#line 2943
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S106 = float3(- _S79)  * _S72 + float3(_S77)  * _S73;
                                                float3 _S107 = float3(- _S77)  * _S71 + float3(_S76)  * _S72;

#line 2955
                                                count_1 = int(5);

#line 2955
                                                l0_0 = _S71;

#line 2955
                                                l1_0 = _S107;

#line 2955
                                                l2_0 = _S106;

#line 2955
                                                l3_0 = _S73;

#line 2955
                                                l4_0 = _S74;

#line 2949
                                            }
                                            else
                                            {

#line 2957
                                                if(config_2 == int(14))
                                                {

#line 2957
                                                    float3 _S108 = float3(- _S76) ;


                                                    float3 _S109 = _S108 * _S74 + float3(_S80)  * _S71;
                                                    float3 _S110 = _S108 * _S72 + float3(_S77)  * _S71;

#line 2961
                                                    count_1 = int(5);

#line 2961
                                                    l0_0 = _S110;

#line 2961
                                                    l1_0 = _S109;

#line 2957
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2963
                                                        count_1 = int(4);

#line 2963
                                                    }
                                                    else
                                                    {

#line 2963
                                                        count_1 = int(0);

#line 2963
                                                    }

#line 2963
                                                    l0_0 = _S71;

#line 2963
                                                    l1_0 = _S75;

#line 2957
                                                }

#line 2878
                                                float3 _S111 = l1_0;

#line 2878
                                                l1_0 = _S72;

#line 2878
                                                l2_0 = _S73;

#line 2878
                                                l3_0 = _S74;

#line 2878
                                                l4_0 = _S111;

#line 2949
                                            }

#line 2943
                                        }

#line 2936
                                    }

#line 2930
                                }

#line 2923
                            }

#line 2917
                        }

#line 2911
                    }

#line 2905
                }

#line 2899
            }

#line 2893
        }

#line 2887
    }

#line 2971
    if(count_1 <= int(3))
    {

#line 2971
        l3_0 = l0_0;

#line 2971
        l4_0 = l0_0;

#line 2971
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2976
            l4_0 = l0_0;

#line 2976
        }

#line 2971
    }

#line 2981
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2844
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2850
    float weight_1;

#line 2855
    if(cosine_0 > 0.0f)
    {

#line 2855
        weight_1 = fit_0;

#line 2855
    }
    else
    {

#line 2855
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2855
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 3001
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 3003
    int corner_1 = int(0);
    for(;;)
    {

#line 3004
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 3004
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 3004
        corner_1 = corner_1 + int(1);

#line 3004
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 3009
    thread LtcPolygon_0 _S112 = polygon_1;

#line 3009
    LtcPolygon_0 _S113 = ltc_clip_0(&_S112);
    polygon_1 = _S113;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 3013
    int at_2 = int(0);

    for(;;)
    {

#line 3015
        if(at_2 < int(5))
        {
        }
        else
        {

#line 3015
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 3015
        at_2 = at_2 + int(1);

#line 3015
    }

#line 3022
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 3022
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 3023
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 3023
    }
    else
    {

#line 3023
        sum_1 = sum_0;

#line 3023
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 3027
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 3027
    }

#line 3034
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2730
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_13)
{
    int _S114 = tap_2->lo_0.x;

#line 2732
    int _S115 = tap_2->lo_0.y;

#line 2732
    int3 _S116 = int3(_S114, _S115, int(0));
    int _S117 = tap_2->hi_0.x;

#line 2733
    int3 _S118 = int3(_S117, _S115, int(0));
    float4 _S119 = float4(tap_2->weight_0.x) ;
    int _S120 = tap_2->hi_0.y;

#line 2735
    int3 _S121 = int3(_S114, _S120, int(0));
    int3 _S122 = int3(_S117, _S120, int(0));

    return mix(mix(((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S116)).xy), uint(((_S116)).z))), ((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z))), _S119), mix(((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S121)).xy), uint(((_S121)).z))), ((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S122)).xy), uint(((_S122)).z))), _S119), float4(tap_2->weight_0.y) );
}


#line 2817
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2553
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float f90_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2560
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2567
    float _S123 = 1.0f - alpha2_0;

#line 2573
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S123 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S123 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(f90_0, f90_0, f90_0) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3646
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3646
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3706
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3678
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_16)
{
    return rect_1.x / kernelContext_16->frame_0->shadow_params_0.x;
}


#line 3275
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3633
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_17)
{

#line 3633
    uint _S124;

    if(uint(pixel_1.x) < (kernelContext_17->frame_0->shadow_filter_0.z))
    {

#line 3635
        _S124 = kernelContext_17->frame_0->shadow_filter_0.x;

#line 3635
    }
    else
    {

#line 3635
        _S124 = kernelContext_17->frame_0->shadow_filter_0.y;

#line 3635
    }

#line 3635
    return _S124;
}


#line 3658
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_18)
{
    return kernelContext_18->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3658
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_19)
{
    return kernelContext_19->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 383
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3728
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_20)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S125 = spoke_0.x;

#line 3733
    float _S126 = rotation_0.x;

#line 3733
    float _S127 = spoke_0.y;

#line 3733
    float _S128 = rotation_0.y;


    float _S129 = ((kernelContext_20->shadow_atlas_0).sample_compare((kernelContext_20->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S125 * _S126 - _S127 * _S128, _S125 * _S128 + _S127 * _S126) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3736
    return _S129;
}


#line 3816
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_21)
{

#line 3816
    float4 _S130 = atlas_rect_1(tile_2, kernelContext_21);


    if(atlas_rect_is_empty_0(_S130))
    {
        return 1.0f;
    }

#line 3821
    float2 _S131 = atlas_step_1(_S130, kernelContext_21);

#line 3821
    int y_1 = int(-1);

#line 3821
    float visibility_0 = 0.0f;

#line 3826
    for(;;)
    {

#line 3826
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3826
            break;
        }

#line 3826
        int x_0 = int(-1);

        for(;;)
        {

#line 3828
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3828
                break;
            }

#line 3828
            float _S132 = tile_tap_0(_S130, _S131, tile_uv_2, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_1, kernelContext_21);

            float visibility_1 = visibility_0 + _S132;

#line 3828
            x_0 = x_0 + int(1);

#line 3828
            visibility_0 = visibility_1;

#line 3828
        }

#line 3826
        y_1 = y_1 + int(1);

#line 3826
    }

#line 3834
    return visibility_0 / 9.0f;
}


#line 3591
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3758
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_22)
{
    float2 _S133 = shadow_rotation_0(pixel_3);

#line 3760
    float4 _S134 = atlas_rect_1(tile_3, kernelContext_22);

    if(atlas_rect_is_empty_0(_S134))
    {
        return 1.0f;
    }

#line 3764
    float2 _S135 = atlas_step_1(_S134, kernelContext_22);

#line 3764
    uint spot_0 = 0U;

#line 3764
    float probe_0 = 0.0f;

#line 3769
    for(;;)
    {

#line 3769
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3769
            break;
        }

#line 3769
        float _S136 = tile_tap_0(_S134, _S135, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S133, reference_2, kernelContext_22);

        float probe_1 = probe_0 + _S136;

#line 3769
        spot_0 = spot_0 + 1U;

#line 3769
        probe_0 = probe_1;

#line 3769
    }

#line 3778
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3784
    uint index_2 = 0U;

#line 3784
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 3788
        if(index_2 < 32U)
        {
        }
        else
        {

#line 3788
            break;
        }

#line 3788
        float _S137 = tile_tap_0(_S134, _S135, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S133, reference_2, kernelContext_22);

        float visibility_3 = visibility_2 + _S137;

#line 3788
        index_2 = index_2 + 1U;

#line 3788
        visibility_2 = visibility_3;

#line 3788
    }

#line 3793
    return visibility_2 / 32.0f;
}


#line 3869
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_23)
{
    float2 texel_2 = kernelContext_23->frame_0->shadow_params_0.xy;

#line 3871
    float4 _S138 = atlas_rect_0(cascade_0, kernelContext_23);

#line 3871
    float2 _S139 = atlas_step_0(_S138, kernelContext_23);


    float2 _S140 = float2(0.5f, 0.5f) * _S139;


    float2 _S141 = float2(1.0f, 1.0f);

#line 3877
    float2 _S142 = _S141 / texel_2;

#line 3877
    uint index_3 = 0U;

#line 3877
    float sum_2 = 0.0f;

#line 3877
    float found_0 = 0.0f;



    for(;;)
    {

#line 3881
        if(index_3 < 16U)
        {
        }
        else
        {

#line 3881
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S143 = spoke_1.x;

#line 3884
        float _S144 = rotation_1.x;

#line 3884
        float _S145 = spoke_1.y;

#line 3884
        float _S146 = rotation_1.y;

#line 3892
        int3 _S147 = int3(int2(min(atlas_uv_0(_S138, clamp(tile_uv_4 + float2(_S143 * _S144 - _S145 * _S146, _S143 * _S146 + _S145 * _S144) * _S139, _S140, float2(1.0f)  - _S140)) * _S142, _S142 - _S141)), int(0));

#line 3892
        float depth_1 = ((kernelContext_23->shadow_atlas_0).read(vec<uint,2>(((_S147)).xy), uint(((_S147)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3896
            sum_2 = sum_2 + depth_1;

#line 3896
            found_0 = found_1;

#line 3893
        }

#line 3881
        index_3 = index_3 + 1U;

#line 3881
    }

#line 3900
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3911
    float _S148 = 2.0f * kernelContext_23->frame_0->cascade_far_0[cascade_0];

#line 3911
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S148 + 40.0f);

#line 3911
    float _S149 = tile_texels_0(_S138, kernelContext_23);

    return clamp(separation_0 * 0.01999999955296516f / (_S148 / _S149), 2.0f, 8.0f);
}


#line 3965
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_24)
{

#line 3966
    float4 _S150 = atlas_rect_0(cascade_1, kernelContext_24);

#line 4000
    if(atlas_rect_is_empty_0(_S150))
    {


        return 1.0f;
    }
    float _S151 = 2.0f * kernelContext_24->frame_0->cascade_far_0[cascade_1];

#line 4006
    float _S152 = tile_texels_0(_S150, kernelContext_24);

#line 4006
    float texel_world_0 = _S151 / _S152;

#line 4013
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_24->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_24->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_24->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 4017
    bool _S153;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 4018
        _S153 = true;

#line 4018
    }
    else
    {

#line 4018
        _S153 = (ndc_0.z) <= 0.0f;

#line 4018
    }

#line 4018
    if(_S153)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 4028
    uint _S154 = shadow_filter_mode_0(pixel_4, kernelContext_24);

#line 4045
    if(_S154 == 2U)
    {

#line 4045
        float _S155 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_24);

        return _S155;
    }
    if(_S154 == 1U)
    {

#line 4049
        float _S156 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_24);



        return _S156;
    }

    float _S157 = ndc_0.z;

#line 4056
    float _S158 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S157, shadow_rotation_0(pixel_4), kernelContext_24);

#line 4056
    float _S159 = tile_pcf_0(cascade_1, tile_uv_5, _S157, pixel_4, _S158, kernelContext_24);
    return _S159;
}


#line 4136
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_25)
{
    uint cascade_2;

#line 4138
    bool covered_0;

#line 4147
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 4159
    float eye_distance_0 = length(world_position_5 - kernelContext_25->frame_0->camera_position_0.xyz);

#line 4159
    uint index_4 = 0U;

#line 4167
    for(;;)
    {

#line 4167
        if(index_4 < 2U)
        {
        }
        else
        {

#line 4167
            covered_0 = false;

#line 4167
            cascade_2 = 1U;

#line 4167
            break;
        }
        if(eye_distance_0 < kernelContext_25->frame_0->cascade_far_0[index_4])
        {

#line 4169
            covered_0 = true;

#line 4169
            cascade_2 = index_4;



            break;
        }

#line 4167
        index_4 = index_4 + 1U;

#line 4167
    }

#line 4176
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 4176
    }

#line 4176
    float _S160 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_25);

#line 4183
    uint _S161 = cascade_2 + 1U;

#line 4183
    if(_S161 >= 2U)
    {



        return _S160;
    }

#line 4196
    float band_0 = kernelContext_25->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_25->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S160;
    }

#line 4204
    float _S162 = cascade_visibility_0(_S161, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_25);

#line 4215
    return mix(_S160, _S162, blend_0);
}


#line 5403
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_26)
{

#line 5403
    texture2d<float, access::sample> _S163 = kernelContext_26->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S163).get_width(0)),(*((&height_2)) = (_S163).get_height(0));

    int3 _S164 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 5409
    return ((kernelContext_26->contact_shadow_0).read(vec<uint,2>(((_S164)).xy), uint(((_S164)).z)).x);
}


#line 4108
float3 cascade_tint_0(uint cascade_3, float blend_1)
{
    if(cascade_3 >= 2U)
    {
        return float3(1.0f, 1.0f, 1.0f);
    }
    uint _S165 = cascade_3 + 1U;

#line 4114
    if(_S165 >= 2U)
    {


        return CASCADE_TINTS_0[cascade_3];
    }
    return mix(CASCADE_TINTS_0[cascade_3], CASCADE_TINTS_0[_S165], float3(blend_1) );
}


#line 4426
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S166 = axis_2.x;

#line 4429
    float _S167 = axis_2.y;

#line 4429
    bool _S168;

#line 4429
    if(_S166 >= _S167)
    {

#line 4429
        _S168 = _S166 >= (axis_2.z);

#line 4429
    }
    else
    {

#line 4429
        _S168 = false;

#line 4429
    }

#line 4429
    uint _S169;

#line 4429
    if(_S168)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 4431
            _S169 = 0U;

#line 4431
        }
        else
        {

#line 4431
            _S169 = 1U;

#line 4431
        }

#line 4431
        return _S169;
    }
    if(_S167 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 4435
            _S169 = 2U;

#line 4435
        }
        else
        {

#line 4435
            _S169 = 3U;

#line 4435
        }

#line 4435
        return _S169;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 4437
        _S169 = 4U;

#line 4437
    }
    else
    {

#line 4437
        _S169 = 5U;

#line 4437
    }

#line 4437
    return _S169;
}


#line 370
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 4322
float punctual_visibility_0(uint tile_5, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_27)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 4325
    float4 _S170 = atlas_rect_0(atlas_0, kernelContext_27);

    if(atlas_rect_is_empty_0(_S170))
    {


        return 1.0f;
    }

#line 4331
    float _S171 = tile_texels_0(_S170, kernelContext_27);

    float texel_world_1 = map_world_0 / _S171;

#line 4343
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_27->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 4350
    float _S172 = clip_1.w;

#line 4350
    if(_S172 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S172) ;

#line 4354
    bool _S173;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 4355
        _S173 = true;

#line 4355
    }
    else
    {

#line 4355
        _S173 = (ndc_1.z) <= 0.0f;

#line 4355
    }

#line 4355
    if(_S173)
    {

#line 4355
        _S173 = true;

#line 4355
    }
    else
    {

#line 4355
        _S173 = (ndc_1.z) > 1.0f;

#line 4355
    }

#line 4355
    if(_S173)
    {

#line 4362
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 4367
    uint _S174 = shadow_filter_mode_0(pixel_6, kernelContext_27);

#line 4376
    if(_S174 == 2U)
    {

#line 4376
        float _S175 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_27);

        return _S175;
    }

#line 4378
    float _S176 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_27);

    return _S176;
}


#line 4445
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_0, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_28)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 4453
    float _S177 = punctual_visibility_0(base_0 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_28);

#line 4459
    return _S177;
}


#line 4387
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_6, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_29)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 4394
    float4 _S178 = float4(light_2->direction_0) ;

#line 4401
    float cos_outer_1 = _S178.w;

#line 4401
    float _S179 = punctual_visibility_0(tile_6, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S178.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_29);

#line 4408
    return _S179;
}


#line 2758
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 5390
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 5392
    float3 _S180;
    if((length(decoded_0)) < 0.5f)
    {

#line 5393
        _S180 = shading_normal_1;

#line 5393
    }
    else
    {

#line 5393
        _S180 = normalize(decoded_0);

#line 5393
    }

#line 5393
    return _S180;
}


#line 5028
float3 sky_irradiance_0(float3 normal_8, KernelContext_0 thread* kernelContext_30)
{
    float4 basis_6 = float4(normal_8, 1.0f);
    return max(float3(dot(kernelContext_30->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_30->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_30->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4932
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4932
    float reach_0 = 0.0f;

#line 4932
    uint axis_3 = 0U;


    for(;;)
    {

#line 4935
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4935
            break;
        }

#line 4935
        uint _S181 = axis_3;

#line 4935
        bool _S182;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4937
            _S182 = true;

#line 4937
        }
        else
        {

#line 4937
            _S182 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4937
        }

#line 4937
        if(_S182)
        {

#line 4938
            axis_3 = axis_3 + 1U;

#line 4935
            continue;
        }

#line 4935
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S181] - 1.0f));

#line 4935
        axis_3 = axis_3 + 1U;

#line 4935
    }

#line 4942
    return reach_0;
}


#line 4962
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4962
    uint level_0 = 0U;

    for(;;)
    {

#line 4964
        uint _S183 = level_0 + 1U;

#line 4964
        if(_S183 < levels_0)
        {
        }
        else
        {

#line 4964
            break;
        }
        float _S184 = float(level_0);

#line 4966
        float at_3 = reach_1 * exp2(- _S184);
        if(at_3 < 1.0f)
        {

#line 4968
            return float2(_S184, saturate((1.0f - at_3) / 0.25f));
        }

#line 4964
        level_0 = _S183;

#line 4964
    }

#line 4970
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4719
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4721
    uint _S185;
    if(at_4 >= count_2)
    {

#line 4722
        _S185 = at_4 - count_2;

#line 4722
    }
    else
    {

#line 4722
        _S185 = at_4;

#line 4722
    }

#line 4722
    return _S185;
}


#line 4745
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_31)
{
    uint3 counts_0 = kernelContext_31->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_31->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S186 = counts_0.x;
    uint _S187 = counts_0.y;



    return min(kernelContext_31->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S187 + probe_wrap_0(cell_2.y, offset_1.y, _S187)) * _S186 + probe_wrap_0(cell_2.x, offset_1.x, _S186), max(kernelContext_31->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4586
float sign_not_zero_0(float value_0)
{

#line 4586
    float _S188;

    if(value_0 >= 0.0f)
    {

#line 4588
        _S188 = 1.0f;

#line 4588
    }
    else
    {

#line 4588
        _S188 = -1.0f;

#line 4588
    }

#line 4588
    return _S188;
}


#line 4605
float2 oct_encode_0(float3 direction_1)
{
    float _S189 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S189) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4608
    float2 p_1;
    if(_S189 < 0.0f)
    {
        float _S190 = p_0.y;

#line 4611
        float _S191 = p_0.x;

#line 4611
        p_1 = float2((1.0f - abs(_S190)) * sign_not_zero_0(_S191), (1.0f - abs(_S191)) * sign_not_zero_0(_S190));

#line 4609
    }
    else
    {

#line 4609
        p_1 = p_0;

#line 4609
    }

#line 4614
    return p_1;
}


#line 4634
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_32)
{

#line 4634
    texture2d_array<float, access::sample> _S192 = kernelContext_32->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S192).get_width(0)),(*((&height_3)) = (_S192).get_height(0)),(*((&layers_0)) = (_S192).get_array_size());

#line 4639
    float2 _S193 = float2(0.5f) ;

#line 4639
    float2 _S194 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S193 + _S193) * float2(16.0f)  + _S194 - _S193;
    float2 _S195 = float2(float(width_3), float(height_3)) - _S194;

#line 4643
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S195);
    float2 high_2 = min(low_2 + _S194, _S195);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S196 = int(low_2.x);

#line 4648
    int _S197 = int(low_2.y);

#line 4648
    int4 _S198 = int4(_S196, _S197, layer_1, int(0));
    int _S199 = int(high_2.x);

#line 4649
    int4 _S200 = int4(_S199, _S197, layer_1, int(0));
    int _S201 = int(high_2.y);

#line 4650
    int4 _S202 = int4(_S196, _S201, layer_1, int(0));
    int4 _S203 = int4(_S199, _S201, layer_1, int(0));
    float2 _S204 = float2(weight_2.x) ;

#line 4652
    return mix(mix(((kernelContext_32->probe_visibility_0).read(vec<uint,2>(((_S198)).xy), uint(((_S198)).z), uint(((_S198)).w))).xy, ((kernelContext_32->probe_visibility_0).read(vec<uint,2>(((_S200)).xy), uint(((_S200)).z), uint(((_S200)).w))).xy, _S204), mix(((kernelContext_32->probe_visibility_0).read(vec<uint,2>(((_S202)).xy), uint(((_S202)).z), uint(((_S202)).w))).xy, ((kernelContext_32->probe_visibility_0).read(vec<uint,2>(((_S203)).xy), uint(((_S203)).z), uint(((_S203)).w))).xy, _S204), float2(weight_2.y) );
}


#line 4680
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_9, KernelContext_0 thread* kernelContext_33)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_9 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4683
    float2 _S205 = probe_moments_0(index_6, - to_probe_0, kernelContext_33);

#line 4689
    float _S206 = _S205.x;

#line 4689
    float _S207 = max(_S205.y - _S206 * _S206, 0.0f);
    float behind_0 = to_surface_0 - _S206;
    float bound_0 = _S207 / (_S207 + behind_0 * behind_0);

#line 4691
    float _S208;
    if(to_surface_0 <= _S206)
    {

#line 4692
        _S208 = 1.0f;

#line 4692
    }
    else
    {

#line 4692
        _S208 = bound_0 * bound_0 * bound_0;

#line 4692
    }

#line 4692
    return _S208;
}


#line 4702
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_10, KernelContext_0 thread* kernelContext_34)
{

#line 4702
    float _S209 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_10, kernelContext_34);

    return max(_S209, 0.00009999999747379f);
}


#line 1284
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4764
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4791
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_11, KernelContext_0 thread* kernelContext_35)
{

#line 4792
    uint _S210 = probe_row_0(level_2, cell_3, kernelContext_35);


    GpuProbe_natural_0 stored_0 = kernelContext_35->probes_0[_S210];

#line 4795
    float _S211 = probe_weight_0(_S210, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_11, kernelContext_35);



    thread WeightedProbe_0 corner_2;

#line 4799
    float4 _S212 = float4(_S211) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S212;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S212;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S212;
    (&corner_2)->weight_3 = _S211;
    return corner_2;
}


#line 4775
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S213 = float4(t_1) ;

#line 4778
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S213);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S213);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S213);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4863
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_12, KernelContext_0 thread* kernelContext_36)
{

#line 4863
    float3 _S214 = float3(1.0f) ;

#line 4868
    float3 _S215 = float3(0.0f, 0.0f, 0.0f);

#line 4868
    float3 last_1 = max(float3(kernelContext_36->frame_0->probe_counts_0.xyz) - _S214, _S215);



    float3 origin_2 = kernelContext_36->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_36->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S215, last_1);
    float3 base_1 = floor(grid_0);
    float3 f_0 = grid_0 - base_1;

    uint3 _S216 = uint3(base_1);



    uint3 _S217 = uint3(min(base_1 + _S214, last_1));

#line 4888
    float _S218 = inv_0.x;

#line 4888
    float _S219;

#line 4888
    if(_S218 != 0.0f)
    {

#line 4888
        _S219 = 1.0f / _S218;

#line 4888
    }
    else
    {

#line 4888
        _S219 = 0.0f;

#line 4888
    }
    float _S220 = inv_0.y;

#line 4889
    float _S221;

#line 4889
    if(_S220 != 0.0f)
    {

#line 4889
        _S221 = 1.0f / _S220;

#line 4889
    }
    else
    {

#line 4889
        _S221 = 0.0f;

#line 4889
    }
    float _S222 = inv_0.z;

#line 4890
    float _S223;

#line 4890
    if(_S222 != 0.0f)
    {

#line 4890
        _S223 = 1.0f / _S222;

#line 4890
    }
    else
    {

#line 4890
        _S223 = 0.0f;

#line 4890
    }

#line 4888
    float3 spacing_1 = float3(_S219, _S221, _S223);

#line 4897
    uint _S224 = _S216.x;

#line 4897
    uint _S225 = _S216.y;

#line 4897
    uint _S226 = _S216.z;

#line 4897
    WeightedProbe_0 _S227 = probe_corner_0(level_3, uint3(_S224, _S225, _S226), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);
    uint _S228 = _S217.x;

#line 4898
    WeightedProbe_0 _S229 = probe_corner_0(level_3, uint3(_S228, _S225, _S226), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);

#line 4898
    float _S230 = f_0.x;

#line 4898
    thread WeightedProbe_0 _S231 = _S227;

#line 4898
    thread WeightedProbe_0 _S232 = _S229;

#line 4898
    WeightedProbe_0 _S233 = lerp_probe_0(&_S231, &_S232, _S230);
    uint _S234 = _S217.y;

#line 4899
    WeightedProbe_0 _S235 = probe_corner_0(level_3, uint3(_S224, _S234, _S226), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);

#line 4899
    WeightedProbe_0 _S236 = probe_corner_0(level_3, uint3(_S228, _S234, _S226), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);

#line 4899
    thread WeightedProbe_0 _S237 = _S235;

#line 4899
    thread WeightedProbe_0 _S238 = _S236;

#line 4899
    WeightedProbe_0 _S239 = lerp_probe_0(&_S237, &_S238, _S230);

    uint _S240 = _S217.z;

#line 4901
    WeightedProbe_0 _S241 = probe_corner_0(level_3, uint3(_S224, _S225, _S240), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);

#line 4901
    WeightedProbe_0 _S242 = probe_corner_0(level_3, uint3(_S228, _S225, _S240), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);

#line 4901
    thread WeightedProbe_0 _S243 = _S241;

#line 4901
    thread WeightedProbe_0 _S244 = _S242;

#line 4901
    WeightedProbe_0 _S245 = lerp_probe_0(&_S243, &_S244, _S230);

#line 4901
    WeightedProbe_0 _S246 = probe_corner_0(level_3, uint3(_S224, _S234, _S240), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);

#line 4901
    WeightedProbe_0 _S247 = probe_corner_0(level_3, uint3(_S228, _S234, _S240), origin_2, spacing_1, world_position_13, normal_12, kernelContext_36);

#line 4901
    thread WeightedProbe_0 _S248 = _S246;

#line 4901
    thread WeightedProbe_0 _S249 = _S247;

#line 4901
    WeightedProbe_0 _S250 = lerp_probe_0(&_S248, &_S249, _S230);



    float _S251 = f_0.y;

#line 4905
    thread WeightedProbe_0 _S252 = _S233;

#line 4905
    thread WeightedProbe_0 _S253 = _S239;

#line 4905
    WeightedProbe_0 _S254 = lerp_probe_0(&_S252, &_S253, _S251);

#line 4905
    thread WeightedProbe_0 _S255 = _S245;

#line 4905
    thread WeightedProbe_0 _S256 = _S250;

#line 4905
    WeightedProbe_0 _S257 = lerp_probe_0(&_S255, &_S256, _S251);

    float _S258 = f_0.z;

#line 4907
    thread WeightedProbe_0 _S259 = _S254;

#line 4907
    thread WeightedProbe_0 _S260 = _S257;

#line 4907
    WeightedProbe_0 _S261 = lerp_probe_0(&_S259, &_S260, _S258);

    float4 basis_7 = float4(normal_12, 1.0f);
    return max(float3(dot(_S261.sh_0.sh_r_0, basis_7), dot(_S261.sh_0.sh_g_0, basis_7), dot(_S261.sh_0.sh_b_0, basis_7)) / float3(_S261.weight_3) , _S215);
}


#line 4997
float3 probe_irradiance_0(float3 world_position_14, float3 normal_13, KernelContext_0 thread* kernelContext_37)
{

#line 5005
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_37->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_37->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_37->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_37->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 5007
    float3 _S262 = probe_level_irradiance_0(level_4, world_position_14, normal_13, kernelContext_37);


    if(share_0 >= 1.0f)
    {

#line 5011
        return _S262;
    }

#line 5011
    float3 _S263 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_13, kernelContext_37);

    return _S263 * float3((1.0f - share_0))  + _S262 * float3(share_0) ;
}


#line 5459
float3 multi_bounce_occlusion_0(float visibility_4, float3 albedo_0)
{

#line 5459
    float3 _S264 = float3(visibility_4) ;

#line 5465
    return min(float3(1.0f) , max(_S264, ((_S264 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S264 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S264));
}


#line 1171
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_15)
{
    return float3(material_15->emissive_r_0, material_15->emissive_g_0, material_15->emissive_b_0);
}


#line 3109
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S265 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 3117
    float kernel_0 = 0.0001984127011383f;

#line 3117
    int term_0 = int(6);

    for(;;)
    {

#line 3119
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 3119
            break;
        }
        float _S266 = kernel_0 * _S265 + FOG_KERNEL_0[term_0];

#line 3119
        int term_1 = term_0 - int(1);

#line 3119
        kernel_0 = _S266;

#line 3119
        term_0 = term_1;

#line 3119
    }

#line 3126
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 3136
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S267 = - d_0;

#line 3140
        float series_0 = 0.00833333376795053f;

#line 3140
        int term_2 = int(3);

        for(;;)
        {

#line 3142
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 3142
                break;
            }
            float _S268 = series_0 * _S267 + FOG_RATIO_KERNEL_0[term_2];

#line 3142
            int term_3 = term_2 - int(1);

#line 3142
            series_0 = _S268;

#line 3142
            term_2 = term_3;

#line 3142
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 3170
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 3181
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 3189
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 5054
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 5054
struct pixelInput_0
{
    float3 world_position_15 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_1 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_3 [[user(CRCBL_COLOR)]];
    [[flat]] uint material_16 [[user(CRCBL_MATERIAL)]];
    float2 uv_5 [[user(CRCBL_UV)]];
    float4 clip_position_1 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_1 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_1 [[user(CRCBL_WORLD_TANGENT)]];
    [[flat]] uint frame_4 [[user(CRCBL_FRAME)]];
};


#line 5501
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S269 [[stage_in]], bool front_facing_1 [[front_facing]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_3 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_3 [[texture(9)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 5501
    thread KernelContext_0 kernelContext_38;

#line 5501
    (&kernelContext_38)->draw_0 = draw_3;

#line 5501
    (&kernelContext_38)->visible_instances_0 = visible_instances_3;

#line 5501
    (&kernelContext_38)->instances_0 = instances_3;

#line 5501
    (&kernelContext_38)->meshes_0 = meshes_3;

#line 5501
    (&kernelContext_38)->frame_0 = frame_5;

#line 5501
    (&kernelContext_38)->vertices_0 = vertices_3;

#line 5501
    (&kernelContext_38)->ambient_occlusion_0 = ambient_occlusion_3;

#line 5501
    (&kernelContext_38)->materials_0 = materials_3;

#line 5501
    (&kernelContext_38)->base_color_textures_0 = base_color_textures_3;

#line 5501
    (&kernelContext_38)->base_color_sampler_0 = base_color_sampler_3;

#line 5501
    (&kernelContext_38)->normal_textures_0 = normal_textures_3;

#line 5501
    (&kernelContext_38)->mro_textures_0 = mro_textures_3;

#line 5501
    (&kernelContext_38)->emissive_textures_0 = emissive_textures_3;

#line 5501
    (&kernelContext_38)->cluster_lights_0 = cluster_lights_3;

#line 5501
    (&kernelContext_38)->specular_dfg_0 = specular_dfg_3;

#line 5501
    (&kernelContext_38)->lights_0 = lights_3;

#line 5501
    (&kernelContext_38)->ltc_matrix_0 = ltc_matrix_3;

#line 5501
    (&kernelContext_38)->shadow_atlas_0 = shadow_atlas_3;

#line 5501
    (&kernelContext_38)->shadow_sampler_0 = shadow_sampler_3;

#line 5501
    (&kernelContext_38)->contact_shadow_0 = contact_shadow_3;

#line 5501
    (&kernelContext_38)->probes_0 = probes_3;

#line 5501
    (&kernelContext_38)->probe_visibility_0 = probe_visibility_3;

#line 5513
    float3 vertex_normal_0 = normalize(_S269.world_normal_1);

#line 5518
    float2 motion_1 = motion_vector_0(_S269.clip_position_1, _S269.previous_clip_position_1);

#line 5534
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 5536
        float4 _S270 = occlusion_at_0(position_5.xy, &kernelContext_38);



        (&bent_0)->lit_0 = float4(_S270.yzw, 1.0f);


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

#line 5590
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5590
        float4 _S271 = occlusion_at_0(position_5.xy, &kernelContext_38);


        float value_1 = _S271.x;

#line 5592
        thread FragmentOutput_0 occlusion_1;

#line 5601
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S269.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 5618
    thread GpuMaterial_natural_0 _S272 = (&kernelContext_38)->materials_0[_S269.material_16];

#line 5618
    float2 uv_6;

#line 5643
    if(((&_S272)->tiling_0) == 1U)
    {

#line 5643
        uv_6 = physical_tile_uv_0(_S269.world_position_15, vertex_normal_0, (&_S272)->tile_metres_0);

#line 5643
    }
    else
    {

#line 5643
        uv_6 = _S269.uv_5;

#line 5643
    }

#line 5643
    float4 _S273 = base_color_texel_0(&_S272, uv_6, &kernelContext_38);

#line 5665
    float4 albedo_1 = _S269.color_3 * float4((&_S272)->base_color_0)  * _S273;

#line 5665
    bool _S274 = alpha_masked_0(&_S272, albedo_1.w);

#line 5679
    if(_S274)
    {
        discard_fragment();

#line 5679
    }

#line 5679
    float3 _S275 = double_sided_normal_0(&_S272, vertex_normal_0, front_facing_1);

#line 5679
    uint _S276 = normal_layer_0(&_S272);

#line 5679
    thread VertexOutput_0 _S277;

#line 5679
    (&_S277)->position_3 = position_5;

#line 5679
    (&_S277)->world_position_1 = _S269.world_position_15;

#line 5679
    (&_S277)->world_normal_0 = _S269.world_normal_1;

#line 5679
    (&_S277)->color_2 = _S269.color_3;

#line 5679
    (&_S277)->material_6 = _S269.material_16;

#line 5679
    (&_S277)->uv_1 = _S269.uv_5;

#line 5679
    (&_S277)->clip_position_0 = _S269.clip_position_1;

#line 5679
    (&_S277)->previous_clip_position_0 = _S269.previous_clip_position_1;

#line 5679
    (&_S277)->world_tangent_0 = _S269.world_tangent_1;

#line 5679
    (&_S277)->frame_3 = _S269.frame_4;

#line 5679
    float3 _S278 = shading_normal_of_0(_S276, (&_S272)->normal_scale_0, &_S277, _S275, uv_6, &kernelContext_38);

#line 5698
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5700
        float3 _S279 = float3(0.5f) ;

#line 5712
        (&normals_0)->lit_0 = float4(_S278 * _S279 + _S279, 1.0f);

#line 5718
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_38)->frame_0->camera_position_0.xyz - _S269.world_position_15);



    float3 _S280 = geometric_normal_of_0(_S269.world_position_15, _S275);

#line 5727
    float4 _S281 = mro_texel_0(&_S272, uv_6, &kernelContext_38);

#line 5727
    float4 _S282 = emissive_texel_0(&_S272, uv_6, &kernelContext_38);

#line 5727
    float _S283 = metallic_of_0(&_S272, _S281);

#line 5758
    float roughness_2 = clamp((&_S272)->roughness_0 * _S281.y, 0.04500000178813934f, 1.0f);
    float alpha_1 = roughness_2 * roughness_2;

#line 5792
    float _S284 = saturate(alpha_1 * alpha_1 + specular_aa_kernel_0(_S278));

#line 5792
    float3 _S285 = specular_f0_of_0(&_S272);

#line 5812
    float3 _S286 = albedo_1.xyz;

#line 5812
    float3 f0_2 = mix(_S285, _S286, float3(_S283) );
    float f90_1 = (&_S272)->specular_f90_0 + (1.0f - (&_S272)->specular_f90_0) * _S283;
    float3 _S287 = _S286 * float3((1.0f - _S283)) ;

#line 5814
    float _S288 = dielectric_diffuse_weight_0(&_S272);

#line 5814
    float3 diffuse_albedo_0 = _S287 * float3(_S288) ;

#line 5820
    float _S289 = max(dot(_S278, to_eye_1), 0.00009999999747379f);

#line 5830
    float2 _S290 = position_5.xy;

#line 5830
    uint _S291 = froxel_of_0(_S290, (((float4(_S269.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_38)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_38);

#line 5830
    uint base_2 = _S291 * 17U;

#line 5835
    uint _S292 = min((&kernelContext_38)->cluster_lights_0[base_2], 16U);

#line 5835
    TableTap_0 _S293 = table_tap_0(_S289, roughness_2, &kernelContext_38);

#line 5835
    thread TableTap_0 _S294 = _S293;

#line 5835
    float2 _S295 = dfg_at_0(&_S294, &kernelContext_38);

#line 5846
    float _S296 = _S295.x;

#line 5846
    float _S297 = _S295.y;

#line 5846
    float3 _S298 = f0_2 * float3(_S296)  + float3((f90_1 * _S297)) ;

#line 5852
    float3 _S299 = float3(0.0f, 0.0f, 0.0f);

#line 5852
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5852
    uint slot_0 = 0U;

#line 5852
    float3 direct_0 = _S299;

#line 5852
    float3 gloss_0 = _S299;

#line 5862
    for(;;)
    {

#line 5862
        if(slot_0 < _S292)
        {
        }
        else
        {

#line 5862
            break;
        }

#line 5862
        thread GpuLight_natural_0 _S300 = (&kernelContext_38)->lights_0[(&kernelContext_38)->cluster_lights_0[base_2 + 1U + slot_0]];

#line 5862
        uint _S301 = (&_S300)->kind_0;

#line 5871
        bool _S302 = ((&_S300)->kind_0) == 0U;

#line 5871
        float3 to_light_7;

#line 5871
        float reach_2;

#line 5871
        if(_S302)
        {

#line 5871
            to_light_7 = normalize((float4((&_S300)->direction_0) ).xyz);

#line 5871
            reach_2 = 1.0f;

#line 5871
        }
        else
        {


            if(_S301 == 3U)
            {

#line 5876
                float4 _S303 = float4((&_S300)->position_0) ;

#line 5884
                float3 offset_2 = _S303.xyz - _S269.world_position_15;
                float distance_3 = length(offset_2);

                float _S304 = range_window_0(distance_3, _S303.w);

#line 5887
                to_light_7 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5887
                reach_2 = _S304;

#line 5876
            }
            else
            {

#line 5876
                float4 _S305 = float4((&_S300)->position_0) ;

#line 5891
                float3 offset_3 = _S305.xyz - _S269.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_8 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S305.w);
                if(_S301 == 2U)
                {

#line 5895
                    float4 _S306 = float4((&_S300)->direction_0) ;

#line 5895
                    reach_2 = reach_3 * spot_cone_0(to_light_8, _S306.xyz, _S306.w, (&_S300)->cos_inner_0);

#line 5895
                }
                else
                {

#line 5895
                    reach_2 = reach_3;

#line 5895
                }

#line 5895
                to_light_7 = to_light_8;

#line 5876
            }

#line 5871
        }

#line 5904
        float n_dot_l_5 = dot(_S278, to_light_7);

#line 5904
        float3 specular_0;

#line 5904
        float diffuse_0;


        if(_S301 == 3U)
        {

#line 5917
            thread array<float3, int(4)> corners_2;

#line 5917
            rect_corners_0(&_S300, _S269.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S278, to_eye_1, _S289);

#line 5919
            thread array<float3, int(4)> _S307 = corners_2;

#line 5919
            float _S308 = ltc_irradiance_0(to_local_0, &_S307);

#line 5919
            thread TableTap_0 _S309 = _S293;

#line 5919
            float4 _S310 = ltc_at_0(&_S309, &kernelContext_38);

            matrix<float,int(3),int(3)>  _S311 = (((to_local_0) * (ltc_transform_0(_S310))));

#line 5921
            thread array<float3, int(4)> _S312 = corners_2;

#line 5921
            float _S313 = ltc_irradiance_0(_S311, &_S312);
            float3 _S314 = float3(_S313)  * _S298;

#line 5922
            diffuse_0 = _S308;

#line 5922
            specular_0 = _S314;

#line 5907
        }
        else
        {

#line 5927
            float _S315 = max(n_dot_l_5, 0.0f);

#line 5934
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 5942
            float3 specular_1 = ggx_lobe_0(_S284, f0_2, f90_1, _S315, _S289, max(dot(_S278, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S315) ;

#line 5942
            diffuse_0 = _S315;

#line 5942
            specular_0 = specular_1;

#line 5907
        }

#line 5907
        float3 specular_2;

#line 5950
        if((((&_S300)->flags_3) & 1U) != 0U)
        {

#line 5950
            specular_2 = _S299;

#line 5950
        }
        else
        {

#line 5950
            specular_2 = specular_0;

#line 5950
        }

#line 5950
        float reach_4;

#line 5968
        if(_S302)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 5971
            float _S316 = sun_visibility_0(_S269.world_position_15, to_light_7, n_dot_l_5, _S280, _S290, &sun_cascade_0, &sun_fade_0, &kernelContext_38);

#line 5971
            float _S317 = contact_at_0(_S290, &kernelContext_38);

#line 5980
            float _S318 = _S316 * _S317;

#line 5980
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 5980
            reach_4 = _S318;

#line 5968
        }
        else
        {

#line 5985
            if(_S301 == 1U)
            {

#line 5985
                uint _S319 = (&_S300)->shadow_tile_0;

#line 5997
                if(((&_S300)->shadow_tile_0) <= 8U)
                {

#line 5997
                    float _S320 = point_visibility_0(&_S300, _S319, _S269.world_position_15, to_light_7, n_dot_l_5, _S280, _S290, &kernelContext_38);

#line 5997
                    reach_4 = reach_2 * _S320;

#line 5997
                }
                else
                {

#line 5997
                    reach_4 = reach_2;

#line 5997
                }

#line 5985
            }
            else
            {

#line 5985
                uint _S321 = (&_S300)->shadow_tile_0;

#line 6003
                if(((&_S300)->shadow_tile_0) < 14U)
                {

#line 6003
                    float _S322 = spot_visibility_0(&_S300, _S321, _S269.world_position_15, to_light_7, n_dot_l_5, _S280, _S290, &kernelContext_38);

#line 6003
                    reach_4 = reach_2 * _S322;

#line 6003
                }
                else
                {

#line 6003
                    reach_4 = reach_2;

#line 6003
                }

#line 5985
            }

#line 5968
        }

#line 6011
        float3 _S323 = (float4((&_S300)->color_0) ).xyz;

#line 6011
        float3 direct_1 = direct_0 + _S323 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S323 * (specular_2 * float3(reach_4) );

#line 5862
        slot_0 = slot_0 + 1U;

#line 5862
        direct_0 = direct_1;

#line 5862
        gloss_0 = gloss_1;

#line 5862
    }

#line 6031
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S296 + _S297);

#line 6031
    float4 _S324 = occlusion_at_0(_S290, &kernelContext_38);

#line 6050
    float occluded_0 = _S324.x;

#line 6059
    float3 bent_normal_0 = bent_normal_at_0(_S324, _S278);

#line 6082
    float3 _S325 = frame_5->ambient_0.xyz;

#line 6082
    float3 _S326 = sky_irradiance_0(bent_normal_0, &kernelContext_38);

#line 6082
    float3 _S327 = _S325 + _S326;

#line 6082
    float3 _S328 = probe_irradiance_0(_S269.world_position_15, bent_normal_0, &kernelContext_38);

#line 6138
    float3 lit_1 = diffuse_albedo_0 * ((_S327 + _S328) * (multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) * float3(_S281.x) ) + direct_0) + gloss_2;

#line 6138
    float3 _S329 = emissive_of_0(&_S272);

#line 6180
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_38)->frame_0->fog_params_0.x, (&kernelContext_38)->frame_0->fog_params_0.y, (&kernelContext_38)->frame_0->camera_position_0.y - (&kernelContext_38)->frame_0->fog_params_0.z, _S269.world_position_15.y - (&kernelContext_38)->frame_0->fog_params_0.z, length((&kernelContext_38)->frame_0->camera_position_0.xyz - _S269.world_position_15)));
    float3 lit_2 = (lit_1 + _S329 * _S282.xyz) * float3(fog_survives_0)  + (&kernelContext_38)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;

#line 6193
    (&output_2)->lit_0 = float4(lit_2, 1.0f);

#line 6201
    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 6211
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, 1.0f);

#line 6220
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 6211
    }

#line 6222
    return output_2;
}


#line 6222
struct pixelInput_1
{
    float3 world_position_16 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_2 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_4 [[user(CRCBL_COLOR)]];
    [[flat]] uint material_17 [[user(CRCBL_MATERIAL)]];
    float2 uv_7 [[user(CRCBL_UV)]];
    float4 clip_position_2 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_2 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_2 [[user(CRCBL_WORLD_TANGENT)]];
    [[flat]] uint frame_6 [[user(CRCBL_FRAME)]];
};


#line 6255
[[fragment]] void depthMaskedFragmentMain(pixelInput_1 _S330 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_4 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_4 [[texture(9)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 6255
    thread KernelContext_0 kernelContext_39;

#line 6255
    (&kernelContext_39)->draw_0 = draw_4;

#line 6255
    (&kernelContext_39)->visible_instances_0 = visible_instances_4;

#line 6255
    (&kernelContext_39)->instances_0 = instances_4;

#line 6255
    (&kernelContext_39)->meshes_0 = meshes_4;

#line 6255
    (&kernelContext_39)->frame_0 = frame_7;

#line 6255
    (&kernelContext_39)->vertices_0 = vertices_4;

#line 6255
    (&kernelContext_39)->ambient_occlusion_0 = ambient_occlusion_4;

#line 6255
    (&kernelContext_39)->materials_0 = materials_4;

#line 6255
    (&kernelContext_39)->base_color_textures_0 = base_color_textures_4;

#line 6255
    (&kernelContext_39)->base_color_sampler_0 = base_color_sampler_4;

#line 6255
    (&kernelContext_39)->normal_textures_0 = normal_textures_4;

#line 6255
    (&kernelContext_39)->mro_textures_0 = mro_textures_4;

#line 6255
    (&kernelContext_39)->emissive_textures_0 = emissive_textures_4;

#line 6255
    (&kernelContext_39)->cluster_lights_0 = cluster_lights_4;

#line 6255
    (&kernelContext_39)->specular_dfg_0 = specular_dfg_4;

#line 6255
    (&kernelContext_39)->lights_0 = lights_4;

#line 6255
    (&kernelContext_39)->ltc_matrix_0 = ltc_matrix_4;

#line 6255
    (&kernelContext_39)->shadow_atlas_0 = shadow_atlas_4;

#line 6255
    (&kernelContext_39)->shadow_sampler_0 = shadow_sampler_4;

#line 6255
    (&kernelContext_39)->contact_shadow_0 = contact_shadow_4;

#line 6255
    (&kernelContext_39)->probes_0 = probes_4;

#line 6255
    (&kernelContext_39)->probe_visibility_0 = probe_visibility_4;

#line 6255
    thread GpuMaterial_natural_0 _S331 = materials_4[_S330.material_17];

#line 6255
    float2 uv_8;

#line 6264
    if(((&_S331)->tiling_0) == 1U)
    {

#line 6264
        uv_8 = physical_tile_uv_0(_S330.world_position_16, normalize(_S330.world_normal_2), (&_S331)->tile_metres_0);

#line 6264
    }
    else
    {

#line 6264
        uv_8 = _S330.uv_7;

#line 6264
    }

#line 6264
    float4 _S332 = base_color_texel_0(&_S331, uv_8, &kernelContext_39);

#line 6264
    bool _S333 = alpha_masked_0(&_S331, _S330.color_4.w * (float4((&_S331)->base_color_0) ).w * _S332.w);

#line 6273
    if(_S333)
    {
        discard_fragment();

#line 6273
    }



    return;
}


#line 6307
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_14 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 6307
struct pixelInput_2
{
    float3 world_position_17 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_3 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_5 [[user(CRCBL_COLOR)]];
    [[flat]] uint material_18 [[user(CRCBL_MATERIAL)]];
    float2 uv_9 [[user(CRCBL_UV)]];
    float4 clip_position_3 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_3 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_3 [[user(CRCBL_WORLD_TANGENT)]];
    [[flat]] uint frame_8 [[user(CRCBL_FRAME)]];
};


#line 6350
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_2 _S334 [[stage_in]], bool front_facing_2 [[front_facing]], float4 position_7 [[position]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_5 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_5 [[texture(9)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 6350
    thread KernelContext_0 kernelContext_40;

#line 6350
    (&kernelContext_40)->draw_0 = draw_5;

#line 6350
    (&kernelContext_40)->visible_instances_0 = visible_instances_5;

#line 6350
    (&kernelContext_40)->instances_0 = instances_5;

#line 6350
    (&kernelContext_40)->meshes_0 = meshes_5;

#line 6350
    (&kernelContext_40)->frame_0 = frame_9;

#line 6350
    (&kernelContext_40)->vertices_0 = vertices_5;

#line 6350
    (&kernelContext_40)->ambient_occlusion_0 = ambient_occlusion_5;

#line 6350
    (&kernelContext_40)->materials_0 = materials_5;

#line 6350
    (&kernelContext_40)->base_color_textures_0 = base_color_textures_5;

#line 6350
    (&kernelContext_40)->base_color_sampler_0 = base_color_sampler_5;

#line 6350
    (&kernelContext_40)->normal_textures_0 = normal_textures_5;

#line 6350
    (&kernelContext_40)->mro_textures_0 = mro_textures_5;

#line 6350
    (&kernelContext_40)->emissive_textures_0 = emissive_textures_5;

#line 6350
    (&kernelContext_40)->cluster_lights_0 = cluster_lights_5;

#line 6350
    (&kernelContext_40)->specular_dfg_0 = specular_dfg_5;

#line 6350
    (&kernelContext_40)->lights_0 = lights_5;

#line 6350
    (&kernelContext_40)->ltc_matrix_0 = ltc_matrix_5;

#line 6350
    (&kernelContext_40)->shadow_atlas_0 = shadow_atlas_5;

#line 6350
    (&kernelContext_40)->shadow_sampler_0 = shadow_sampler_5;

#line 6350
    (&kernelContext_40)->contact_shadow_0 = contact_shadow_5;

#line 6350
    (&kernelContext_40)->probes_0 = probes_5;

#line 6350
    (&kernelContext_40)->probe_visibility_0 = probe_visibility_5;

#line 6355
    float3 vertex_normal_1 = normalize(_S334.world_normal_3);

#line 6355
    thread GpuMaterial_natural_0 _S335 = materials_5[_S334.material_18];

#line 6355
    float2 uv_10;

#line 6362
    if(((&_S335)->tiling_0) == 1U)
    {

#line 6362
        uv_10 = physical_tile_uv_0(_S334.world_position_17, vertex_normal_1, (&_S335)->tile_metres_0);

#line 6362
    }
    else
    {

#line 6362
        uv_10 = _S334.uv_9;

#line 6362
    }

#line 6362
    float4 _S336 = base_color_texel_0(&_S335, uv_10, &kernelContext_40);

#line 6367
    float4 albedo_3 = _S334.color_5 * float4((&_S335)->base_color_0)  * _S336;

#line 6367
    bool _S337 = alpha_masked_0(&_S335, albedo_3.w);

#line 6373
    if(_S337)
    {
        discard_fragment();

#line 6373
    }

#line 6378
    thread RsmOutput_0 written_0;

#line 6389
    float3 _S338 = albedo_3.xyz;

#line 6389
    float4 _S339 = mro_texel_0(&_S335, uv_10, &kernelContext_40);

#line 6389
    float _S340 = metallic_of_0(&_S335, _S339);

#line 6389
    float3 _S341 = _S338 * float3((1.0f - _S340)) ;

#line 6389
    float _S342 = dielectric_diffuse_weight_0(&_S335);

#line 6388
    (&written_0)->albedo_2 = float4(_S341 * float3(_S342) , 1.0f);

#line 6388
    float3 _S343 = double_sided_normal_0(&_S335, vertex_normal_1, front_facing_2);

#line 6388
    float3 _S344 = float3(0.5f) ;

#line 6397
    (&written_0)->normal_14 = float4(_S343 * _S344 + _S344, 1.0f);

    (&written_0)->world_0 = float4(_S334.world_position_17, 1.0f);
    return written_0;
}


#line 6400
struct vertexMain_Result_0
{
    float4 position_8 [[position]];
    float3 world_position_18 [[user(CRCBL_WORLD_POSITION)]];
    float3 world_normal_4 [[user(CRCBL_WORLD_NORMAL)]];
    float4 color_6 [[user(CRCBL_COLOR)]];
    uint material_19 [[user(CRCBL_MATERIAL)]];
    float2 uv_11 [[user(CRCBL_UV)]];
    float4 clip_position_4 [[user(CRCBL_CLIP_POSITION)]];
    float4 previous_clip_position_4 [[user(CRCBL_PREVIOUS_CLIP_POSITION)]];
    float3 world_tangent_4 [[user(CRCBL_WORLD_TANGENT)]];
    uint frame_10 [[user(CRCBL_FRAME)]];
};


#line 6400
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], uint base_instance_1 [[base_instance]], DrawConstants_0 constant* draw_6 [[buffer(3)]], uint device* visible_instances_6 [[buffer(5)]], GpuInstance_natural_0 device* instances_6 [[buffer(2)]], GpuMesh_0 device* meshes_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_11 [[buffer(0)]], uint device* vertices_6 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_6 [[texture(2)]], GpuMaterial_natural_0 device* materials_6 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_6 [[texture(0)]], sampler base_color_sampler_6 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_6 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_6 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_6 [[texture(9)]], uint device* cluster_lights_6 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_6 [[texture(3)]], GpuLight_natural_0 device* lights_6 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_6 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_6 [[texture(6)]], GpuProbe_natural_0 device* probes_6 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_6 [[texture(7)]])
{

#line 6400
    thread KernelContext_0 kernelContext_41;

#line 6400
    (&kernelContext_41)->draw_0 = draw_6;

#line 6400
    (&kernelContext_41)->visible_instances_0 = visible_instances_6;

#line 6400
    (&kernelContext_41)->instances_0 = instances_6;

#line 6400
    (&kernelContext_41)->meshes_0 = meshes_6;

#line 6400
    (&kernelContext_41)->frame_0 = frame_11;

#line 6400
    (&kernelContext_41)->vertices_0 = vertices_6;

#line 6400
    (&kernelContext_41)->ambient_occlusion_0 = ambient_occlusion_6;

#line 6400
    (&kernelContext_41)->materials_0 = materials_6;

#line 6400
    (&kernelContext_41)->base_color_textures_0 = base_color_textures_6;

#line 6400
    (&kernelContext_41)->base_color_sampler_0 = base_color_sampler_6;

#line 6400
    (&kernelContext_41)->normal_textures_0 = normal_textures_6;

#line 6400
    (&kernelContext_41)->mro_textures_0 = mro_textures_6;

#line 6400
    (&kernelContext_41)->emissive_textures_0 = emissive_textures_6;

#line 6400
    (&kernelContext_41)->cluster_lights_0 = cluster_lights_6;

#line 6400
    (&kernelContext_41)->specular_dfg_0 = specular_dfg_6;

#line 6400
    (&kernelContext_41)->lights_0 = lights_6;

#line 6400
    (&kernelContext_41)->ltc_matrix_0 = ltc_matrix_6;

#line 6400
    (&kernelContext_41)->shadow_atlas_0 = shadow_atlas_6;

#line 6400
    (&kernelContext_41)->shadow_sampler_0 = shadow_sampler_6;

#line 6400
    (&kernelContext_41)->contact_shadow_0 = contact_shadow_6;

#line 6400
    (&kernelContext_41)->probes_0 = probes_6;

#line 6400
    (&kernelContext_41)->probe_visibility_0 = probe_visibility_6;

#line 6400
    uint _S345 = instance_id_1 - base_instance_1;

#line 6400
    DrawSource_0 _S346 = draw_source_0(0U, &kernelContext_41);

#line 6400
    GpuInstance_natural_0 device* _S347 = (&kernelContext_41)->instances_0+(&kernelContext_41)->visible_instances_0[_S346.run_0 + _S345];

#line 2286
    GpuMesh_0 mesh_4 = (&kernelContext_41)->meshes_0[_S346.mesh_0];

#line 2294
    bool _S348 = ((_S347->flags_0) & 2U) != 0U;

#line 2294
    uint base_vertex_3;
    if(_S348)
    {

#line 2295
        base_vertex_3 = _S347->base_vertex_0;

#line 2295
    }
    else
    {

#line 2295
        base_vertex_3 = mesh_4.base_vertex_1;

#line 2295
    }

#line 2295
    MeshVertex_0 _S349 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_4.uv_scale_u_0, mesh_4.uv_scale_v_0, mesh_4.uv_offset_u_0, mesh_4.uv_offset_v_0), &kernelContext_41);

#line 2295
    uint previous_base_0;

#line 2308
    if(_S348)
    {

#line 2308
        previous_base_0 = _S347->previous_base_vertex_0;

#line 2308
    }
    else
    {

#line 2308
        previous_base_0 = base_vertex_3;

#line 2308
    }

#line 2308
    float3 _S350 = load_position_0(index_8 + previous_base_0, &kernelContext_41);

#line 2308
    matrix<float,int(4),int(4)>  _S351 = matrix<float,int(4),int(4)> (_S347->transform_0.data_0[int(0)][int(0)], _S347->transform_0.data_0[int(1)][int(0)], _S347->transform_0.data_0[int(2)][int(0)], _S347->transform_0.data_0[int(3)][int(0)], _S347->transform_0.data_0[int(0)][int(1)], _S347->transform_0.data_0[int(1)][int(1)], _S347->transform_0.data_0[int(2)][int(1)], _S347->transform_0.data_0[int(3)][int(1)], _S347->transform_0.data_0[int(0)][int(2)], _S347->transform_0.data_0[int(1)][int(2)], _S347->transform_0.data_0[int(2)][int(2)], _S347->transform_0.data_0[int(3)][int(2)], _S347->transform_0.data_0[int(0)][int(3)], _S347->transform_0.data_0[int(1)][int(3)], _S347->transform_0.data_0[int(2)][int(3)], _S347->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S349.position_1, 1.0f)) * (_S351)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_41)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_41)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 2322
    matrix<float,int(3),int(3)>  _S352 = matrix<float,int(3),int(3)> (_S351[int(0)].xyz, _S351[int(1)].xyz, _S351[int(2)].xyz);

#line 2322
    (&output_3)->world_normal_0 = (((_S349.basis_1.normal_0) * (normal_basis_0(_S352))));

#line 2328
    (&output_3)->world_tangent_0 = (((_S349.basis_1.tangent_1) * (_S352)));

#line 2328
    thread TangentFrame_0 _S353 = _S349.basis_1;

#line 2328
    uint _S354 = frame_word_0(mesh_4.flags_1, &_S353);
    (&output_3)->frame_3 = _S354;

#line 2329
    float4 _S355;

#line 2336
    if(((&kernelContext_41)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 2336
        _S355 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 2336
    }
    else
    {

#line 2336
        _S355 = _S349.color_1;

#line 2336
    }

#line 2335
    (&output_3)->color_2 = _S355;

#line 2342
    (&output_3)->material_6 = _S347->material_0;
    (&output_3)->uv_1 = _S349.uv0_0;

#line 2349
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S350, 1.0f)) * (matrix<float,int(4),int(4)> (_S347->previous_transform_0.data_0[int(0)][int(0)], _S347->previous_transform_0.data_0[int(1)][int(0)], _S347->previous_transform_0.data_0[int(2)][int(0)], _S347->previous_transform_0.data_0[int(3)][int(0)], _S347->previous_transform_0.data_0[int(0)][int(1)], _S347->previous_transform_0.data_0[int(1)][int(1)], _S347->previous_transform_0.data_0[int(2)][int(1)], _S347->previous_transform_0.data_0[int(3)][int(1)], _S347->previous_transform_0.data_0[int(0)][int(2)], _S347->previous_transform_0.data_0[int(1)][int(2)], _S347->previous_transform_0.data_0[int(2)][int(2)], _S347->previous_transform_0.data_0[int(3)][int(2)], _S347->previous_transform_0.data_0[int(0)][int(3)], _S347->previous_transform_0.data_0[int(1)][int(3)], _S347->previous_transform_0.data_0[int(2)][int(3)], _S347->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_41)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S356 = output_3;

#line 2353
    thread vertexMain_Result_0 _S357;

#line 2353
    (&_S357)->position_8 = _S356.position_3;

#line 2353
    (&_S357)->world_position_18 = _S356.world_position_1;

#line 2353
    (&_S357)->world_normal_4 = _S356.world_normal_0;

#line 2353
    (&_S357)->color_6 = _S356.color_2;

#line 2353
    (&_S357)->material_19 = _S356.material_6;

#line 2353
    (&_S357)->uv_11 = _S356.uv_1;

#line 2353
    (&_S357)->clip_position_4 = _S356.clip_position_0;

#line 2353
    (&_S357)->previous_clip_position_4 = _S356.previous_clip_position_0;

#line 2353
    (&_S357)->world_tangent_4 = _S356.world_tangent_0;

#line 2353
    (&_S357)->frame_10 = _S356.frame_3;

#line 2353
    return _S357;
}

