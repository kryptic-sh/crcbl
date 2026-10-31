#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 3103 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f } };

#line 3098
constant array<float, int(8)> FOG_KERNEL_0 = { { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f } };

#line 4126
constant array<float3, int(2)> CASCADE_TINTS_0 = { { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) } };

#line 3370
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 3430
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

#line 3584
constant array<float2, int(16)> SHADOW_SEARCH_OFFSET_0 = { { float2(1.41421604156494141f, 0.0f), float2(-1.80617594718933105f, 1.65460801124572754f), float2(0.27646398544311523f, -3.15016794204711914f), float2(2.27656793594360352f, 2.96938395500183105f), float2(-4.17778396606445312f, -0.73899197578430176f), float2(3.95756006240844727f, -2.51747989654541016f), float2(-1.32372796535491943f, 4.92420005798339844f), float2(-2.52448797225952148f, -4.86075210571289062f), float2(5.47713613510131836f, 2.00024008750915527f), float2(-5.69804811477661133f, 2.35207200050354004f), float2(2.74683189392089844f, -5.86983203887939453f), float2(2.02983999252319336f, 6.47145605087280273f), float2(-6.11796808242797852f, -3.54548811912536621f), float2(7.17707204818725586f, -1.57785594463348389f), float2(-4.38005590438842773f, 6.23017597198486328f), float2(-1.01189601421356201f, -7.80872011184692383f) } };

#line 3445
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) } };

#line 3473
constant array<uint, int(16)> SHADOW_DITHER_0 = { { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U } };

#line 1520
struct DrawSource_0
{
    uint run_0;
    uint mesh_0;
};


#line 1404
struct DrawConstants_0
{
    uint start_at_0;
    uint mesh_1;
    uint mesh_at_0;
    uint pad1_0;
};


#line 1481
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1481
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
    float bounds_min_x_0;
    float bounds_min_y_0;
    float bounds_min_z_0;
    float bounds_max_x_0;
    float bounds_max_y_0;
    float bounds_max_z_0;
};


#line 918
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


#line 1439
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1439
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


#line 1556
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


#line 1708
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


#line 1788
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 1980
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


#line 1529
DrawSource_0 draw_source_0(uint draw_index_0, KernelContext_0 thread* kernelContext_0)
{
    thread DrawSource_0 source_0;

#line 1536
    (&source_0)->run_0 = kernelContext_0->visible_instances_0[kernelContext_0->draw_0->start_at_0];
    (&source_0)->mesh_0 = kernelContext_0->draw_0->mesh_1;

    return source_0;
}


#line 1458
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


#line 1469
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_2)
{
    uint word_3 = kernelContext_2->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1472
    float3 _S6 = load_position_0(at_1, kernelContext_2);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_2->vertices_0[word_3], kernelContext_2->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_2->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_2->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 2142
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 2275
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 2275
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 2277
        word_4 = 1U;

#line 2277
    }
    else
    {

#line 2277
        word_4 = 0U;

#line 2277
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 2281
        word_4 = word_4 | 2U;

#line 2281
    }

#line 2280
    return word_4;
}


#line 2280
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 2402
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], uint base_instance_0 [[base_instance]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_1 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_1 [[texture(9)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 2403
    thread KernelContext_0 kernelContext_3;

#line 2403
    (&kernelContext_3)->draw_0 = draw_1;

#line 2403
    (&kernelContext_3)->visible_instances_0 = visible_instances_1;

#line 2403
    (&kernelContext_3)->instances_0 = instances_1;

#line 2403
    (&kernelContext_3)->meshes_0 = meshes_1;

#line 2403
    (&kernelContext_3)->frame_0 = frame_1;

#line 2403
    (&kernelContext_3)->vertices_0 = vertices_1;

#line 2403
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2403
    (&kernelContext_3)->materials_0 = materials_1;

#line 2403
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_1;

#line 2403
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_1;

#line 2403
    (&kernelContext_3)->normal_textures_0 = normal_textures_1;

#line 2403
    (&kernelContext_3)->mro_textures_0 = mro_textures_1;

#line 2403
    (&kernelContext_3)->emissive_textures_0 = emissive_textures_1;

#line 2403
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_1;

#line 2403
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_1;

#line 2403
    (&kernelContext_3)->lights_0 = lights_1;

#line 2403
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_1;

#line 2403
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_1;

#line 2403
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_1;

#line 2403
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_1;

#line 2403
    (&kernelContext_3)->probes_0 = probes_1;

#line 2403
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_1;

#line 2403
    uint _S7 = instance_id_0 - base_instance_0;

#line 2403
    DrawSource_0 _S8 = draw_source_0(0U, &kernelContext_3);

#line 2403
    GpuInstance_natural_0 device* _S9 = (&kernelContext_3)->instances_0+(&kernelContext_3)->visible_instances_0[_S8.run_0 + _S7];



    GpuMesh_0 mesh_3 = (&kernelContext_3)->meshes_0[_S8.mesh_0];

#line 2407
    uint base_vertex_2;

#line 2413
    if(((_S9->flags_0) & 2U) != 0U)
    {

#line 2413
        base_vertex_2 = _S9->base_vertex_0;

#line 2413
    }
    else
    {

#line 2413
        base_vertex_2 = mesh_3.base_vertex_1;

#line 2413
    }

#line 2413
    matrix<float,int(4),int(4)>  _S10 = matrix<float,int(4),int(4)> (_S9->transform_0.data_0[int(0)][int(0)], _S9->transform_0.data_0[int(1)][int(0)], _S9->transform_0.data_0[int(2)][int(0)], _S9->transform_0.data_0[int(3)][int(0)], _S9->transform_0.data_0[int(0)][int(1)], _S9->transform_0.data_0[int(1)][int(1)], _S9->transform_0.data_0[int(2)][int(1)], _S9->transform_0.data_0[int(3)][int(1)], _S9->transform_0.data_0[int(0)][int(2)], _S9->transform_0.data_0[int(1)][int(2)], _S9->transform_0.data_0[int(2)][int(2)], _S9->transform_0.data_0[int(3)][int(2)], _S9->transform_0.data_0[int(0)][int(3)], _S9->transform_0.data_0[int(1)][int(3)], _S9->transform_0.data_0[int(2)][int(3)], _S9->transform_0.data_0[int(3)][int(3)]);

#line 2413
    float3 _S11 = load_position_0(index_0 + base_vertex_2, &kernelContext_3);

#line 2413
    vertexOutput_0 _S12 = { ((((((float4(_S11, 1.0f)) * (_S10)))) * (matrix<float,int(4),int(4)> ((&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_3)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S12;
}


#line 2416
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 2437
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_2 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_2 [[texture(9)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 2437
    thread KernelContext_0 kernelContext_4;

#line 2437
    (&kernelContext_4)->draw_0 = draw_2;

#line 2437
    (&kernelContext_4)->visible_instances_0 = visible_instances_2;

#line 2437
    (&kernelContext_4)->instances_0 = instances_2;

#line 2437
    (&kernelContext_4)->meshes_0 = meshes_2;

#line 2437
    (&kernelContext_4)->frame_0 = frame_2;

#line 2437
    (&kernelContext_4)->vertices_0 = vertices_2;

#line 2437
    (&kernelContext_4)->ambient_occlusion_0 = ambient_occlusion_2;

#line 2437
    (&kernelContext_4)->materials_0 = materials_2;

#line 2437
    (&kernelContext_4)->base_color_textures_0 = base_color_textures_2;

#line 2437
    (&kernelContext_4)->base_color_sampler_0 = base_color_sampler_2;

#line 2437
    (&kernelContext_4)->normal_textures_0 = normal_textures_2;

#line 2437
    (&kernelContext_4)->mro_textures_0 = mro_textures_2;

#line 2437
    (&kernelContext_4)->emissive_textures_0 = emissive_textures_2;

#line 2437
    (&kernelContext_4)->cluster_lights_0 = cluster_lights_2;

#line 2437
    (&kernelContext_4)->specular_dfg_0 = specular_dfg_2;

#line 2437
    (&kernelContext_4)->lights_0 = lights_2;

#line 2437
    (&kernelContext_4)->ltc_matrix_0 = ltc_matrix_2;

#line 2437
    (&kernelContext_4)->shadow_atlas_0 = shadow_atlas_2;

#line 2437
    (&kernelContext_4)->shadow_sampler_0 = shadow_sampler_2;

#line 2437
    (&kernelContext_4)->contact_shadow_0 = contact_shadow_2;

#line 2437
    (&kernelContext_4)->probes_0 = probes_2;

#line 2437
    (&kernelContext_4)->probe_visibility_0 = probe_visibility_2;

#line 2437
    vertexOutput_1 _S13 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S13;
}


#line 5541
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S14 = previous_0.w;

#line 5543
    if(_S14 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S14) ) * float2(0.5f, -0.5f);
}


#line 5417
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_5)
{

#line 5417
    texture2d<float, access::sample> _S15 = kernelContext_5->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S15).get_width(0)),(*((&height_0)) = (_S15).get_height(0));

    int3 _S16 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 5423
    return ((kernelContext_5->ambient_occlusion_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z)));
}


#line 5151
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S17 = axis_0.x;

#line 5155
    float _S18 = axis_0.y;

#line 5155
    bool _S19;

#line 5155
    if(_S17 >= _S18)
    {

#line 5155
        _S19 = _S17 >= (axis_0.z);

#line 5155
    }
    else
    {

#line 5155
        _S19 = false;

#line 5155
    }

#line 5155
    float2 planar_0;

#line 5155
    if(_S19)
    {

#line 5155
        planar_0 = world_position_0.zy;

#line 5155
    }
    else
    {

        if(_S18 >= (axis_0.z))
        {

#line 5159
            planar_0 = world_position_0.xz;

#line 5159
        }
        else
        {

#line 5159
            planar_0 = world_position_0.xy;

#line 5159
        }

#line 5155
    }

#line 5167
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 1126
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) & 65535U;
}


#line 1641
float4 base_color_texel_0(const GpuMaterial_natural_0 thread* material_2, float2 uv_0, KernelContext_0 thread* kernelContext_6)
{

#line 1641
    uint _S20 = base_color_layer_0(material_2);


    bool named_0 = _S20 != 65535U;

#line 1644
    uint _S21;

    if(named_0)
    {

#line 1646
        _S21 = _S20;

#line 1646
    }
    else
    {

#line 1646
        _S21 = 0U;

#line 1646
    }

#line 1646
    float3 _S22 = float3(uv_0, float(_S21));

#line 1645
    float4 texel_0 = ((kernelContext_6->base_color_textures_0).sample((kernelContext_6->base_color_sampler_0), ((_S22)).xy, uint(((_S22)).z)));

#line 1645
    float4 _S23;

    if(named_0)
    {

#line 1647
        _S23 = texel_0;

#line 1647
    }
    else
    {

#line 1647
        _S23 = float4(1.0f, 1.0f, 1.0f, 1.0f);

#line 1647
    }

#line 1647
    return _S23;
}


#line 1243
bool alpha_masked_0(const GpuMaterial_natural_0 thread* material_3, float alpha_0)
{

#line 1243
    bool _S24;

    if(((material_3->flags_2) & 1U) != 0U)
    {

#line 1245
        _S24 = alpha_0 < (material_3->alpha_cutoff_0);

#line 1245
    }
    else
    {

#line 1245
        _S24 = false;

#line 1245
    }

#line 1245
    return _S24;
}


#line 1278
float3 double_sided_normal_0(const GpuMaterial_natural_0 thread* material_4, float3 normal_2, bool front_facing_0)
{

#line 1278
    bool _S25;

    if(((material_4->flags_2) & 2U) != 0U)
    {

#line 1280
        _S25 = !front_facing_0;

#line 1280
    }
    else
    {

#line 1280
        _S25 = false;

#line 1280
    }

#line 1280
    float3 _S26;

#line 1280
    if(_S25)
    {

#line 1280
        _S26 = - normal_2;

#line 1280
    }
    else
    {

#line 1280
        _S26 = normal_2;

#line 1280
    }

#line 1280
    return _S26;
}


#line 1141
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_5)
{
    return (material_5->color_normal_pages_0) >> 16U;
}


#line 5188
float3 orthonormal_tangent_0(float3 normal_3)
{
    float _S27 = normal_3.z;

#line 5190
    float sign_z_0;

#line 5190
    if(_S27 >= 0.0f)
    {

#line 5190
        sign_z_0 = 1.0f;

#line 5190
    }
    else
    {

#line 5190
        sign_z_0 = -1.0f;

#line 5190
    }
    float a_0 = -1.0f / (sign_z_0 + _S27);
    float _S28 = normal_3.x;

#line 5192
    float _S29 = sign_z_0 * _S28;

#line 5192
    return float3(1.0f + _S29 * _S28 * a_0, _S29 * normal_3.y * a_0, - sign_z_0 * _S28);
}


#line 5242
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_4)
{
    float _S30 = duvdy_0.y;

#line 5244
    float _S31 = duvdx_0.y;

#line 5244
    float winding_0;
    if((duvdx_0.x * _S30 - duvdy_0.x * _S31) < 0.0f)
    {

#line 5245
        winding_0 = -1.0f;

#line 5245
    }
    else
    {

#line 5245
        winding_0 = 1.0f;

#line 5245
    }
    float3 tangent_2 = (float3(_S30)  * dpdx_0 - float3(_S31)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_4;

#line 5254
    float3 tangent_3 = tangent_2 - normal_4 * float3(dot(normal_4, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 5255
    float3 _S32;

#line 5264
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 5264
        _S32 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 5264
    }
    else
    {

#line 5264
        _S32 = orthonormal_tangent_0(normal_4);

#line 5264
    }

#line 5264
    (&basis_4)->tangent_1 = _S32;

    (&basis_4)->bitangent_0 = cross(normal_4, _S32);
    return basis_4;
}


#line 2159
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


#line 5324
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_5, float2 uv_2, KernelContext_0 thread* kernelContext_7)
{

#line 5336
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_2);
    float2 duvdy_1 = dfdy(uv_2);

    if(layer_0 == 65535U)
    {
        return normal_5;
    }

    thread TangentFrame_0 basis_5;

#line 5346
    uint _S33 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 5355
        (&basis_5)->normal_0 = normal_5;
        float3 tangent_4 = input_0->world_tangent_0 - normal_5 * float3(dot(normal_5, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 5357
        float3 _S34;

#line 5362
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 5362
            _S34 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 5362
        }
        else
        {

#line 5362
            _S34 = orthonormal_tangent_0(normal_5);

#line 5362
        }

#line 5362
        (&basis_5)->tangent_1 = _S34;

#line 5368
        float3 _S35 = cross((&basis_5)->normal_0, _S34);

#line 5368
        float _S36;
        if((_S33 & 2U) != 0U)
        {

#line 5369
            _S36 = -1.0f;

#line 5369
        }
        else
        {

#line 5369
            _S36 = 1.0f;

#line 5369
        }

#line 5368
        (&basis_5)->bitangent_0 = _S35 * float3(_S36) ;

#line 5347
    }
    else
    {

#line 5373
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_5);

#line 5347
    }

#line 5377
    float3 _S37 = float3(uv_2, float(layer_0));
    float3 _S38 = ((kernelContext_7->normal_textures_0).sample((kernelContext_7->base_color_sampler_0), ((_S37)).xy, uint(((_S37)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 5378
    thread float3 tangent_space_0 = _S38;
    tangent_space_0.xy = _S38.xy * float2(normal_scale_1) ;

#line 5384
    float3 _S39 = normalize(tangent_space_0);

#line 5384
    tangent_space_0 = _S39;
    return normalize(float3(_S39.x)  * (&basis_5)->tangent_1 + float3(_S39.y)  * (&basis_5)->bitangent_0 + float3(_S39.z)  * (&basis_5)->normal_0);
}


#line 3238
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 3249
    float3 _S40;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 3250
        _S40 = - facet_1;

#line 3250
    }
    else
    {

#line 3250
        _S40 = facet_1;

#line 3250
    }

#line 3250
    return _S40;
}


#line 1159
uint mro_layer_0(const GpuMaterial_natural_0 thread* material_7)
{
    return (material_7->mro_emissive_pages_0) & 65535U;
}


#line 2049
float4 mro_texel_0(const GpuMaterial_natural_0 thread* material_8, float2 uv_3, KernelContext_0 thread* kernelContext_8)
{
    float2 duvdx_2 = dfdx(uv_3);
    float2 duvdy_2 = dfdy(uv_3);

#line 2052
    uint _S41 = mro_layer_0(material_8);

    if(_S41 == 65535U)
    {
        return float4(1.0f, 1.0f, 1.0f, 1.0f);
    }

    float3 _S42 = float3(uv_3, float(_S41));

#line 2058
    return ((kernelContext_8->mro_textures_0).sample((kernelContext_8->base_color_sampler_0), ((_S42)).xy, uint(((_S42)).z), gradient2d((duvdx_2), (duvdy_2))));
}


#line 1171
uint emissive_layer_0(const GpuMaterial_natural_0 thread* material_9)
{
    return (material_9->mro_emissive_pages_0) >> 16U;
}


#line 2068
float4 emissive_texel_0(const GpuMaterial_natural_0 thread* material_10, float2 uv_4, KernelContext_0 thread* kernelContext_9)
{
    float2 duvdx_3 = dfdx(uv_4);
    float2 duvdy_3 = dfdy(uv_4);

#line 2071
    uint _S43 = emissive_layer_0(material_10);

    if(_S43 == 65535U)
    {
        return float4(1.0f, 1.0f, 1.0f, 1.0f);
    }

    float3 _S44 = float3(uv_4, float(_S43));

#line 2077
    return ((kernelContext_9->emissive_textures_0).sample((kernelContext_9->base_color_sampler_0), ((_S44)).xy, uint(((_S44)).z), gradient2d((duvdx_3), (duvdy_3))));
}


#line 2095
float metallic_of_0(const GpuMaterial_natural_0 thread* material_11, float4 mro_0)
{
    return saturate(material_11->metallic_0 * mro_0.z);
}


#line 2643
float specular_aa_kernel_0(float3 normal_6)
{
    float3 dndx_0 = dfdx(normal_6);
    float3 dndy_0 = dfdy(normal_6);


    return min(2.0f * (0.25f * (dot(dndx_0, dndx_0) + dot(dndy_0, dndy_0))), 0.18000000715255737f);
}


#line 1189
float3 specular_f0_of_0(const GpuMaterial_natural_0 thread* material_12)
{
    return float3(material_12->specular_f0_r_0, material_12->specular_f0_g_0, material_12->specular_f0_b_0);
}


#line 1189
float3 specular_f0_of_1(const GpuMaterial_natural_0 thread* material_13)
{
    return float3(material_13->specular_f0_r_0, material_13->specular_f0_g_0, material_13->specular_f0_b_0);
}


#line 2524
float dielectric_diffuse_weight_0(const GpuMaterial_natural_0 thread* material_14)
{

#line 2524
    float3 _S45 = specular_f0_of_1(material_14);


    float _S46 = max(_S45.x, max(_S45.y, _S45.z));
    if(_S46 == 0.03999999910593033f)
    {
        return 1.0f;
    }
    return max(1.0f - (_S46 - 0.03999999910593033f) / 0.95999997854232788f, 0.0f);
}


#line 4569
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_10)
{
    uint _S47 = max(kernelContext_10->frame_0->cluster_grid_0.x, 1U);
    uint _S48 = max(kernelContext_10->frame_0->cluster_grid_0.y, 1U);
    uint _S49 = max(kernelContext_10->frame_0->cluster_grid_0.z, 1U);
    uint _S50 = max(kernelContext_10->frame_0->cluster_grid_0.w, 1U);

#line 4579
    uint _S51 = uint(pixel_0.x) / _S50;

#line 4579
    uint _S52 = min(_S51, _S47 - 1U);
    uint _S53 = uint(pixel_0.y) / _S50;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4590
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S49 - 1U))) * _S48 + min(_S53, _S48 - 1U)) * _S47 + _S52;
}


#line 2670
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2691
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_11)
{

#line 2691
    texture2d<float, access::sample> _S54 = kernelContext_11->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S54).get_width(0)),(*((&height_1)) = (_S54).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2697
    float2 _S55 = float2(1.0f) ;
    float2 _S56 = extent_1 - _S55;

#line 2698
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S56);
    float2 high_1 = min(low_1 + _S55, _S56);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2716
float2 decode_dfg_pair_0(float4 texel_1)
{
    return float2(texel_1.x * 65280.0f + texel_1.y * 255.0f, texel_1.z * 65280.0f + texel_1.w * 255.0f) / float2(65535.0f) ;
}


#line 2728
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_12)
{
    int _S57 = tap_1->lo_0.x;

#line 2730
    int _S58 = tap_1->lo_0.y;

#line 2730
    int3 _S59 = int3(_S57, _S58, int(0));
    int _S60 = tap_1->hi_0.x;

#line 2731
    int3 _S61 = int3(_S60, _S58, int(0));
    float2 _S62 = float2(tap_1->weight_0.x) ;
    int _S63 = tap_1->hi_0.y;

#line 2733
    int3 _S64 = int3(_S57, _S63, int(0));
    int3 _S65 = int3(_S60, _S63, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S59)).xy), uint(((_S59)).z)))), decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S61)).xy), uint(((_S61)).z)))), _S62), mix(decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S64)).xy), uint(((_S64)).z)))), decode_dfg_pair_0(((kernelContext_12->specular_dfg_0).read(vec<uint,2>(((_S65)).xy), uint(((_S65)).z)))), _S62), float2(tap_1->weight_0.y) );
}


#line 4520
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 4536
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 4548
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 4555
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 3057
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 3057
    float4 _S66 = float4(light_0->tangent_0) ;

    float3 _S67 = _S66.xyz;

#line 3059
    float3 across_0 = _S67 * float3(_S66.w) ;

#line 3059
    float4 _S68 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S67, _S68.xyz) * float3(_S68.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S69 = centre_0 - across_0;

#line 3062
    (*corners_0)[int(0)] = _S69 - down_0;
    float3 _S70 = centre_0 + across_0;

#line 3063
    (*corners_0)[int(1)] = _S70 - down_0;
    (*corners_0)[int(2)] = _S70 + down_0;
    (*corners_0)[int(3)] = _S69 + down_0;
    return;
}


#line 2815
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_7, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_7 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2818
    float3 seed_0;
    if((abs(normal_7.z)) < 0.89999997615814209f)
    {

#line 2819
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2819
    }
    else
    {

#line 2819
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2819
    }

#line 2819
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2820
        tangent_5 = across_1 / float3(span_0) ;

#line 2820
    }
    else
    {

#line 2820
        tangent_5 = normalize(cross(seed_0, normal_7));

#line 2820
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_7, tangent_5), normal_7);
}


#line 2796
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2886
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2886
    float3 _S71 = polygon_0->corner_0[int(0)];

#line 2886
    float3 _S72 = polygon_0->corner_0[int(1)];

#line 2886
    float3 _S73 = polygon_0->corner_0[int(2)];

#line 2886
    float3 _S74 = polygon_0->corner_0[int(3)];

#line 2892
    float3 _S75 = float3(0.0f, 0.0f, 0.0f);


    float _S76 = polygon_0->corner_0[int(0)].z;

#line 2895
    int count_1;

#line 2895
    if(_S76 > 0.0f)
    {

#line 2895
        count_1 = int(1);

#line 2895
    }
    else
    {

#line 2895
        count_1 = int(0);

#line 2895
    }
    float _S77 = _S72.z;

#line 2896
    int _S78;

#line 2896
    if(_S77 > 0.0f)
    {

#line 2896
        _S78 = int(2);

#line 2896
    }
    else
    {

#line 2896
        _S78 = int(0);

#line 2896
    }

#line 2896
    int config_0 = count_1 + _S78;
    float _S79 = _S73.z;

#line 2897
    if(_S79 > 0.0f)
    {

#line 2897
        count_1 = int(4);

#line 2897
    }
    else
    {

#line 2897
        count_1 = int(0);

#line 2897
    }

#line 2897
    int config_1 = config_0 + count_1;
    float _S80 = _S74.z;

#line 2898
    if(_S80 > 0.0f)
    {

#line 2898
        count_1 = int(8);

#line 2898
    }
    else
    {

#line 2898
        count_1 = int(0);

#line 2898
    }

#line 2898
    int config_2 = config_1 + count_1;

#line 2898
    float3 l0_0;

#line 2898
    float3 l1_0;

#line 2898
    float3 l2_0;

#line 2898
    float3 l3_0;

#line 2898
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2901
        float3 _S81 = float3(_S76) ;


        float3 _S82 = float3(- _S77)  * _S71 + _S81 * _S72;
        float3 _S83 = float3(- _S80)  * _S71 + _S81 * _S74;

#line 2905
        count_1 = int(3);

#line 2905
        l0_0 = _S71;

#line 2905
        l1_0 = _S82;

#line 2905
        l2_0 = _S83;

#line 2905
        l3_0 = _S74;

#line 2905
        l4_0 = _S75;

#line 2901
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2907
            float3 _S84 = float3(_S77) ;


            float3 _S85 = float3(- _S76)  * _S72 + _S84 * _S71;
            float3 _S86 = float3(- _S79)  * _S72 + _S84 * _S73;

#line 2911
            count_1 = int(3);

#line 2911
            l0_0 = _S85;

#line 2911
            l1_0 = _S72;

#line 2911
            l2_0 = _S86;

#line 2911
            l3_0 = _S74;

#line 2911
            l4_0 = _S75;

#line 2907
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S87 = float3(- _S79)  * _S72 + float3(_S77)  * _S73;
                float3 _S88 = float3(- _S80)  * _S71 + float3(_S76)  * _S74;

#line 2917
                count_1 = int(4);

#line 2917
                l0_0 = _S71;

#line 2917
                l1_0 = _S72;

#line 2917
                l2_0 = _S87;

#line 2917
                l3_0 = _S88;

#line 2917
                l4_0 = _S75;

#line 2913
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2919
                    float3 _S89 = float3(_S79) ;


                    float3 _S90 = float3(- _S80)  * _S73 + _S89 * _S74;
                    float3 _S91 = float3(- _S77)  * _S73 + _S89 * _S72;

#line 2923
                    count_1 = int(3);

#line 2923
                    l0_0 = _S90;

#line 2923
                    l1_0 = _S91;

#line 2923
                    l2_0 = _S73;

#line 2923
                    l3_0 = _S74;

#line 2923
                    l4_0 = _S75;

#line 2919
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S92 = float3(- _S76)  * _S72 + float3(_S77)  * _S71;
                        float3 _S93 = float3(- _S80)  * _S73 + float3(_S79)  * _S74;

#line 2929
                        count_1 = int(4);

#line 2929
                        l0_0 = _S92;

#line 2929
                        l1_0 = _S72;

#line 2929
                        l2_0 = _S73;

#line 2929
                        l3_0 = _S93;

#line 2929
                        l4_0 = _S75;

#line 2925
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2931
                            float3 _S94 = float3(- _S80) ;


                            float3 _S95 = _S94 * _S71 + float3(_S76)  * _S74;
                            float3 _S96 = _S94 * _S73 + float3(_S79)  * _S74;

#line 2935
                            count_1 = int(5);

#line 2935
                            l0_0 = _S71;

#line 2935
                            l1_0 = _S72;

#line 2935
                            l2_0 = _S73;

#line 2935
                            l3_0 = _S96;

#line 2935
                            l4_0 = _S95;

#line 2931
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2937
                                float3 _S97 = float3(_S80) ;


                                float3 _S98 = float3(- _S76)  * _S74 + _S97 * _S71;
                                float3 _S99 = float3(- _S79)  * _S74 + _S97 * _S73;

#line 2941
                                count_1 = int(3);

#line 2941
                                l0_0 = _S98;

#line 2941
                                l1_0 = _S99;

#line 2941
                                l2_0 = _S74;

#line 2941
                                l3_0 = _S74;

#line 2941
                                l4_0 = _S75;

#line 2937
                            }
                            else
                            {

#line 2944
                                if(config_2 == int(9))
                                {

                                    float3 _S100 = float3(- _S77)  * _S71 + float3(_S76)  * _S72;
                                    float3 _S101 = float3(- _S79)  * _S74 + float3(_S80)  * _S73;

#line 2948
                                    count_1 = int(4);

#line 2948
                                    l0_0 = _S71;

#line 2948
                                    l1_0 = _S100;

#line 2948
                                    l2_0 = _S101;

#line 2948
                                    l3_0 = _S74;

#line 2948
                                    l4_0 = _S75;

#line 2944
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S102 = float3(- _S80)  * _S73 + float3(_S79)  * _S74;
                                        float3 _S103 = float3(- _S79)  * _S72 + float3(_S77)  * _S73;

#line 2955
                                        count_1 = int(5);

#line 2955
                                        l0_0 = _S71;

#line 2955
                                        l1_0 = _S72;

#line 2955
                                        l2_0 = _S103;

#line 2955
                                        l3_0 = _S102;

#line 2955
                                        l4_0 = _S74;

#line 2950
                                    }
                                    else
                                    {

#line 2957
                                        if(config_2 == int(12))
                                        {

                                            float3 _S104 = float3(- _S77)  * _S73 + float3(_S79)  * _S72;
                                            float3 _S105 = float3(- _S76)  * _S74 + float3(_S80)  * _S71;

#line 2961
                                            count_1 = int(4);

#line 2961
                                            l0_0 = _S105;

#line 2961
                                            l1_0 = _S104;

#line 2961
                                            l2_0 = _S73;

#line 2961
                                            l3_0 = _S74;

#line 2961
                                            l4_0 = _S75;

#line 2957
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S106 = float3(- _S79)  * _S72 + float3(_S77)  * _S73;
                                                float3 _S107 = float3(- _S77)  * _S71 + float3(_S76)  * _S72;

#line 2969
                                                count_1 = int(5);

#line 2969
                                                l0_0 = _S71;

#line 2969
                                                l1_0 = _S107;

#line 2969
                                                l2_0 = _S106;

#line 2969
                                                l3_0 = _S73;

#line 2969
                                                l4_0 = _S74;

#line 2963
                                            }
                                            else
                                            {

#line 2971
                                                if(config_2 == int(14))
                                                {

#line 2971
                                                    float3 _S108 = float3(- _S76) ;


                                                    float3 _S109 = _S108 * _S74 + float3(_S80)  * _S71;
                                                    float3 _S110 = _S108 * _S72 + float3(_S77)  * _S71;

#line 2975
                                                    count_1 = int(5);

#line 2975
                                                    l0_0 = _S110;

#line 2975
                                                    l1_0 = _S109;

#line 2971
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2977
                                                        count_1 = int(4);

#line 2977
                                                    }
                                                    else
                                                    {

#line 2977
                                                        count_1 = int(0);

#line 2977
                                                    }

#line 2977
                                                    l0_0 = _S71;

#line 2977
                                                    l1_0 = _S75;

#line 2971
                                                }

#line 2892
                                                float3 _S111 = l1_0;

#line 2892
                                                l1_0 = _S72;

#line 2892
                                                l2_0 = _S73;

#line 2892
                                                l3_0 = _S74;

#line 2892
                                                l4_0 = _S111;

#line 2963
                                            }

#line 2957
                                        }

#line 2950
                                    }

#line 2944
                                }

#line 2937
                            }

#line 2931
                        }

#line 2925
                    }

#line 2919
                }

#line 2913
            }

#line 2907
        }

#line 2901
    }

#line 2985
    if(count_1 <= int(3))
    {

#line 2985
        l3_0 = l0_0;

#line 2985
        l4_0 = l0_0;

#line 2985
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2990
            l4_0 = l0_0;

#line 2990
        }

#line 2985
    }

#line 2995
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2858
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2864
    float weight_1;

#line 2869
    if(cosine_0 > 0.0f)
    {

#line 2869
        weight_1 = fit_0;

#line 2869
    }
    else
    {

#line 2869
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2869
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 3015
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 3017
    int corner_1 = int(0);
    for(;;)
    {

#line 3018
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 3018
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 3018
        corner_1 = corner_1 + int(1);

#line 3018
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 3023
    thread LtcPolygon_0 _S112 = polygon_1;

#line 3023
    LtcPolygon_0 _S113 = ltc_clip_0(&_S112);
    polygon_1 = _S113;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 3027
    int at_2 = int(0);

    for(;;)
    {

#line 3029
        if(at_2 < int(5))
        {
        }
        else
        {

#line 3029
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 3029
        at_2 = at_2 + int(1);

#line 3029
    }

#line 3036
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 3036
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 3037
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 3037
    }
    else
    {

#line 3037
        sum_1 = sum_0;

#line 3037
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 3041
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 3041
    }

#line 3048
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2744
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_13)
{
    int _S114 = tap_2->lo_0.x;

#line 2746
    int _S115 = tap_2->lo_0.y;

#line 2746
    int3 _S116 = int3(_S114, _S115, int(0));
    int _S117 = tap_2->hi_0.x;

#line 2747
    int3 _S118 = int3(_S117, _S115, int(0));
    float4 _S119 = float4(tap_2->weight_0.x) ;
    int _S120 = tap_2->hi_0.y;

#line 2749
    int3 _S121 = int3(_S114, _S120, int(0));
    int3 _S122 = int3(_S117, _S120, int(0));

    return mix(mix(((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S116)).xy), uint(((_S116)).z))), ((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z))), _S119), mix(((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S121)).xy), uint(((_S121)).z))), ((kernelContext_13->ltc_matrix_0).read(vec<uint,2>(((_S122)).xy), uint(((_S122)).z))), _S119), float4(tap_2->weight_0.y) );
}


#line 2831
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 2567
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float f90_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 2574
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 2581
    float _S123 = 1.0f - alpha2_0;

#line 2587
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S123 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S123 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(f90_0, f90_0, f90_0) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 3661
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3661
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3721
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3693
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_16)
{
    return rect_1.x / kernelContext_16->frame_0->shadow_params_0.x;
}


#line 3289
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 3648
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_17)
{

#line 3648
    uint _S124;

    if(uint(pixel_1.x) < (kernelContext_17->frame_0->shadow_filter_0.z))
    {

#line 3650
        _S124 = kernelContext_17->frame_0->shadow_filter_0.x;

#line 3650
    }
    else
    {

#line 3650
        _S124 = kernelContext_17->frame_0->shadow_filter_0.y;

#line 3650
    }

#line 3650
    return _S124;
}


#line 3673
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_18)
{
    return kernelContext_18->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3673
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_19)
{
    return kernelContext_19->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 383
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3743
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_20)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S125 = spoke_0.x;

#line 3748
    float _S126 = rotation_0.x;

#line 3748
    float _S127 = spoke_0.y;

#line 3748
    float _S128 = rotation_0.y;


    float _S129 = ((kernelContext_20->shadow_atlas_0).sample_compare((kernelContext_20->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S125 * _S126 - _S127 * _S128, _S125 * _S128 + _S127 * _S126) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3751
    return _S129;
}


#line 3743
float tile_tap_1(float4 rect_6, float2 texel_step_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_21)
{

    float2 tile_min_1 = float2(0.5f, 0.5f) * texel_step_1;

    float _S130 = spoke_1.x;

#line 3748
    float _S131 = rotation_1.x;

#line 3748
    float _S132 = spoke_1.y;

#line 3748
    float _S133 = rotation_1.y;


    float _S134 = ((kernelContext_21->shadow_atlas_0).sample_compare((kernelContext_21->shadow_sampler_0), (atlas_uv_0(rect_6, clamp(tile_uv_2 + float2(_S130 * _S131 - _S132 * _S133, _S130 * _S133 + _S132 * _S131) * texel_step_1, tile_min_1, float2(1.0f)  - tile_min_1))), (reference_1), level((0.0f))));

#line 3751
    return _S134;
}


#line 3853
float tile_box_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, KernelContext_0 thread* kernelContext_22)
{

#line 3853
    float4 _S135 = atlas_rect_1(tile_2, kernelContext_22);


    if(atlas_rect_is_empty_0(_S135))
    {
        return 1.0f;
    }

#line 3858
    float2 _S136 = atlas_step_1(_S135, kernelContext_22);

#line 3858
    int y_1 = int(-1);

#line 3858
    float visibility_0 = 0.0f;

#line 3863
    for(;;)
    {

#line 3863
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3863
            break;
        }

#line 3863
        int x_0 = int(-1);

        for(;;)
        {

#line 3865
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3865
                break;
            }

#line 3865
            float _S137 = tile_tap_1(_S135, _S136, tile_uv_3, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_2, kernelContext_22);

            float visibility_1 = visibility_0 + _S137;

#line 3865
            x_0 = x_0 + int(1);

#line 3865
            visibility_0 = visibility_1;

#line 3865
        }

#line 3863
        y_1 = y_1 + int(1);

#line 3863
    }

#line 3871
    return visibility_0 / 9.0f;
}


#line 3606
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3906
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_2, KernelContext_0 thread* kernelContext_23)
{
    float2 texel_2 = kernelContext_23->frame_0->shadow_params_0.xy;

#line 3908
    float4 _S138 = atlas_rect_0(cascade_0, kernelContext_23);

#line 3908
    float2 _S139 = atlas_step_0(_S138, kernelContext_23);


    float2 _S140 = float2(0.5f, 0.5f) * _S139;


    float2 _S141 = float2(1.0f, 1.0f);

#line 3914
    float2 _S142 = _S141 / texel_2;

#line 3914
    uint index_2 = 0U;

#line 3914
    float sum_2 = 0.0f;

#line 3914
    float found_0 = 0.0f;



    for(;;)
    {

#line 3918
        if(index_2 < 16U)
        {
        }
        else
        {

#line 3918
            break;
        }

        float _S143 = SHADOW_SEARCH_OFFSET_0[index_2].x;

#line 3921
        float _S144 = rotation_2.x;

#line 3921
        float _S145 = SHADOW_SEARCH_OFFSET_0[index_2].y;

#line 3921
        float _S146 = rotation_2.y;

#line 3929
        int3 _S147 = int3(int2(min(atlas_uv_0(_S138, clamp(tile_uv_4 + float2(_S143 * _S144 - _S145 * _S146, _S143 * _S146 + _S145 * _S144) * _S139, _S140, float2(1.0f)  - _S140)) * _S142, _S142 - _S141)), int(0));

#line 3929
        float depth_1 = ((kernelContext_23->shadow_atlas_0).read(vec<uint,2>(((_S147)).xy), uint(((_S147)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3933
            sum_2 = sum_2 + depth_1;

#line 3933
            found_0 = found_1;

#line 3930
        }

#line 3918
        index_2 = index_2 + 1U;

#line 3918
    }

#line 3937
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3948
    float _S148 = 2.0f * kernelContext_23->frame_0->cascade_far_0[cascade_0];

#line 3948
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S148 + 40.0f);

#line 3948
    float _S149 = tile_texels_0(_S138, kernelContext_23);

    return clamp(separation_0 * 0.01999999955296516f / (_S148 / _S149), 2.0f, 8.0f);
}


#line 3773
float tile_pcf_0(uint tile_3, float2 tile_uv_5, float reference_4, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_24)
{
    float2 rotation_3 = shadow_rotation_0(pixel_3);

#line 3775
    float4 _S150 = atlas_rect_0(tile_3, kernelContext_24);

    if(atlas_rect_is_empty_0(_S150))
    {
        return 1.0f;
    }

#line 3779
    float2 _S151 = atlas_step_0(_S150, kernelContext_24);



    thread array<float, int(5)> probe_taps_0;

#line 3783
    uint spot_0 = 0U;

#line 3783
    float probe_0 = 0.0f;

    for(;;)
    {

#line 3785
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3785
            break;
        }

#line 3785
        float _S152 = tile_tap_0(_S150, _S151, tile_uv_5, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , rotation_3, reference_4, kernelContext_24);

        probe_taps_0[spot_0] = _S152;


        float probe_1 = probe_0 + _S152;

#line 3785
        spot_0 = spot_0 + 1U;

#line 3785
        probe_0 = probe_1;

#line 3785
    }

#line 3796
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3802
    uint index_3 = 1U;

#line 3802
    float visibility_2 = probe_taps_0[int(0)];

#line 3810
    for(;;)
    {

#line 3810
        if(index_3 < 23U)
        {
        }
        else
        {

#line 3810
            break;
        }

#line 3810
        float _S153 = tile_tap_0(_S150, _S151, tile_uv_5, SHADOW_DISC_0[index_3] * float2(radius_2) , rotation_3, reference_4, kernelContext_24);

        float visibility_3 = visibility_2 + _S153;

#line 3810
        index_3 = index_3 + 1U;

#line 3810
        visibility_2 = visibility_3;

#line 3810
    }

#line 3815
    float visibility_4 = visibility_2 + probe_taps_0[int(1)];

#line 3815
    float2 _S154 = float2(radius_2) ;

#line 3815
    float _S155 = tile_tap_0(_S150, _S151, tile_uv_5, float2(0.43503299355506897f, 0.75919097661972046f) * _S154, rotation_3, reference_4, kernelContext_24);


    float visibility_5 = visibility_4 + _S155 + probe_taps_0[int(2)];

#line 3818
    float _S156 = tile_tap_0(_S150, _S151, tile_uv_5, float2(0.82610201835632324f, -0.38168001174926758f) * _S154, rotation_3, reference_4, kernelContext_24);


    float visibility_6 = visibility_5 + _S156 + probe_taps_0[int(3)];

#line 3821
    float _S157 = tile_tap_0(_S150, _S151, tile_uv_5, float2(-0.31940698623657227f, -0.88803398609161377f) * _S154, rotation_3, reference_4, kernelContext_24);


    float visibility_7 = visibility_6 + _S157 + probe_taps_0[int(4)];

#line 3824
    index_3 = 30U;

#line 3824
    visibility_2 = visibility_7;
    for(;;)
    {

#line 3825
        if(index_3 < 32U)
        {
        }
        else
        {

#line 3825
            break;
        }

#line 3825
        float _S158 = tile_tap_0(_S150, _S151, tile_uv_5, SHADOW_DISC_0[index_3] * _S154, rotation_3, reference_4, kernelContext_24);

        float visibility_8 = visibility_2 + _S158;

#line 3825
        index_3 = index_3 + 1U;

#line 3825
        visibility_2 = visibility_8;

#line 3825
    }

#line 3830
    return visibility_2 / 32.0f;
}


#line 4002
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_25)
{

#line 4003
    float4 _S159 = atlas_rect_0(cascade_1, kernelContext_25);

#line 4037
    if(atlas_rect_is_empty_0(_S159))
    {


        return 1.0f;
    }
    float _S160 = 2.0f * kernelContext_25->frame_0->cascade_far_0[cascade_1];

#line 4043
    float _S161 = tile_texels_0(_S159, kernelContext_25);

#line 4043
    float texel_world_0 = _S160 / _S161;

#line 4050
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_25->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_25->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 4054
    bool _S162;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 4055
        _S162 = true;

#line 4055
    }
    else
    {

#line 4055
        _S162 = (ndc_0.z) <= 0.0f;

#line 4055
    }

#line 4055
    if(_S162)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 4065
    uint _S163 = shadow_filter_mode_0(pixel_4, kernelContext_25);

#line 4082
    if(_S163 == 2U)
    {

#line 4082
        float _S164 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_0.z, kernelContext_25);

        return _S164;
    }

#line 4084
    float radius_3;

#line 4092
    if(_S163 != 1U)
    {

#line 4092
        float _S165 = sun_penumbra_texels_0(cascade_1, tile_uv_6, ndc_0.z, shadow_rotation_0(pixel_4), kernelContext_25);

#line 4092
        radius_3 = _S165;

#line 4092
    }
    else
    {

#line 4092
        radius_3 = 2.0f;

#line 4092
    }

#line 4092
    float _S166 = tile_pcf_0(cascade_1, tile_uv_6, ndc_0.z, pixel_4, radius_3, kernelContext_25);

#line 4097
    return _S166;
}


#line 4190
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_26)
{
    uint cascade_2;

#line 4192
    bool covered_0;

#line 4201
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 4213
    float eye_distance_0 = length(world_position_5 - kernelContext_26->frame_0->camera_position_0.xyz);

#line 4213
    uint index_4 = 0U;

#line 4221
    for(;;)
    {

#line 4221
        if(index_4 < 2U)
        {
        }
        else
        {

#line 4221
            covered_0 = false;

#line 4221
            cascade_2 = 1U;

#line 4221
            break;
        }
        if(eye_distance_0 < kernelContext_26->frame_0->cascade_far_0[index_4])
        {

#line 4223
            covered_0 = true;

#line 4223
            cascade_2 = index_4;



            break;
        }

#line 4221
        index_4 = index_4 + 1U;

#line 4221
    }

#line 4230
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 4230
    }

#line 4230
    float blend_0;

#line 4242
    if((cascade_2 + 1U) < 2U)
    {

        float band_0 = kernelContext_26->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;

#line 4245
        blend_0 = saturate((eye_distance_0 - (kernelContext_26->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);

#line 4242
    }
    else
    {

#line 4242
        blend_0 = 0.0f;

#line 4242
    }

#line 4250
    *fade_0 = blend_0;

#line 4250
    uint lookups_0;

#line 4264
    if(blend_0 > 0.0f)
    {

#line 4264
        lookups_0 = 2U;

#line 4264
    }
    else
    {

#line 4264
        lookups_0 = 1U;

#line 4264
    }

#line 4264
    float visibility_9 = 1.0f;

#line 4264
    float next_0 = 1.0f;

#line 4264
    uint step_0 = 0U;



    for(;;)
    {

#line 4268
        if(step_0 < lookups_0)
        {
        }
        else
        {

#line 4268
            break;
        }

#line 4268
        float _S167 = cascade_visibility_0(cascade_2 + step_0, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_26);


        if(step_0 == 0U)
        {

#line 4271
            visibility_9 = _S167;

#line 4271
        }
        else
        {

#line 4271
            next_0 = _S167;

#line 4271
        }

#line 4268
        step_0 = step_0 + 1U;

#line 4268
    }

#line 4280
    if(lookups_0 == 1U)
    {

#line 4280
        blend_0 = visibility_9;

#line 4280
    }
    else
    {

#line 4280
        blend_0 = mix(visibility_9, next_0, blend_0);

#line 4280
    }

#line 4280
    return blend_0;
}


#line 5453
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_27)
{

#line 5453
    texture2d<float, access::sample> _S168 = kernelContext_27->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S168).get_width(0)),(*((&height_2)) = (_S168).get_height(0));

    int3 _S169 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 5459
    return ((kernelContext_27->contact_shadow_0).read(vec<uint,2>(((_S169)).xy), uint(((_S169)).z)).x);
}


#line 4157
float3 cascade_tint_0(uint cascade_3, float blend_1)
{

#line 4157
    float3 tint_0;

#line 4163
    if(0U == cascade_3)
    {

#line 4163
        tint_0 = mix(float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), CASCADE_TINTS_0[min(1U, 1U)], float3(blend_1) );

#line 4163
    }
    else
    {

#line 4163
        tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 4163
    }

#line 4163
    if(1U == cascade_3)
    {

#line 4163
        tint_0 = float3(0.34999999403953552f, 0.55000001192092896f, 1.0f);

#line 4163
    }

#line 4174
    return tint_0;
}


#line 4480
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S170 = axis_2.x;

#line 4483
    float _S171 = axis_2.y;

#line 4483
    bool _S172;

#line 4483
    if(_S170 >= _S171)
    {

#line 4483
        _S172 = _S170 >= (axis_2.z);

#line 4483
    }
    else
    {

#line 4483
        _S172 = false;

#line 4483
    }

#line 4483
    uint _S173;

#line 4483
    if(_S172)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 4485
            _S173 = 0U;

#line 4485
        }
        else
        {

#line 4485
            _S173 = 1U;

#line 4485
        }

#line 4485
        return _S173;
    }
    if(_S171 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 4489
            _S173 = 2U;

#line 4489
        }
        else
        {

#line 4489
            _S173 = 3U;

#line 4489
        }

#line 4489
        return _S173;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 4491
        _S173 = 4U;

#line 4491
    }
    else
    {

#line 4491
        _S173 = 5U;

#line 4491
    }

#line 4491
    return _S173;
}




float point_map_world_0(const GpuLight_natural_0 thread* light_1, float3 world_position_6, uint thread* face_0)
{
    float3 from_light_1 = world_position_6 - (float4(light_1->position_0) ).xyz;

#line 4504
    float _S174 = max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z));
    *face_0 = point_face_0(from_light_1);
    return 2.0f * _S174;
}


#line 370
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 4387
float punctual_visibility_0(uint tile_5, float3 world_position_7, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_28)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 4390
    float4 _S175 = atlas_rect_0(atlas_0, kernelContext_28);

    if(atlas_rect_is_empty_0(_S175))
    {


        return 1.0f;
    }

#line 4396
    float _S176 = tile_texels_0(_S175, kernelContext_28);

    float texel_world_1 = map_world_0 / _S176;

#line 4408
    float4 clip_1 = (((float4(world_position_7 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 4415
    float _S177 = clip_1.w;

#line 4415
    if(_S177 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S177) ;

#line 4419
    bool _S178;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 4420
        _S178 = true;

#line 4420
    }
    else
    {

#line 4420
        _S178 = (ndc_1.z) <= 0.0f;

#line 4420
    }

#line 4420
    if(_S178)
    {

#line 4420
        _S178 = true;

#line 4420
    }
    else
    {

#line 4420
        _S178 = (ndc_1.z) > 1.0f;

#line 4420
    }

#line 4420
    if(_S178)
    {

#line 4427
        return 1.0f;
    }



    float2 tile_uv_7 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 4432
    uint _S179 = shadow_filter_mode_0(pixel_6, kernelContext_28);

#line 4441
    if(_S179 == 2U)
    {

#line 4441
        float _S180 = tile_box_pcf_0(atlas_0, tile_uv_7, ndc_1.z, kernelContext_28);

        return _S180;
    }

#line 4443
    float _S181 = tile_pcf_0(atlas_0, tile_uv_7, ndc_1.z, pixel_6, 2.0f, kernelContext_28);

    return _S181;
}



float spot_map_world_0(const GpuLight_natural_0 thread* light_2, float3 world_position_8)
{

#line 4450
    float4 _S182 = float4(light_2->direction_0) ;

#line 4456
    float cos_outer_1 = _S182.w;

#line 4463
    return 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S182.xyz)), 0.0f);
}


#line 2772
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 5440
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 5442
    float3 _S183;
    if((length(decoded_0)) < 0.5f)
    {

#line 5443
        _S183 = shading_normal_1;

#line 5443
    }
    else
    {

#line 5443
        _S183 = normalize(decoded_0);

#line 5443
    }

#line 5443
    return _S183;
}


#line 5078
float3 sky_irradiance_0(float3 normal_8, KernelContext_0 thread* kernelContext_29)
{
    float4 basis_6 = float4(normal_8, 1.0f);
    return max(float3(dot(kernelContext_29->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_29->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_29->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4979
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4979
    float reach_0 = 0.0f;

#line 4979
    uint axis_3 = 0U;


    for(;;)
    {

#line 4982
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4982
            break;
        }

#line 4982
        uint _S184 = axis_3;

#line 4982
        bool _S185;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4984
            _S185 = true;

#line 4984
        }
        else
        {

#line 4984
            _S185 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4984
        }

#line 4984
        if(_S185)
        {

#line 4985
            axis_3 = axis_3 + 1U;

#line 4982
            continue;
        }

#line 4982
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S184] - 1.0f));

#line 4982
        axis_3 = axis_3 + 1U;

#line 4982
    }

#line 4989
    return reach_0;
}


#line 5009
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 5009
    uint level_0 = 0U;

    for(;;)
    {

#line 5011
        uint _S186 = level_0 + 1U;

#line 5011
        if(_S186 < levels_0)
        {
        }
        else
        {

#line 5011
            break;
        }
        float _S187 = float(level_0);

#line 5013
        float at_3 = reach_1 * exp2(- _S187);
        if(at_3 < 1.0f)
        {

#line 5015
            return float2(_S187, saturate((1.0f - at_3) / 0.25f));
        }

#line 5011
        level_0 = _S186;

#line 5011
    }

#line 5017
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4765
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4767
    uint _S188;
    if(at_4 >= count_2)
    {

#line 4768
        _S188 = at_4 - count_2;

#line 4768
    }
    else
    {

#line 4768
        _S188 = at_4;

#line 4768
    }

#line 4768
    return _S188;
}


#line 4791
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_30)
{
    uint3 counts_0 = kernelContext_30->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_30->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S189 = counts_0.x;
    uint _S190 = counts_0.y;



    return min(kernelContext_30->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S190 + probe_wrap_0(cell_2.y, offset_1.y, _S190)) * _S189 + probe_wrap_0(cell_2.x, offset_1.x, _S189), max(kernelContext_30->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4632
float sign_not_zero_0(float value_0)
{

#line 4632
    float _S191;

    if(value_0 >= 0.0f)
    {

#line 4634
        _S191 = 1.0f;

#line 4634
    }
    else
    {

#line 4634
        _S191 = -1.0f;

#line 4634
    }

#line 4634
    return _S191;
}


#line 4651
float2 oct_encode_0(float3 direction_1)
{
    float _S192 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S192) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4654
    float2 p_1;
    if(_S192 < 0.0f)
    {
        float _S193 = p_0.y;

#line 4657
        float _S194 = p_0.x;

#line 4657
        p_1 = float2((1.0f - abs(_S193)) * sign_not_zero_0(_S194), (1.0f - abs(_S194)) * sign_not_zero_0(_S193));

#line 4655
    }
    else
    {

#line 4655
        p_1 = p_0;

#line 4655
    }

#line 4660
    return p_1;
}


#line 4680
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_31)
{

#line 4680
    texture2d_array<float, access::sample> _S195 = kernelContext_31->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S195).get_width(0)),(*((&height_3)) = (_S195).get_height(0)),(*((&layers_0)) = (_S195).get_array_size());

#line 4685
    float2 _S196 = float2(0.5f) ;

#line 4685
    float2 _S197 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S196 + _S196) * float2(16.0f)  + _S197 - _S196;
    float2 _S198 = float2(float(width_3), float(height_3)) - _S197;

#line 4689
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S198);
    float2 high_2 = min(low_2 + _S197, _S198);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S199 = int(low_2.x);

#line 4694
    int _S200 = int(low_2.y);

#line 4694
    int4 _S201 = int4(_S199, _S200, layer_1, int(0));
    int _S202 = int(high_2.x);

#line 4695
    int4 _S203 = int4(_S202, _S200, layer_1, int(0));
    int _S204 = int(high_2.y);

#line 4696
    int4 _S205 = int4(_S199, _S204, layer_1, int(0));
    int4 _S206 = int4(_S202, _S204, layer_1, int(0));
    float2 _S207 = float2(weight_2.x) ;

#line 4698
    return mix(mix(((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S201)).xy), uint(((_S201)).z), uint(((_S201)).w))).xy, ((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S203)).xy), uint(((_S203)).z), uint(((_S203)).w))).xy, _S207), mix(((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S205)).xy), uint(((_S205)).z), uint(((_S205)).w))).xy, ((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S206)).xy), uint(((_S206)).z), uint(((_S206)).w))).xy, _S207), float2(weight_2.y) );
}


#line 4726
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_9, KernelContext_0 thread* kernelContext_32)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_9 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4729
    float2 _S208 = probe_moments_0(index_6, - to_probe_0, kernelContext_32);

#line 4735
    float _S209 = _S208.x;

#line 4735
    float _S210 = max(_S208.y - _S209 * _S209, 0.0f);
    float behind_0 = to_surface_0 - _S209;
    float bound_0 = _S210 / (_S210 + behind_0 * behind_0);

#line 4737
    float _S211;
    if(to_surface_0 <= _S209)
    {

#line 4738
        _S211 = 1.0f;

#line 4738
    }
    else
    {

#line 4738
        _S211 = bound_0 * bound_0 * bound_0;

#line 4738
    }

#line 4738
    return _S211;
}


#line 4748
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_10, KernelContext_0 thread* kernelContext_33)
{

#line 4748
    float _S212 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_10, kernelContext_33);

    return max(_S212, 0.00009999999747379f);
}


#line 1294
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4810
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4837
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_11, KernelContext_0 thread* kernelContext_34)
{

#line 4838
    uint _S213 = probe_row_0(level_2, cell_3, kernelContext_34);


    GpuProbe_natural_0 stored_0 = kernelContext_34->probes_0[_S213];

#line 4841
    float _S214 = probe_weight_0(_S213, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_11, kernelContext_34);



    thread WeightedProbe_0 corner_2;

#line 4845
    float4 _S215 = float4(_S214) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S215;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S215;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S215;
    (&corner_2)->weight_3 = _S214;
    return corner_2;
}


#line 4821
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S216 = float4(t_1) ;

#line 4824
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S216);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S216);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S216);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4910
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_12, KernelContext_0 thread* kernelContext_35)
{

#line 4910
    float3 _S217 = float3(1.0f) ;

#line 4915
    float3 _S218 = float3(0.0f, 0.0f, 0.0f);

#line 4915
    float3 last_1 = max(float3(kernelContext_35->frame_0->probe_counts_0.xyz) - _S217, _S218);



    float3 origin_2 = kernelContext_35->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_35->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S218, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;

    uint3 _S219 = uint3(base_0);



    uint3 _S220 = uint3(min(base_0 + _S217, last_1));

#line 4935
    float _S221 = inv_0.x;

#line 4935
    float _S222;

#line 4935
    if(_S221 != 0.0f)
    {

#line 4935
        _S222 = 1.0f / _S221;

#line 4935
    }
    else
    {

#line 4935
        _S222 = 0.0f;

#line 4935
    }
    float _S223 = inv_0.y;

#line 4936
    float _S224;

#line 4936
    if(_S223 != 0.0f)
    {

#line 4936
        _S224 = 1.0f / _S223;

#line 4936
    }
    else
    {

#line 4936
        _S224 = 0.0f;

#line 4936
    }
    float _S225 = inv_0.z;

#line 4937
    float _S226;

#line 4937
    if(_S225 != 0.0f)
    {

#line 4937
        _S226 = 1.0f / _S225;

#line 4937
    }
    else
    {

#line 4937
        _S226 = 0.0f;

#line 4937
    }

#line 4935
    float3 spacing_1 = float3(_S222, _S224, _S226);

#line 4944
    uint _S227 = _S219.x;

#line 4944
    uint _S228 = _S219.y;

#line 4944
    uint _S229 = _S219.z;

#line 4944
    WeightedProbe_0 _S230 = probe_corner_0(level_3, uint3(_S227, _S228, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);
    uint _S231 = _S220.x;

#line 4945
    WeightedProbe_0 _S232 = probe_corner_0(level_3, uint3(_S231, _S228, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4945
    float _S233 = f_0.x;

#line 4945
    thread WeightedProbe_0 _S234 = _S230;

#line 4945
    thread WeightedProbe_0 _S235 = _S232;

#line 4945
    WeightedProbe_0 _S236 = lerp_probe_0(&_S234, &_S235, _S233);
    uint _S237 = _S220.y;

#line 4946
    WeightedProbe_0 _S238 = probe_corner_0(level_3, uint3(_S227, _S237, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4946
    WeightedProbe_0 _S239 = probe_corner_0(level_3, uint3(_S231, _S237, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4946
    thread WeightedProbe_0 _S240 = _S238;

#line 4946
    thread WeightedProbe_0 _S241 = _S239;

#line 4946
    WeightedProbe_0 _S242 = lerp_probe_0(&_S240, &_S241, _S233);

    uint _S243 = _S220.z;

#line 4948
    WeightedProbe_0 _S244 = probe_corner_0(level_3, uint3(_S227, _S228, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4948
    WeightedProbe_0 _S245 = probe_corner_0(level_3, uint3(_S231, _S228, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4948
    thread WeightedProbe_0 _S246 = _S244;

#line 4948
    thread WeightedProbe_0 _S247 = _S245;

#line 4948
    WeightedProbe_0 _S248 = lerp_probe_0(&_S246, &_S247, _S233);

#line 4948
    WeightedProbe_0 _S249 = probe_corner_0(level_3, uint3(_S227, _S237, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4948
    WeightedProbe_0 _S250 = probe_corner_0(level_3, uint3(_S231, _S237, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4948
    thread WeightedProbe_0 _S251 = _S249;

#line 4948
    thread WeightedProbe_0 _S252 = _S250;

#line 4948
    WeightedProbe_0 _S253 = lerp_probe_0(&_S251, &_S252, _S233);



    float _S254 = f_0.y;

#line 4952
    thread WeightedProbe_0 _S255 = _S236;

#line 4952
    thread WeightedProbe_0 _S256 = _S242;

#line 4952
    WeightedProbe_0 _S257 = lerp_probe_0(&_S255, &_S256, _S254);

#line 4952
    thread WeightedProbe_0 _S258 = _S248;

#line 4952
    thread WeightedProbe_0 _S259 = _S253;

#line 4952
    WeightedProbe_0 _S260 = lerp_probe_0(&_S258, &_S259, _S254);

    float _S261 = f_0.z;

#line 4954
    thread WeightedProbe_0 _S262 = _S257;

#line 4954
    thread WeightedProbe_0 _S263 = _S260;

#line 4954
    WeightedProbe_0 _S264 = lerp_probe_0(&_S262, &_S263, _S261);

    float4 basis_7 = float4(normal_12, 1.0f);
    return max(float3(dot(_S264.sh_0.sh_r_0, basis_7), dot(_S264.sh_0.sh_g_0, basis_7), dot(_S264.sh_0.sh_b_0, basis_7)) / float3(_S264.weight_3) , _S218);
}


#line 5044
float3 probe_irradiance_0(float3 world_position_14, float3 normal_13, KernelContext_0 thread* kernelContext_36)
{
    if((kernelContext_36->frame_0->probe_counts_0.w) == 0U)
    {

#line 5047
        return float3(0.0f, 0.0f, 0.0f);
    }

#line 5055
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_36->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_36->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_36->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_36->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 5057
    float3 _S265 = probe_level_irradiance_0(level_4, world_position_14, normal_13, kernelContext_36);


    if(share_0 >= 1.0f)
    {

#line 5061
        return _S265;
    }

#line 5061
    float3 _S266 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_13, kernelContext_36);

    return _S266 * float3((1.0f - share_0))  + _S265 * float3(share_0) ;
}


#line 5509
float3 multi_bounce_occlusion_0(float visibility_10, float3 albedo_0)
{

#line 5509
    float3 _S267 = float3(visibility_10) ;

#line 5515
    return min(float3(1.0f) , max(_S267, ((_S267 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S267 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S267));
}


#line 1181
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_15)
{
    return float3(material_15->emissive_r_0, material_15->emissive_g_0, material_15->emissive_b_0);
}


#line 3123
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S268 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 3131
    float kernel_0 = 0.0001984127011383f;

#line 3131
    int term_0 = int(6);

    for(;;)
    {

#line 3133
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 3133
            break;
        }
        float _S269 = kernel_0 * _S268 + FOG_KERNEL_0[term_0];

#line 3133
        int term_1 = term_0 - int(1);

#line 3133
        kernel_0 = _S269;

#line 3133
        term_0 = term_1;

#line 3133
    }

#line 3140
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 3150
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S270 = - d_0;

#line 3154
        float series_0 = 0.00833333376795053f;

#line 3154
        int term_2 = int(3);

        for(;;)
        {

#line 3156
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 3156
                break;
            }
            float _S271 = series_0 * _S270 + FOG_RATIO_KERNEL_0[term_2];

#line 3156
            int term_3 = term_2 - int(1);

#line 3156
            series_0 = _S271;

#line 3156
            term_2 = term_3;

#line 3156
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 3184
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 3195
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 3203
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 5104
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 5104
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


#line 5551
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S272 [[stage_in]], bool front_facing_1 [[front_facing]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_3 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_3 [[texture(9)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 5551
    thread KernelContext_0 kernelContext_37;

#line 5551
    (&kernelContext_37)->draw_0 = draw_3;

#line 5551
    (&kernelContext_37)->visible_instances_0 = visible_instances_3;

#line 5551
    (&kernelContext_37)->instances_0 = instances_3;

#line 5551
    (&kernelContext_37)->meshes_0 = meshes_3;

#line 5551
    (&kernelContext_37)->frame_0 = frame_5;

#line 5551
    (&kernelContext_37)->vertices_0 = vertices_3;

#line 5551
    (&kernelContext_37)->ambient_occlusion_0 = ambient_occlusion_3;

#line 5551
    (&kernelContext_37)->materials_0 = materials_3;

#line 5551
    (&kernelContext_37)->base_color_textures_0 = base_color_textures_3;

#line 5551
    (&kernelContext_37)->base_color_sampler_0 = base_color_sampler_3;

#line 5551
    (&kernelContext_37)->normal_textures_0 = normal_textures_3;

#line 5551
    (&kernelContext_37)->mro_textures_0 = mro_textures_3;

#line 5551
    (&kernelContext_37)->emissive_textures_0 = emissive_textures_3;

#line 5551
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_3;

#line 5551
    (&kernelContext_37)->specular_dfg_0 = specular_dfg_3;

#line 5551
    (&kernelContext_37)->lights_0 = lights_3;

#line 5551
    (&kernelContext_37)->ltc_matrix_0 = ltc_matrix_3;

#line 5551
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_3;

#line 5551
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_3;

#line 5551
    (&kernelContext_37)->contact_shadow_0 = contact_shadow_3;

#line 5551
    (&kernelContext_37)->probes_0 = probes_3;

#line 5551
    (&kernelContext_37)->probe_visibility_0 = probe_visibility_3;

#line 5563
    float3 vertex_normal_0 = normalize(_S272.world_normal_1);

#line 5568
    float2 motion_1 = motion_vector_0(_S272.clip_position_1, _S272.previous_clip_position_1);

#line 5584
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 5586
        float4 _S273 = occlusion_at_0(position_5.xy, &kernelContext_37);



        (&bent_0)->lit_0 = float4(_S273.yzw, 1.0f);


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

#line 5640
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5640
        float4 _S274 = occlusion_at_0(position_5.xy, &kernelContext_37);


        float value_1 = _S274.x;

#line 5642
        thread FragmentOutput_0 occlusion_1;

#line 5651
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_1;



        (&tint_1)->lit_0 = float4(_S272.color_3.xyz, 1.0f);
        (&tint_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_1)->motion_0 = motion_1;
        return tint_1;
    }

#line 5668
    thread GpuMaterial_natural_0 _S275 = (&kernelContext_37)->materials_0[_S272.material_16];

#line 5668
    float2 uv_6;

#line 5693
    if(((&_S275)->tiling_0) == 1U)
    {

#line 5693
        uv_6 = physical_tile_uv_0(_S272.world_position_15, vertex_normal_0, (&_S275)->tile_metres_0);

#line 5693
    }
    else
    {

#line 5693
        uv_6 = _S272.uv_5;

#line 5693
    }

#line 5693
    float4 _S276 = base_color_texel_0(&_S275, uv_6, &kernelContext_37);

#line 5715
    float4 albedo_1 = _S272.color_3 * float4((&_S275)->base_color_0)  * _S276;

#line 5715
    bool _S277 = alpha_masked_0(&_S275, albedo_1.w);

#line 5729
    if(_S277)
    {
        discard_fragment();

#line 5729
    }

#line 5729
    float3 _S278 = double_sided_normal_0(&_S275, vertex_normal_0, front_facing_1);

#line 5729
    uint _S279 = normal_layer_0(&_S275);

#line 5729
    thread VertexOutput_0 _S280;

#line 5729
    (&_S280)->position_3 = position_5;

#line 5729
    (&_S280)->world_position_1 = _S272.world_position_15;

#line 5729
    (&_S280)->world_normal_0 = _S272.world_normal_1;

#line 5729
    (&_S280)->color_2 = _S272.color_3;

#line 5729
    (&_S280)->material_6 = _S272.material_16;

#line 5729
    (&_S280)->uv_1 = _S272.uv_5;

#line 5729
    (&_S280)->clip_position_0 = _S272.clip_position_1;

#line 5729
    (&_S280)->previous_clip_position_0 = _S272.previous_clip_position_1;

#line 5729
    (&_S280)->world_tangent_0 = _S272.world_tangent_1;

#line 5729
    (&_S280)->frame_3 = _S272.frame_4;

#line 5729
    float3 _S281 = shading_normal_of_0(_S279, (&_S275)->normal_scale_0, &_S280, _S278, uv_6, &kernelContext_37);

#line 5748
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5750
        float3 _S282 = float3(0.5f) ;

#line 5762
        (&normals_0)->lit_0 = float4(_S281 * _S282 + _S282, 1.0f);

#line 5768
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_37)->frame_0->camera_position_0.xyz - _S272.world_position_15);



    float3 _S283 = geometric_normal_of_0(_S272.world_position_15, _S278);

#line 5777
    float4 _S284 = mro_texel_0(&_S275, uv_6, &kernelContext_37);

#line 5777
    float4 _S285 = emissive_texel_0(&_S275, uv_6, &kernelContext_37);

#line 5777
    float _S286 = metallic_of_0(&_S275, _S284);

#line 5808
    float roughness_2 = clamp((&_S275)->roughness_0 * _S284.y, 0.04500000178813934f, 1.0f);
    float alpha_1 = roughness_2 * roughness_2;

#line 5842
    float _S287 = saturate(alpha_1 * alpha_1 + specular_aa_kernel_0(_S281));

#line 5842
    float3 _S288 = specular_f0_of_0(&_S275);

#line 5862
    float3 _S289 = albedo_1.xyz;

#line 5862
    float3 f0_2 = mix(_S288, _S289, float3(_S286) );
    float f90_1 = (&_S275)->specular_f90_0 + (1.0f - (&_S275)->specular_f90_0) * _S286;
    float3 _S290 = _S289 * float3((1.0f - _S286)) ;

#line 5864
    float _S291 = dielectric_diffuse_weight_0(&_S275);

#line 5864
    float3 diffuse_albedo_0 = _S290 * float3(_S291) ;

#line 5870
    float _S292 = max(dot(_S281, to_eye_1), 0.00009999999747379f);

#line 5880
    float2 _S293 = position_5.xy;

#line 5880
    uint _S294 = froxel_of_0(_S293, (((float4(_S272.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_37);

#line 5880
    uint base_1 = _S294 * 17U;

#line 5885
    uint _S295 = min((&kernelContext_37)->cluster_lights_0[base_1], 16U);

#line 5885
    TableTap_0 _S296 = table_tap_0(_S292, roughness_2, &kernelContext_37);

#line 5885
    thread TableTap_0 _S297 = _S296;

#line 5885
    float2 _S298 = dfg_at_0(&_S297, &kernelContext_37);

#line 5896
    float _S299 = _S298.x;

#line 5896
    float _S300 = _S298.y;

#line 5896
    float3 _S301 = f0_2 * float3(_S299)  + float3((f90_1 * _S300)) ;

#line 5902
    float3 _S302 = float3(0.0f, 0.0f, 0.0f);

#line 5902
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5902
    uint slot_0 = 0U;

#line 5902
    float3 direct_0 = _S302;

#line 5902
    float3 gloss_0 = _S302;

#line 5912
    for(;;)
    {

#line 5912
        if(slot_0 < _S295)
        {
        }
        else
        {

#line 5912
            break;
        }

#line 5912
        thread GpuLight_natural_0 _S303 = (&kernelContext_37)->lights_0[(&kernelContext_37)->cluster_lights_0[base_1 + 1U + slot_0]];

#line 5923
        bool fill_0 = (((&_S303)->flags_3) & 1U) != 0U;

#line 5923
        uint _S304 = (&_S303)->kind_0;

#line 5930
        bool _S305 = ((&_S303)->kind_0) == 0U;

#line 5930
        float3 to_light_5;

#line 5930
        float reach_2;

#line 5930
        if(_S305)
        {

#line 5930
            to_light_5 = normalize((float4((&_S303)->direction_0) ).xyz);

#line 5930
            reach_2 = 1.0f;

#line 5930
        }
        else
        {


            if(_S304 == 3U)
            {

#line 5935
                float4 _S306 = float4((&_S303)->position_0) ;

#line 5943
                float3 offset_2 = _S306.xyz - _S272.world_position_15;
                float distance_3 = length(offset_2);

                float _S307 = range_window_0(distance_3, _S306.w);

#line 5946
                to_light_5 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5946
                reach_2 = _S307;

#line 5935
            }
            else
            {

#line 5935
                float4 _S308 = float4((&_S303)->position_0) ;

#line 5950
                float3 offset_3 = _S308.xyz - _S272.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_6 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S308.w);
                if(_S304 == 2U)
                {

#line 5954
                    float4 _S309 = float4((&_S303)->direction_0) ;

#line 5954
                    reach_2 = reach_3 * spot_cone_0(to_light_6, _S309.xyz, _S309.w, (&_S303)->cos_inner_0);

#line 5954
                }
                else
                {

#line 5954
                    reach_2 = reach_3;

#line 5954
                }

#line 5954
                to_light_5 = to_light_6;

#line 5935
            }

#line 5930
        }

#line 5963
        float n_dot_l_3 = dot(_S281, to_light_5);

#line 5963
        float3 specular_0;

#line 5963
        float diffuse_0;


        if(_S304 == 3U)
        {

#line 5976
            thread array<float3, int(4)> corners_2;

#line 5976
            rect_corners_0(&_S303, _S272.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S281, to_eye_1, _S292);

#line 5978
            thread array<float3, int(4)> _S310 = corners_2;

#line 5978
            float _S311 = ltc_irradiance_0(to_local_0, &_S310);

            if(fill_0)
            {

#line 5980
                specular_0 = _S302;

#line 5980
            }
            else
            {

#line 5980
                thread TableTap_0 _S312 = _S296;

#line 5980
                float4 _S313 = ltc_at_0(&_S312, &kernelContext_37);

#line 5986
                matrix<float,int(3),int(3)>  _S314 = (((to_local_0) * (ltc_transform_0(_S313))));

#line 5986
                thread array<float3, int(4)> _S315 = corners_2;

#line 5986
                float _S316 = ltc_irradiance_0(_S314, &_S315);

#line 5986
                specular_0 = float3(_S316)  * _S301;

#line 5980
            }

#line 5980
            diffuse_0 = _S311;

#line 5966
        }
        else
        {

#line 5993
            float _S317 = max(n_dot_l_3, 0.0f);

            if(fill_0)
            {

#line 5995
                specular_0 = _S302;

#line 5995
            }
            else
            {

#line 6006
                float3 half_vector_0 = normalize(to_light_5 + to_eye_1);

#line 6006
                specular_0 = ggx_lobe_0(_S287, f0_2, f90_1, _S317, _S292, max(dot(_S281, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S317) ;

#line 5995
            }

#line 5995
            diffuse_0 = _S317;

#line 5966
        }

#line 5966
        float reach_4;

#line 6031
        if(_S305)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 6034
            float _S318 = sun_visibility_0(_S272.world_position_15, to_light_5, n_dot_l_3, _S283, _S293, &sun_cascade_0, &sun_fade_0, &kernelContext_37);

#line 6034
            float _S319 = contact_at_0(_S293, &kernelContext_37);

#line 6043
            float _S320 = _S318 * _S319;

#line 6043
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 6043
            reach_4 = _S320;

#line 6031
        }
        else
        {

#line 6054
            if(_S304 == 1U)
            {

#line 6054
                uint _S321 = (&_S303)->shadow_tile_0;

#line 6066
                if(((&_S303)->shadow_tile_0) <= 8U)
                {



                    if(n_dot_l_3 > 0.0f)
                    {
                        thread uint face_1;

#line 6073
                        float _S322 = point_map_world_0(&_S303, _S272.world_position_15, &face_1);

#line 6073
                        float _S323 = punctual_visibility_0(_S321 + face_1, _S272.world_position_15, to_light_5, n_dot_l_3, _S322, _S283, _S293, &kernelContext_37);

#line 6073
                        reach_4 = reach_2 * _S323;

#line 6071
                    }
                    else
                    {

#line 6071
                        reach_4 = reach_2;

#line 6071
                    }

#line 6066
                }
                else
                {

#line 6066
                    reach_4 = reach_2;

#line 6066
                }

#line 6054
            }
            else
            {

#line 6054
                uint _S324 = (&_S303)->shadow_tile_0;

#line 6082
                if(((&_S303)->shadow_tile_0) < 14U)
                {
                    if(n_dot_l_3 > 0.0f)
                    {

#line 6084
                        float _S325 = spot_map_world_0(&_S303, _S272.world_position_15);

#line 6084
                        float _S326 = punctual_visibility_0(_S324, _S272.world_position_15, to_light_5, n_dot_l_3, _S325, _S283, _S293, &kernelContext_37);

#line 6084
                        reach_4 = reach_2 * _S326;

#line 6084
                    }
                    else
                    {

#line 6084
                        reach_4 = reach_2;

#line 6084
                    }

#line 6082
                }
                else
                {

#line 6082
                    reach_4 = reach_2;

#line 6082
                }

#line 6054
            }

#line 6031
        }

#line 6094
        float3 _S327 = (float4((&_S303)->color_0) ).xyz;

#line 6094
        float3 direct_1 = direct_0 + _S327 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S327 * (specular_0 * float3(reach_4) );

#line 5912
        slot_0 = slot_0 + 1U;

#line 5912
        direct_0 = direct_1;

#line 5912
        gloss_0 = gloss_1;

#line 5912
    }

#line 6114
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S299 + _S300);

#line 6114
    float4 _S328 = occlusion_at_0(_S293, &kernelContext_37);

#line 6133
    float occluded_0 = _S328.x;

#line 6142
    float3 bent_normal_0 = bent_normal_at_0(_S328, _S281);

#line 6166
    float3 _S329 = frame_5->ambient_0.xyz;

#line 6166
    float3 _S330 = sky_irradiance_0(bent_normal_0, &kernelContext_37);

#line 6166
    float3 _S331 = _S329 + _S330;

#line 6166
    float3 _S332 = probe_irradiance_0(_S272.world_position_15, bent_normal_0, &kernelContext_37);

#line 6222
    float3 lit_1 = diffuse_albedo_0 * ((_S331 + _S332) * (multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) * float3(_S284.x) ) + direct_0) + gloss_2;

#line 6222
    float3 _S333 = emissive_of_0(&_S275);

#line 6264
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_37)->frame_0->fog_params_0.x, (&kernelContext_37)->frame_0->fog_params_0.y, (&kernelContext_37)->frame_0->camera_position_0.y - (&kernelContext_37)->frame_0->fog_params_0.z, _S272.world_position_15.y - (&kernelContext_37)->frame_0->fog_params_0.z, length((&kernelContext_37)->frame_0->camera_position_0.xyz - _S272.world_position_15)));
    float3 lit_2 = (lit_1 + _S333 * _S285.xyz) * float3(fog_survives_0)  + (&kernelContext_37)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;

#line 6277
    (&output_2)->lit_0 = float4(lit_2, 1.0f);

#line 6285
    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 6295
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, 1.0f);

#line 6304
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 6295
    }

#line 6306
    return output_2;
}


#line 6306
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


#line 6339
[[fragment]] void depthMaskedFragmentMain(pixelInput_1 _S334 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_4 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_4 [[texture(9)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 6339
    thread KernelContext_0 kernelContext_38;

#line 6339
    (&kernelContext_38)->draw_0 = draw_4;

#line 6339
    (&kernelContext_38)->visible_instances_0 = visible_instances_4;

#line 6339
    (&kernelContext_38)->instances_0 = instances_4;

#line 6339
    (&kernelContext_38)->meshes_0 = meshes_4;

#line 6339
    (&kernelContext_38)->frame_0 = frame_7;

#line 6339
    (&kernelContext_38)->vertices_0 = vertices_4;

#line 6339
    (&kernelContext_38)->ambient_occlusion_0 = ambient_occlusion_4;

#line 6339
    (&kernelContext_38)->materials_0 = materials_4;

#line 6339
    (&kernelContext_38)->base_color_textures_0 = base_color_textures_4;

#line 6339
    (&kernelContext_38)->base_color_sampler_0 = base_color_sampler_4;

#line 6339
    (&kernelContext_38)->normal_textures_0 = normal_textures_4;

#line 6339
    (&kernelContext_38)->mro_textures_0 = mro_textures_4;

#line 6339
    (&kernelContext_38)->emissive_textures_0 = emissive_textures_4;

#line 6339
    (&kernelContext_38)->cluster_lights_0 = cluster_lights_4;

#line 6339
    (&kernelContext_38)->specular_dfg_0 = specular_dfg_4;

#line 6339
    (&kernelContext_38)->lights_0 = lights_4;

#line 6339
    (&kernelContext_38)->ltc_matrix_0 = ltc_matrix_4;

#line 6339
    (&kernelContext_38)->shadow_atlas_0 = shadow_atlas_4;

#line 6339
    (&kernelContext_38)->shadow_sampler_0 = shadow_sampler_4;

#line 6339
    (&kernelContext_38)->contact_shadow_0 = contact_shadow_4;

#line 6339
    (&kernelContext_38)->probes_0 = probes_4;

#line 6339
    (&kernelContext_38)->probe_visibility_0 = probe_visibility_4;

#line 6339
    thread GpuMaterial_natural_0 _S335 = materials_4[_S334.material_17];

#line 6339
    float2 uv_8;

#line 6348
    if(((&_S335)->tiling_0) == 1U)
    {

#line 6348
        uv_8 = physical_tile_uv_0(_S334.world_position_16, normalize(_S334.world_normal_2), (&_S335)->tile_metres_0);

#line 6348
    }
    else
    {

#line 6348
        uv_8 = _S334.uv_7;

#line 6348
    }

#line 6348
    float4 _S336 = base_color_texel_0(&_S335, uv_8, &kernelContext_38);

#line 6348
    bool _S337 = alpha_masked_0(&_S335, _S334.color_4.w * (float4((&_S335)->base_color_0) ).w * _S336.w);

#line 6357
    if(_S337)
    {
        discard_fragment();

#line 6357
    }



    return;
}


#line 6391
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_14 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 6391
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


#line 6434
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_2 _S338 [[stage_in]], bool front_facing_2 [[front_facing]], float4 position_7 [[position]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_5 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_5 [[texture(9)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 6434
    thread KernelContext_0 kernelContext_39;

#line 6434
    (&kernelContext_39)->draw_0 = draw_5;

#line 6434
    (&kernelContext_39)->visible_instances_0 = visible_instances_5;

#line 6434
    (&kernelContext_39)->instances_0 = instances_5;

#line 6434
    (&kernelContext_39)->meshes_0 = meshes_5;

#line 6434
    (&kernelContext_39)->frame_0 = frame_9;

#line 6434
    (&kernelContext_39)->vertices_0 = vertices_5;

#line 6434
    (&kernelContext_39)->ambient_occlusion_0 = ambient_occlusion_5;

#line 6434
    (&kernelContext_39)->materials_0 = materials_5;

#line 6434
    (&kernelContext_39)->base_color_textures_0 = base_color_textures_5;

#line 6434
    (&kernelContext_39)->base_color_sampler_0 = base_color_sampler_5;

#line 6434
    (&kernelContext_39)->normal_textures_0 = normal_textures_5;

#line 6434
    (&kernelContext_39)->mro_textures_0 = mro_textures_5;

#line 6434
    (&kernelContext_39)->emissive_textures_0 = emissive_textures_5;

#line 6434
    (&kernelContext_39)->cluster_lights_0 = cluster_lights_5;

#line 6434
    (&kernelContext_39)->specular_dfg_0 = specular_dfg_5;

#line 6434
    (&kernelContext_39)->lights_0 = lights_5;

#line 6434
    (&kernelContext_39)->ltc_matrix_0 = ltc_matrix_5;

#line 6434
    (&kernelContext_39)->shadow_atlas_0 = shadow_atlas_5;

#line 6434
    (&kernelContext_39)->shadow_sampler_0 = shadow_sampler_5;

#line 6434
    (&kernelContext_39)->contact_shadow_0 = contact_shadow_5;

#line 6434
    (&kernelContext_39)->probes_0 = probes_5;

#line 6434
    (&kernelContext_39)->probe_visibility_0 = probe_visibility_5;

#line 6439
    float3 vertex_normal_1 = normalize(_S338.world_normal_3);

#line 6439
    thread GpuMaterial_natural_0 _S339 = materials_5[_S338.material_18];

#line 6439
    float2 uv_10;

#line 6446
    if(((&_S339)->tiling_0) == 1U)
    {

#line 6446
        uv_10 = physical_tile_uv_0(_S338.world_position_17, vertex_normal_1, (&_S339)->tile_metres_0);

#line 6446
    }
    else
    {

#line 6446
        uv_10 = _S338.uv_9;

#line 6446
    }

#line 6446
    float4 _S340 = base_color_texel_0(&_S339, uv_10, &kernelContext_39);

#line 6451
    float4 albedo_3 = _S338.color_5 * float4((&_S339)->base_color_0)  * _S340;

#line 6451
    bool _S341 = alpha_masked_0(&_S339, albedo_3.w);

#line 6457
    if(_S341)
    {
        discard_fragment();

#line 6457
    }

#line 6462
    thread RsmOutput_0 written_0;

#line 6473
    float3 _S342 = albedo_3.xyz;

#line 6473
    float4 _S343 = mro_texel_0(&_S339, uv_10, &kernelContext_39);

#line 6473
    float _S344 = metallic_of_0(&_S339, _S343);

#line 6473
    float3 _S345 = _S342 * float3((1.0f - _S344)) ;

#line 6473
    float _S346 = dielectric_diffuse_weight_0(&_S339);

#line 6472
    (&written_0)->albedo_2 = float4(_S345 * float3(_S346) , 1.0f);

#line 6472
    float3 _S347 = double_sided_normal_0(&_S339, vertex_normal_1, front_facing_2);

#line 6472
    float3 _S348 = float3(0.5f) ;

#line 6481
    (&written_0)->normal_14 = float4(_S347 * _S348 + _S348, 1.0f);

    (&written_0)->world_0 = float4(_S338.world_position_17, 1.0f);
    return written_0;
}


#line 6484
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


#line 6484
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], uint base_instance_1 [[base_instance]], DrawConstants_0 constant* draw_6 [[buffer(3)]], uint device* visible_instances_6 [[buffer(5)]], GpuInstance_natural_0 device* instances_6 [[buffer(2)]], GpuMesh_0 device* meshes_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_11 [[buffer(0)]], uint device* vertices_6 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_6 [[texture(2)]], GpuMaterial_natural_0 device* materials_6 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_6 [[texture(0)]], sampler base_color_sampler_6 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_6 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_6 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_6 [[texture(9)]], uint device* cluster_lights_6 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_6 [[texture(3)]], GpuLight_natural_0 device* lights_6 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_6 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_6 [[texture(6)]], GpuProbe_natural_0 device* probes_6 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_6 [[texture(7)]])
{

#line 6484
    thread KernelContext_0 kernelContext_40;

#line 6484
    (&kernelContext_40)->draw_0 = draw_6;

#line 6484
    (&kernelContext_40)->visible_instances_0 = visible_instances_6;

#line 6484
    (&kernelContext_40)->instances_0 = instances_6;

#line 6484
    (&kernelContext_40)->meshes_0 = meshes_6;

#line 6484
    (&kernelContext_40)->frame_0 = frame_11;

#line 6484
    (&kernelContext_40)->vertices_0 = vertices_6;

#line 6484
    (&kernelContext_40)->ambient_occlusion_0 = ambient_occlusion_6;

#line 6484
    (&kernelContext_40)->materials_0 = materials_6;

#line 6484
    (&kernelContext_40)->base_color_textures_0 = base_color_textures_6;

#line 6484
    (&kernelContext_40)->base_color_sampler_0 = base_color_sampler_6;

#line 6484
    (&kernelContext_40)->normal_textures_0 = normal_textures_6;

#line 6484
    (&kernelContext_40)->mro_textures_0 = mro_textures_6;

#line 6484
    (&kernelContext_40)->emissive_textures_0 = emissive_textures_6;

#line 6484
    (&kernelContext_40)->cluster_lights_0 = cluster_lights_6;

#line 6484
    (&kernelContext_40)->specular_dfg_0 = specular_dfg_6;

#line 6484
    (&kernelContext_40)->lights_0 = lights_6;

#line 6484
    (&kernelContext_40)->ltc_matrix_0 = ltc_matrix_6;

#line 6484
    (&kernelContext_40)->shadow_atlas_0 = shadow_atlas_6;

#line 6484
    (&kernelContext_40)->shadow_sampler_0 = shadow_sampler_6;

#line 6484
    (&kernelContext_40)->contact_shadow_0 = contact_shadow_6;

#line 6484
    (&kernelContext_40)->probes_0 = probes_6;

#line 6484
    (&kernelContext_40)->probe_visibility_0 = probe_visibility_6;

#line 6484
    uint _S349 = instance_id_1 - base_instance_1;

#line 6484
    DrawSource_0 _S350 = draw_source_0(0U, &kernelContext_40);

#line 6484
    GpuInstance_natural_0 device* _S351 = (&kernelContext_40)->instances_0+(&kernelContext_40)->visible_instances_0[_S350.run_0 + _S349];

#line 2296
    GpuMesh_0 mesh_4 = (&kernelContext_40)->meshes_0[_S350.mesh_0];

#line 2304
    bool _S352 = ((_S351->flags_0) & 2U) != 0U;

#line 2304
    uint base_vertex_3;
    if(_S352)
    {

#line 2305
        base_vertex_3 = _S351->base_vertex_0;

#line 2305
    }
    else
    {

#line 2305
        base_vertex_3 = mesh_4.base_vertex_1;

#line 2305
    }

#line 2305
    MeshVertex_0 _S353 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_4.uv_scale_u_0, mesh_4.uv_scale_v_0, mesh_4.uv_offset_u_0, mesh_4.uv_offset_v_0), &kernelContext_40);

#line 2305
    uint previous_base_0;

#line 2318
    if(_S352)
    {

#line 2318
        previous_base_0 = _S351->previous_base_vertex_0;

#line 2318
    }
    else
    {

#line 2318
        previous_base_0 = base_vertex_3;

#line 2318
    }

#line 2318
    float3 _S354 = load_position_0(index_8 + previous_base_0, &kernelContext_40);

#line 2318
    matrix<float,int(4),int(4)>  _S355 = matrix<float,int(4),int(4)> (_S351->transform_0.data_0[int(0)][int(0)], _S351->transform_0.data_0[int(1)][int(0)], _S351->transform_0.data_0[int(2)][int(0)], _S351->transform_0.data_0[int(3)][int(0)], _S351->transform_0.data_0[int(0)][int(1)], _S351->transform_0.data_0[int(1)][int(1)], _S351->transform_0.data_0[int(2)][int(1)], _S351->transform_0.data_0[int(3)][int(1)], _S351->transform_0.data_0[int(0)][int(2)], _S351->transform_0.data_0[int(1)][int(2)], _S351->transform_0.data_0[int(2)][int(2)], _S351->transform_0.data_0[int(3)][int(2)], _S351->transform_0.data_0[int(0)][int(3)], _S351->transform_0.data_0[int(1)][int(3)], _S351->transform_0.data_0[int(2)][int(3)], _S351->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S353.position_1, 1.0f)) * (_S355)));


    float4 clip_2 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));

#line 2324
    thread VertexOutput_0 output_3;

    (&output_3)->position_3 = clip_2;
    (&output_3)->world_position_1 = world_1.xyz;

#line 2334
    matrix<float,int(3),int(3)>  _S356 = matrix<float,int(3),int(3)> (_S355[int(0)].xyz, _S355[int(1)].xyz, _S355[int(2)].xyz);

#line 2334
    float3 world_normal_5 = (((_S353.basis_1.normal_0) * (normal_basis_0(_S356))));
    (&output_3)->world_normal_0 = world_normal_5;

#line 2341
    float3 world_tangent_5 = (((_S353.basis_1.tangent_1) * (_S356)));
    (&output_3)->world_tangent_0 = world_tangent_5;

#line 2342
    thread TangentFrame_0 _S357 = _S353.basis_1;

#line 2342
    uint _S358 = frame_word_0(mesh_4.flags_1, &_S357);
    (&output_3)->frame_3 = _S358;

#line 2343
    float4 _S359;

#line 2350
    if(((&kernelContext_40)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 2350
        _S359 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 2350
    }
    else
    {

#line 2350
        _S359 = _S353.color_1;

#line 2350
    }

#line 2349
    (&output_3)->color_2 = _S359;

#line 2356
    (&output_3)->material_6 = _S351->material_0;
    (&output_3)->uv_1 = _S353.uv0_0;

#line 2363
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S354, 1.0f)) * (matrix<float,int(4),int(4)> (_S351->previous_transform_0.data_0[int(0)][int(0)], _S351->previous_transform_0.data_0[int(1)][int(0)], _S351->previous_transform_0.data_0[int(2)][int(0)], _S351->previous_transform_0.data_0[int(3)][int(0)], _S351->previous_transform_0.data_0[int(0)][int(1)], _S351->previous_transform_0.data_0[int(1)][int(1)], _S351->previous_transform_0.data_0[int(2)][int(1)], _S351->previous_transform_0.data_0[int(3)][int(1)], _S351->previous_transform_0.data_0[int(0)][int(2)], _S351->previous_transform_0.data_0[int(1)][int(2)], _S351->previous_transform_0.data_0[int(2)][int(2)], _S351->previous_transform_0.data_0[int(3)][int(2)], _S351->previous_transform_0.data_0[int(0)][int(3)], _S351->previous_transform_0.data_0[int(1)][int(3)], _S351->previous_transform_0.data_0[int(2)][int(3)], _S351->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S360 = output_3;

#line 2367
    thread vertexMain_Result_0 _S361;

#line 2367
    (&_S361)->position_8 = _S360.position_3;

#line 2367
    (&_S361)->world_position_18 = _S360.world_position_1;

#line 2367
    (&_S361)->world_normal_4 = _S360.world_normal_0;

#line 2367
    (&_S361)->color_6 = _S360.color_2;

#line 2367
    (&_S361)->material_19 = _S360.material_6;

#line 2367
    (&_S361)->uv_11 = _S360.uv_1;

#line 2367
    (&_S361)->clip_position_4 = _S360.clip_position_0;

#line 2367
    (&_S361)->previous_clip_position_4 = _S360.previous_clip_position_0;

#line 2367
    (&_S361)->world_tangent_4 = _S360.world_tangent_0;

#line 2367
    (&_S361)->frame_10 = _S360.frame_3;

#line 2367
    return _S361;
}

