#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 3089 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f } };

#line 3084
constant array<float, int(8)> FOG_KERNEL_0 = { { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f } };

#line 4112
constant array<float3, int(2)> CASCADE_TINTS_0 = { { float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), float3(0.34999999403953552f, 0.55000001192092896f, 1.0f) } };

#line 3356
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 3416
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

#line 3570
constant array<float2, int(16)> SHADOW_SEARCH_OFFSET_0 = { { float2(1.41421604156494141f, 0.0f), float2(-1.80617594718933105f, 1.65460801124572754f), float2(0.27646398544311523f, -3.15016794204711914f), float2(2.27656793594360352f, 2.96938395500183105f), float2(-4.17778396606445312f, -0.73899197578430176f), float2(3.95756006240844727f, -2.51747989654541016f), float2(-1.32372796535491943f, 4.92420005798339844f), float2(-2.52448797225952148f, -4.86075210571289062f), float2(5.47713613510131836f, 2.00024008750915527f), float2(-5.69804811477661133f, 2.35207200050354004f), float2(2.74683189392089844f, -5.86983203887939453f), float2(2.02983999252319336f, 6.47145605087280273f), float2(-6.11796808242797852f, -3.54548811912536621f), float2(7.17707204818725586f, -1.57785594463348389f), float2(-4.38005590438842773f, 6.23017597198486328f), float2(-1.01189601421356201f, -7.80872011184692383f) } };

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


#line 5527
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S14 = previous_0.w;

#line 5529
    if(_S14 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S14) ) * float2(0.5f, -0.5f);
}


#line 5403
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_5)
{

#line 5403
    texture2d<float, access::sample> _S15 = kernelContext_5->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S15).get_width(0)),(*((&height_0)) = (_S15).get_height(0));

    int3 _S16 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 5409
    return ((kernelContext_5->ambient_occlusion_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z)));
}


#line 5137
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S17 = axis_0.x;

#line 5141
    float _S18 = axis_0.y;

#line 5141
    bool _S19;

#line 5141
    if(_S17 >= _S18)
    {

#line 5141
        _S19 = _S17 >= (axis_0.z);

#line 5141
    }
    else
    {

#line 5141
        _S19 = false;

#line 5141
    }

#line 5141
    float2 planar_0;

#line 5141
    if(_S19)
    {

#line 5141
        planar_0 = world_position_0.zy;

#line 5141
    }
    else
    {

        if(_S18 >= (axis_0.z))
        {

#line 5145
            planar_0 = world_position_0.xz;

#line 5145
        }
        else
        {

#line 5145
            planar_0 = world_position_0.xy;

#line 5145
        }

#line 5141
    }

#line 5153
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


#line 5174
float3 orthonormal_tangent_0(float3 normal_3)
{
    float _S27 = normal_3.z;

#line 5176
    float sign_z_0;

#line 5176
    if(_S27 >= 0.0f)
    {

#line 5176
        sign_z_0 = 1.0f;

#line 5176
    }
    else
    {

#line 5176
        sign_z_0 = -1.0f;

#line 5176
    }
    float a_0 = -1.0f / (sign_z_0 + _S27);
    float _S28 = normal_3.x;

#line 5178
    float _S29 = sign_z_0 * _S28;

#line 5178
    return float3(1.0f + _S29 * _S28 * a_0, _S29 * normal_3.y * a_0, - sign_z_0 * _S28);
}


#line 5228
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_4)
{
    float _S30 = duvdy_0.y;

#line 5230
    float _S31 = duvdx_0.y;

#line 5230
    float winding_0;
    if((duvdx_0.x * _S30 - duvdy_0.x * _S31) < 0.0f)
    {

#line 5231
        winding_0 = -1.0f;

#line 5231
    }
    else
    {

#line 5231
        winding_0 = 1.0f;

#line 5231
    }
    float3 tangent_2 = (float3(_S30)  * dpdx_0 - float3(_S31)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_4;

#line 5240
    float3 tangent_3 = tangent_2 - normal_4 * float3(dot(normal_4, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 5241
    float3 _S32;

#line 5250
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 5250
        _S32 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 5250
    }
    else
    {

#line 5250
        _S32 = orthonormal_tangent_0(normal_4);

#line 5250
    }

#line 5250
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


#line 5310
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_5, float2 uv_2, KernelContext_0 thread* kernelContext_7)
{

#line 5322
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_2);
    float2 duvdy_1 = dfdy(uv_2);

    if(layer_0 == 65535U)
    {
        return normal_5;
    }

    thread TangentFrame_0 basis_5;

#line 5332
    uint _S33 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 5341
        (&basis_5)->normal_0 = normal_5;
        float3 tangent_4 = input_0->world_tangent_0 - normal_5 * float3(dot(normal_5, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 5343
        float3 _S34;

#line 5348
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 5348
            _S34 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 5348
        }
        else
        {

#line 5348
            _S34 = orthonormal_tangent_0(normal_5);

#line 5348
        }

#line 5348
        (&basis_5)->tangent_1 = _S34;

#line 5354
        float3 _S35 = cross((&basis_5)->normal_0, _S34);

#line 5354
        float _S36;
        if((_S33 & 2U) != 0U)
        {

#line 5355
            _S36 = -1.0f;

#line 5355
        }
        else
        {

#line 5355
            _S36 = 1.0f;

#line 5355
        }

#line 5354
        (&basis_5)->bitangent_0 = _S35 * float3(_S36) ;

#line 5333
    }
    else
    {

#line 5359
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_5);

#line 5333
    }

#line 5363
    float3 _S37 = float3(uv_2, float(layer_0));
    float3 _S38 = ((kernelContext_7->normal_textures_0).sample((kernelContext_7->base_color_sampler_0), ((_S37)).xy, uint(((_S37)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 5364
    thread float3 tangent_space_0 = _S38;
    tangent_space_0.xy = _S38.xy * float2(normal_scale_1) ;

#line 5370
    float3 _S39 = normalize(tangent_space_0);

#line 5370
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


#line 4555
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_10)
{
    uint _S47 = max(kernelContext_10->frame_0->cluster_grid_0.x, 1U);
    uint _S48 = max(kernelContext_10->frame_0->cluster_grid_0.y, 1U);
    uint _S49 = max(kernelContext_10->frame_0->cluster_grid_0.z, 1U);
    uint _S50 = max(kernelContext_10->frame_0->cluster_grid_0.w, 1U);

#line 4565
    uint _S51 = uint(pixel_0.x) / _S50;

#line 4565
    uint _S52 = min(_S51, _S47 - 1U);
    uint _S53 = uint(pixel_0.y) / _S50;

    float scale_0 = 24.0f / log2(10000.0f);

#line 4576
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


#line 4506
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 4522
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 4534
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 4541
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


#line 3647
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 3647
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_15)
{
    return kernelContext_15->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3707
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 3679
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


#line 3634
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_17)
{

#line 3634
    uint _S124;

    if(uint(pixel_1.x) < (kernelContext_17->frame_0->shadow_filter_0.z))
    {

#line 3636
        _S124 = kernelContext_17->frame_0->shadow_filter_0.x;

#line 3636
    }
    else
    {

#line 3636
        _S124 = kernelContext_17->frame_0->shadow_filter_0.y;

#line 3636
    }

#line 3636
    return _S124;
}


#line 3659
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_18)
{
    return kernelContext_18->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 3659
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_19)
{
    return kernelContext_19->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 383
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3729
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_20)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S125 = spoke_0.x;

#line 3734
    float _S126 = rotation_0.x;

#line 3734
    float _S127 = spoke_0.y;

#line 3734
    float _S128 = rotation_0.y;


    float _S129 = ((kernelContext_20->shadow_atlas_0).sample_compare((kernelContext_20->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S125 * _S126 - _S127 * _S128, _S125 * _S128 + _S127 * _S126) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 3737
    return _S129;
}


#line 3729
float tile_tap_1(float4 rect_6, float2 texel_step_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_21)
{

    float2 tile_min_1 = float2(0.5f, 0.5f) * texel_step_1;

    float _S130 = spoke_1.x;

#line 3734
    float _S131 = rotation_1.x;

#line 3734
    float _S132 = spoke_1.y;

#line 3734
    float _S133 = rotation_1.y;


    float _S134 = ((kernelContext_21->shadow_atlas_0).sample_compare((kernelContext_21->shadow_sampler_0), (atlas_uv_0(rect_6, clamp(tile_uv_2 + float2(_S130 * _S131 - _S132 * _S133, _S130 * _S133 + _S132 * _S131) * texel_step_1, tile_min_1, float2(1.0f)  - tile_min_1))), (reference_1), level((0.0f))));

#line 3737
    return _S134;
}


#line 3839
float tile_box_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, KernelContext_0 thread* kernelContext_22)
{

#line 3839
    float4 _S135 = atlas_rect_1(tile_2, kernelContext_22);


    if(atlas_rect_is_empty_0(_S135))
    {
        return 1.0f;
    }

#line 3844
    float2 _S136 = atlas_step_1(_S135, kernelContext_22);

#line 3844
    int y_1 = int(-1);

#line 3844
    float visibility_0 = 0.0f;

#line 3849
    for(;;)
    {

#line 3849
        if(y_1 <= int(1))
        {
        }
        else
        {

#line 3849
            break;
        }

#line 3849
        int x_0 = int(-1);

        for(;;)
        {

#line 3851
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 3851
                break;
            }

#line 3851
            float _S137 = tile_tap_1(_S135, _S136, tile_uv_3, float2(float(x_0), float(y_1)), float2(1.0f, 0.0f), reference_2, kernelContext_22);

            float visibility_1 = visibility_0 + _S137;

#line 3851
            x_0 = x_0 + int(1);

#line 3851
            visibility_0 = visibility_1;

#line 3851
        }

#line 3849
        y_1 = y_1 + int(1);

#line 3849
    }

#line 3857
    return visibility_0 / 9.0f;
}


#line 3592
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 3892
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_2, KernelContext_0 thread* kernelContext_23)
{
    float2 texel_2 = kernelContext_23->frame_0->shadow_params_0.xy;

#line 3894
    float4 _S138 = atlas_rect_0(cascade_0, kernelContext_23);

#line 3894
    float2 _S139 = atlas_step_0(_S138, kernelContext_23);


    float2 _S140 = float2(0.5f, 0.5f) * _S139;


    float2 _S141 = float2(1.0f, 1.0f);

#line 3900
    float2 _S142 = _S141 / texel_2;

#line 3900
    uint index_2 = 0U;

#line 3900
    float sum_2 = 0.0f;

#line 3900
    float found_0 = 0.0f;



    for(;;)
    {

#line 3904
        if(index_2 < 16U)
        {
        }
        else
        {

#line 3904
            break;
        }

        float _S143 = SHADOW_SEARCH_OFFSET_0[index_2].x;

#line 3907
        float _S144 = rotation_2.x;

#line 3907
        float _S145 = SHADOW_SEARCH_OFFSET_0[index_2].y;

#line 3907
        float _S146 = rotation_2.y;

#line 3915
        int3 _S147 = int3(int2(min(atlas_uv_0(_S138, clamp(tile_uv_4 + float2(_S143 * _S144 - _S145 * _S146, _S143 * _S146 + _S145 * _S144) * _S139, _S140, float2(1.0f)  - _S140)) * _S142, _S142 - _S141)), int(0));

#line 3915
        float depth_1 = ((kernelContext_23->shadow_atlas_0).read(vec<uint,2>(((_S147)).xy), uint(((_S147)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 3919
            sum_2 = sum_2 + depth_1;

#line 3919
            found_0 = found_1;

#line 3916
        }

#line 3904
        index_2 = index_2 + 1U;

#line 3904
    }

#line 3923
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3934
    float _S148 = 2.0f * kernelContext_23->frame_0->cascade_far_0[cascade_0];

#line 3934
    float separation_0 = (sum_2 / found_0 - reference_3) * (_S148 + 40.0f);

#line 3934
    float _S149 = tile_texels_0(_S138, kernelContext_23);

    return clamp(separation_0 * 0.01999999955296516f / (_S148 / _S149), 2.0f, 8.0f);
}


#line 3759
float tile_pcf_0(uint tile_3, float2 tile_uv_5, float reference_4, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_24)
{
    float2 rotation_3 = shadow_rotation_0(pixel_3);

#line 3761
    float4 _S150 = atlas_rect_0(tile_3, kernelContext_24);

    if(atlas_rect_is_empty_0(_S150))
    {
        return 1.0f;
    }

#line 3765
    float2 _S151 = atlas_step_0(_S150, kernelContext_24);



    thread array<float, int(5)> probe_taps_0;

#line 3769
    uint spot_0 = 0U;

#line 3769
    float probe_0 = 0.0f;

    for(;;)
    {

#line 3771
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3771
            break;
        }

#line 3771
        float _S152 = tile_tap_0(_S150, _S151, tile_uv_5, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , rotation_3, reference_4, kernelContext_24);

        probe_taps_0[spot_0] = _S152;


        float probe_1 = probe_0 + _S152;

#line 3771
        spot_0 = spot_0 + 1U;

#line 3771
        probe_0 = probe_1;

#line 3771
    }

#line 3782
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3788
    uint index_3 = 1U;

#line 3788
    float visibility_2 = probe_taps_0[int(0)];

#line 3796
    for(;;)
    {

#line 3796
        if(index_3 < 23U)
        {
        }
        else
        {

#line 3796
            break;
        }

#line 3796
        float _S153 = tile_tap_0(_S150, _S151, tile_uv_5, SHADOW_DISC_0[index_3] * float2(radius_2) , rotation_3, reference_4, kernelContext_24);

        float visibility_3 = visibility_2 + _S153;

#line 3796
        index_3 = index_3 + 1U;

#line 3796
        visibility_2 = visibility_3;

#line 3796
    }

#line 3801
    float visibility_4 = visibility_2 + probe_taps_0[int(1)];

#line 3801
    float2 _S154 = float2(radius_2) ;

#line 3801
    float _S155 = tile_tap_0(_S150, _S151, tile_uv_5, float2(0.43503299355506897f, 0.75919097661972046f) * _S154, rotation_3, reference_4, kernelContext_24);


    float visibility_5 = visibility_4 + _S155 + probe_taps_0[int(2)];

#line 3804
    float _S156 = tile_tap_0(_S150, _S151, tile_uv_5, float2(0.82610201835632324f, -0.38168001174926758f) * _S154, rotation_3, reference_4, kernelContext_24);


    float visibility_6 = visibility_5 + _S156 + probe_taps_0[int(3)];

#line 3807
    float _S157 = tile_tap_0(_S150, _S151, tile_uv_5, float2(-0.31940698623657227f, -0.88803398609161377f) * _S154, rotation_3, reference_4, kernelContext_24);


    float visibility_7 = visibility_6 + _S157 + probe_taps_0[int(4)];

#line 3810
    index_3 = 30U;

#line 3810
    visibility_2 = visibility_7;
    for(;;)
    {

#line 3811
        if(index_3 < 32U)
        {
        }
        else
        {

#line 3811
            break;
        }

#line 3811
        float _S158 = tile_tap_0(_S150, _S151, tile_uv_5, SHADOW_DISC_0[index_3] * _S154, rotation_3, reference_4, kernelContext_24);

        float visibility_8 = visibility_2 + _S158;

#line 3811
        index_3 = index_3 + 1U;

#line 3811
        visibility_2 = visibility_8;

#line 3811
    }

#line 3816
    return visibility_2 / 32.0f;
}


#line 3988
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_25)
{

#line 3989
    float4 _S159 = atlas_rect_0(cascade_1, kernelContext_25);

#line 4023
    if(atlas_rect_is_empty_0(_S159))
    {


        return 1.0f;
    }
    float _S160 = 2.0f * kernelContext_25->frame_0->cascade_far_0[cascade_1];

#line 4029
    float _S161 = tile_texels_0(_S159, kernelContext_25);

#line 4029
    float texel_world_0 = _S160 / _S161;

#line 4036
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_25->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_25->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_25->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 4040
    bool _S162;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 4041
        _S162 = true;

#line 4041
    }
    else
    {

#line 4041
        _S162 = (ndc_0.z) <= 0.0f;

#line 4041
    }

#line 4041
    if(_S162)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 4051
    uint _S163 = shadow_filter_mode_0(pixel_4, kernelContext_25);

#line 4068
    if(_S163 == 2U)
    {

#line 4068
        float _S164 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_0.z, kernelContext_25);

        return _S164;
    }

#line 4070
    float radius_3;

#line 4078
    if(_S163 != 1U)
    {

#line 4078
        float _S165 = sun_penumbra_texels_0(cascade_1, tile_uv_6, ndc_0.z, shadow_rotation_0(pixel_4), kernelContext_25);

#line 4078
        radius_3 = _S165;

#line 4078
    }
    else
    {

#line 4078
        radius_3 = 2.0f;

#line 4078
    }

#line 4078
    float _S166 = tile_pcf_0(cascade_1, tile_uv_6, ndc_0.z, pixel_4, radius_3, kernelContext_25);

#line 4083
    return _S166;
}


#line 4176
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_26)
{
    uint cascade_2;

#line 4178
    bool covered_0;

#line 4187
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 4199
    float eye_distance_0 = length(world_position_5 - kernelContext_26->frame_0->camera_position_0.xyz);

#line 4199
    uint index_4 = 0U;

#line 4207
    for(;;)
    {

#line 4207
        if(index_4 < 2U)
        {
        }
        else
        {

#line 4207
            covered_0 = false;

#line 4207
            cascade_2 = 1U;

#line 4207
            break;
        }
        if(eye_distance_0 < kernelContext_26->frame_0->cascade_far_0[index_4])
        {

#line 4209
            covered_0 = true;

#line 4209
            cascade_2 = index_4;



            break;
        }

#line 4207
        index_4 = index_4 + 1U;

#line 4207
    }

#line 4216
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 4216
    }

#line 4216
    float blend_0;

#line 4228
    if((cascade_2 + 1U) < 2U)
    {

        float band_0 = kernelContext_26->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;

#line 4231
        blend_0 = saturate((eye_distance_0 - (kernelContext_26->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);

#line 4228
    }
    else
    {

#line 4228
        blend_0 = 0.0f;

#line 4228
    }

#line 4236
    *fade_0 = blend_0;

#line 4236
    uint lookups_0;

#line 4250
    if(blend_0 > 0.0f)
    {

#line 4250
        lookups_0 = 2U;

#line 4250
    }
    else
    {

#line 4250
        lookups_0 = 1U;

#line 4250
    }

#line 4250
    float visibility_9 = 1.0f;

#line 4250
    float next_0 = 1.0f;

#line 4250
    uint step_0 = 0U;



    for(;;)
    {

#line 4254
        if(step_0 < lookups_0)
        {
        }
        else
        {

#line 4254
            break;
        }

#line 4254
        float _S167 = cascade_visibility_0(cascade_2 + step_0, world_position_5, to_light_3, geometric_normal_2, pixel_5, kernelContext_26);


        if(step_0 == 0U)
        {

#line 4257
            visibility_9 = _S167;

#line 4257
        }
        else
        {

#line 4257
            next_0 = _S167;

#line 4257
        }

#line 4254
        step_0 = step_0 + 1U;

#line 4254
    }

#line 4266
    if(lookups_0 == 1U)
    {

#line 4266
        blend_0 = visibility_9;

#line 4266
    }
    else
    {

#line 4266
        blend_0 = mix(visibility_9, next_0, blend_0);

#line 4266
    }

#line 4266
    return blend_0;
}


#line 5439
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_27)
{

#line 5439
    texture2d<float, access::sample> _S168 = kernelContext_27->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S168).get_width(0)),(*((&height_2)) = (_S168).get_height(0));

    int3 _S169 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 5445
    return ((kernelContext_27->contact_shadow_0).read(vec<uint,2>(((_S169)).xy), uint(((_S169)).z)).x);
}


#line 4143
float3 cascade_tint_0(uint cascade_3, float blend_1)
{

#line 4143
    float3 tint_0;

#line 4149
    if(0U == cascade_3)
    {

#line 4149
        tint_0 = mix(float3(1.0f, 0.34999999403953552f, 0.34999999403953552f), CASCADE_TINTS_0[min(1U, 1U)], float3(blend_1) );

#line 4149
    }
    else
    {

#line 4149
        tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 4149
    }

#line 4149
    if(1U == cascade_3)
    {

#line 4149
        tint_0 = float3(0.34999999403953552f, 0.55000001192092896f, 1.0f);

#line 4149
    }

#line 4160
    return tint_0;
}


#line 4466
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S170 = axis_2.x;

#line 4469
    float _S171 = axis_2.y;

#line 4469
    bool _S172;

#line 4469
    if(_S170 >= _S171)
    {

#line 4469
        _S172 = _S170 >= (axis_2.z);

#line 4469
    }
    else
    {

#line 4469
        _S172 = false;

#line 4469
    }

#line 4469
    uint _S173;

#line 4469
    if(_S172)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 4471
            _S173 = 0U;

#line 4471
        }
        else
        {

#line 4471
            _S173 = 1U;

#line 4471
        }

#line 4471
        return _S173;
    }
    if(_S171 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 4475
            _S173 = 2U;

#line 4475
        }
        else
        {

#line 4475
            _S173 = 3U;

#line 4475
        }

#line 4475
        return _S173;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 4477
        _S173 = 4U;

#line 4477
    }
    else
    {

#line 4477
        _S173 = 5U;

#line 4477
    }

#line 4477
    return _S173;
}




float point_map_world_0(const GpuLight_natural_0 thread* light_1, float3 world_position_6, uint thread* face_0)
{
    float3 from_light_1 = world_position_6 - (float4(light_1->position_0) ).xyz;

#line 4490
    float _S174 = max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z));
    *face_0 = point_face_0(from_light_1);
    return 2.0f * _S174;
}


#line 370
uint light_tile_0(uint tile_4)
{
    return 2U + tile_4;
}


#line 4373
float punctual_visibility_0(uint tile_5, float3 world_position_7, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_28)
{

    uint atlas_0 = light_tile_0(tile_5);

#line 4376
    float4 _S175 = atlas_rect_0(atlas_0, kernelContext_28);

    if(atlas_rect_is_empty_0(_S175))
    {


        return 1.0f;
    }

#line 4382
    float _S176 = tile_texels_0(_S175, kernelContext_28);

    float texel_world_1 = map_world_0 / _S176;

#line 4394
    float4 clip_1 = (((float4(world_position_7 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(0)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(1)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(2)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(0)][int(3)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(1)][int(3)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(2)][int(3)], (&kernelContext_28->frame_0->light_view_proj_0)->data_3[tile_5].data_1[int(3)][int(3)]))));

#line 4401
    float _S177 = clip_1.w;

#line 4401
    if(_S177 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S177) ;

#line 4405
    bool _S178;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 4406
        _S178 = true;

#line 4406
    }
    else
    {

#line 4406
        _S178 = (ndc_1.z) <= 0.0f;

#line 4406
    }

#line 4406
    if(_S178)
    {

#line 4406
        _S178 = true;

#line 4406
    }
    else
    {

#line 4406
        _S178 = (ndc_1.z) > 1.0f;

#line 4406
    }

#line 4406
    if(_S178)
    {

#line 4413
        return 1.0f;
    }



    float2 tile_uv_7 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 4418
    uint _S179 = shadow_filter_mode_0(pixel_6, kernelContext_28);

#line 4427
    if(_S179 == 2U)
    {

#line 4427
        float _S180 = tile_box_pcf_0(atlas_0, tile_uv_7, ndc_1.z, kernelContext_28);

        return _S180;
    }

#line 4429
    float _S181 = tile_pcf_0(atlas_0, tile_uv_7, ndc_1.z, pixel_6, 2.0f, kernelContext_28);

    return _S181;
}



float spot_map_world_0(const GpuLight_natural_0 thread* light_2, float3 world_position_8)
{

#line 4436
    float4 _S182 = float4(light_2->direction_0) ;

#line 4442
    float cos_outer_1 = _S182.w;

#line 4449
    return 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S182.xyz)), 0.0f);
}


#line 2758
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 5426
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 5428
    float3 _S183;
    if((length(decoded_0)) < 0.5f)
    {

#line 5429
        _S183 = shading_normal_1;

#line 5429
    }
    else
    {

#line 5429
        _S183 = normalize(decoded_0);

#line 5429
    }

#line 5429
    return _S183;
}


#line 5064
float3 sky_irradiance_0(float3 normal_8, KernelContext_0 thread* kernelContext_29)
{
    float4 basis_6 = float4(normal_8, 1.0f);
    return max(float3(dot(kernelContext_29->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_29->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_29->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 4965
float probe_level_reach_0(float3 world_position_9, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 4965
    float reach_0 = 0.0f;

#line 4965
    uint axis_3 = 0U;


    for(;;)
    {

#line 4968
        if(axis_3 < 3U)
        {
        }
        else
        {

#line 4968
            break;
        }

#line 4968
        uint _S184 = axis_3;

#line 4968
        bool _S185;

        if((last_0[axis_3]) == 0.0f)
        {

#line 4970
            _S185 = true;

#line 4970
        }
        else
        {

#line 4970
            _S185 = (inv_spacing_0[axis_3]) == 0.0f;

#line 4970
        }

#line 4970
        if(_S185)
        {

#line 4971
            axis_3 = axis_3 + 1U;

#line 4968
            continue;
        }

#line 4968
        reach_0 = max(reach_0, abs(2.0f * ((world_position_9[axis_3] - origin_0[axis_3]) * inv_spacing_0[axis_3]) / last_0[_S184] - 1.0f));

#line 4968
        axis_3 = axis_3 + 1U;

#line 4968
    }

#line 4975
    return reach_0;
}


#line 4995
float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 4995
    uint level_0 = 0U;

    for(;;)
    {

#line 4997
        uint _S186 = level_0 + 1U;

#line 4997
        if(_S186 < levels_0)
        {
        }
        else
        {

#line 4997
            break;
        }
        float _S187 = float(level_0);

#line 4999
        float at_3 = reach_1 * exp2(- _S187);
        if(at_3 < 1.0f)
        {

#line 5001
            return float2(_S187, saturate((1.0f - at_3) / 0.25f));
        }

#line 4997
        level_0 = _S186;

#line 4997
    }

#line 5003
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 4751
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_2)
{
    uint at_4 = cell_1 + offset_0;

#line 4753
    uint _S188;
    if(at_4 >= count_2)
    {

#line 4754
        _S188 = at_4 - count_2;

#line 4754
    }
    else
    {

#line 4754
        _S188 = at_4;

#line 4754
    }

#line 4754
    return _S188;
}


#line 4777
uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_30)
{
    uint3 counts_0 = kernelContext_30->frame_0->probe_counts_0.xyz;
    uint3 offset_1 = kernelContext_30->frame_0->probe_level_offset_0[level_1].xyz;
    uint _S189 = counts_0.x;
    uint _S190 = counts_0.y;



    return min(kernelContext_30->frame_0->probe_levels_0.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S190 + probe_wrap_0(cell_2.y, offset_1.y, _S190)) * _S189 + probe_wrap_0(cell_2.x, offset_1.x, _S189), max(kernelContext_30->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 4618
float sign_not_zero_0(float value_0)
{

#line 4618
    float _S191;

    if(value_0 >= 0.0f)
    {

#line 4620
        _S191 = 1.0f;

#line 4620
    }
    else
    {

#line 4620
        _S191 = -1.0f;

#line 4620
    }

#line 4620
    return _S191;
}


#line 4637
float2 oct_encode_0(float3 direction_1)
{
    float _S192 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S192) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 4640
    float2 p_1;
    if(_S192 < 0.0f)
    {
        float _S193 = p_0.y;

#line 4643
        float _S194 = p_0.x;

#line 4643
        p_1 = float2((1.0f - abs(_S193)) * sign_not_zero_0(_S194), (1.0f - abs(_S194)) * sign_not_zero_0(_S193));

#line 4641
    }
    else
    {

#line 4641
        p_1 = p_0;

#line 4641
    }

#line 4646
    return p_1;
}


#line 4666
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_31)
{

#line 4666
    texture2d_array<float, access::sample> _S195 = kernelContext_31->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S195).get_width(0)),(*((&height_3)) = (_S195).get_height(0)),(*((&layers_0)) = (_S195).get_array_size());

#line 4671
    float2 _S196 = float2(0.5f) ;

#line 4671
    float2 _S197 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S196 + _S196) * float2(16.0f)  + _S197 - _S196;
    float2 _S198 = float2(float(width_3), float(height_3)) - _S197;

#line 4675
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S198);
    float2 high_2 = min(low_2 + _S197, _S198);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S199 = int(low_2.x);

#line 4680
    int _S200 = int(low_2.y);

#line 4680
    int4 _S201 = int4(_S199, _S200, layer_1, int(0));
    int _S202 = int(high_2.x);

#line 4681
    int4 _S203 = int4(_S202, _S200, layer_1, int(0));
    int _S204 = int(high_2.y);

#line 4682
    int4 _S205 = int4(_S199, _S204, layer_1, int(0));
    int4 _S206 = int4(_S202, _S204, layer_1, int(0));
    float2 _S207 = float2(weight_2.x) ;

#line 4684
    return mix(mix(((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S201)).xy), uint(((_S201)).z), uint(((_S201)).w))).xy, ((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S203)).xy), uint(((_S203)).z), uint(((_S203)).w))).xy, _S207), mix(((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S205)).xy), uint(((_S205)).z), uint(((_S205)).w))).xy, ((kernelContext_31->probe_visibility_0).read(vec<uint,2>(((_S206)).xy), uint(((_S206)).z), uint(((_S206)).w))).xy, _S207), float2(weight_2.y) );
}


#line 4712
float probe_chebyshev_0(uint index_6, float3 probe_position_0, float3 world_position_10, float3 normal_9, KernelContext_0 thread* kernelContext_32)
{
    float3 to_probe_0 = probe_position_0 - (world_position_10 + normal_9 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 4715
    float2 _S208 = probe_moments_0(index_6, - to_probe_0, kernelContext_32);

#line 4721
    float _S209 = _S208.x;

#line 4721
    float _S210 = max(_S208.y - _S209 * _S209, 0.0f);
    float behind_0 = to_surface_0 - _S209;
    float bound_0 = _S210 / (_S210 + behind_0 * behind_0);

#line 4723
    float _S211;
    if(to_surface_0 <= _S209)
    {

#line 4724
        _S211 = 1.0f;

#line 4724
    }
    else
    {

#line 4724
        _S211 = bound_0 * bound_0 * bound_0;

#line 4724
    }

#line 4724
    return _S211;
}


#line 4734
float probe_weight_0(uint index_7, float3 probe_position_1, float3 world_position_11, float3 normal_10, KernelContext_0 thread* kernelContext_33)
{

#line 4734
    float _S212 = probe_chebyshev_0(index_7, probe_position_1, world_position_11, normal_10, kernelContext_33);

    return max(_S212, 0.00009999999747379f);
}


#line 1284
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 4796
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 4823
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_12, float3 normal_11, KernelContext_0 thread* kernelContext_34)
{

#line 4824
    uint _S213 = probe_row_0(level_2, cell_3, kernelContext_34);


    GpuProbe_natural_0 stored_0 = kernelContext_34->probes_0[_S213];

#line 4827
    float _S214 = probe_weight_0(_S213, origin_1 + float3(cell_3) * spacing_0, world_position_12, normal_11, kernelContext_34);



    thread WeightedProbe_0 corner_2;

#line 4831
    float4 _S215 = float4(_S214) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S215;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S215;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S215;
    (&corner_2)->weight_3 = _S214;
    return corner_2;
}


#line 4807
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S216 = float4(t_1) ;

#line 4810
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S216);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S216);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S216);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 4896
float3 probe_level_irradiance_0(uint level_3, float3 world_position_13, float3 normal_12, KernelContext_0 thread* kernelContext_35)
{

#line 4896
    float3 _S217 = float3(1.0f) ;

#line 4901
    float3 _S218 = float3(0.0f, 0.0f, 0.0f);

#line 4901
    float3 last_1 = max(float3(kernelContext_35->frame_0->probe_counts_0.xyz) - _S217, _S218);



    float3 origin_2 = kernelContext_35->frame_0->probe_level_origin_0[level_3].xyz;
    float3 inv_0 = kernelContext_35->frame_0->probe_level_inv_spacing_0[level_3].xyz;
    float3 grid_0 = clamp((world_position_13 - origin_2) * inv_0, _S218, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;

    uint3 _S219 = uint3(base_0);



    uint3 _S220 = uint3(min(base_0 + _S217, last_1));

#line 4921
    float _S221 = inv_0.x;

#line 4921
    float _S222;

#line 4921
    if(_S221 != 0.0f)
    {

#line 4921
        _S222 = 1.0f / _S221;

#line 4921
    }
    else
    {

#line 4921
        _S222 = 0.0f;

#line 4921
    }
    float _S223 = inv_0.y;

#line 4922
    float _S224;

#line 4922
    if(_S223 != 0.0f)
    {

#line 4922
        _S224 = 1.0f / _S223;

#line 4922
    }
    else
    {

#line 4922
        _S224 = 0.0f;

#line 4922
    }
    float _S225 = inv_0.z;

#line 4923
    float _S226;

#line 4923
    if(_S225 != 0.0f)
    {

#line 4923
        _S226 = 1.0f / _S225;

#line 4923
    }
    else
    {

#line 4923
        _S226 = 0.0f;

#line 4923
    }

#line 4921
    float3 spacing_1 = float3(_S222, _S224, _S226);

#line 4930
    uint _S227 = _S219.x;

#line 4930
    uint _S228 = _S219.y;

#line 4930
    uint _S229 = _S219.z;

#line 4930
    WeightedProbe_0 _S230 = probe_corner_0(level_3, uint3(_S227, _S228, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);
    uint _S231 = _S220.x;

#line 4931
    WeightedProbe_0 _S232 = probe_corner_0(level_3, uint3(_S231, _S228, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4931
    float _S233 = f_0.x;

#line 4931
    thread WeightedProbe_0 _S234 = _S230;

#line 4931
    thread WeightedProbe_0 _S235 = _S232;

#line 4931
    WeightedProbe_0 _S236 = lerp_probe_0(&_S234, &_S235, _S233);
    uint _S237 = _S220.y;

#line 4932
    WeightedProbe_0 _S238 = probe_corner_0(level_3, uint3(_S227, _S237, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4932
    WeightedProbe_0 _S239 = probe_corner_0(level_3, uint3(_S231, _S237, _S229), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4932
    thread WeightedProbe_0 _S240 = _S238;

#line 4932
    thread WeightedProbe_0 _S241 = _S239;

#line 4932
    WeightedProbe_0 _S242 = lerp_probe_0(&_S240, &_S241, _S233);

    uint _S243 = _S220.z;

#line 4934
    WeightedProbe_0 _S244 = probe_corner_0(level_3, uint3(_S227, _S228, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4934
    WeightedProbe_0 _S245 = probe_corner_0(level_3, uint3(_S231, _S228, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4934
    thread WeightedProbe_0 _S246 = _S244;

#line 4934
    thread WeightedProbe_0 _S247 = _S245;

#line 4934
    WeightedProbe_0 _S248 = lerp_probe_0(&_S246, &_S247, _S233);

#line 4934
    WeightedProbe_0 _S249 = probe_corner_0(level_3, uint3(_S227, _S237, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4934
    WeightedProbe_0 _S250 = probe_corner_0(level_3, uint3(_S231, _S237, _S243), origin_2, spacing_1, world_position_13, normal_12, kernelContext_35);

#line 4934
    thread WeightedProbe_0 _S251 = _S249;

#line 4934
    thread WeightedProbe_0 _S252 = _S250;

#line 4934
    WeightedProbe_0 _S253 = lerp_probe_0(&_S251, &_S252, _S233);



    float _S254 = f_0.y;

#line 4938
    thread WeightedProbe_0 _S255 = _S236;

#line 4938
    thread WeightedProbe_0 _S256 = _S242;

#line 4938
    WeightedProbe_0 _S257 = lerp_probe_0(&_S255, &_S256, _S254);

#line 4938
    thread WeightedProbe_0 _S258 = _S248;

#line 4938
    thread WeightedProbe_0 _S259 = _S253;

#line 4938
    WeightedProbe_0 _S260 = lerp_probe_0(&_S258, &_S259, _S254);

    float _S261 = f_0.z;

#line 4940
    thread WeightedProbe_0 _S262 = _S257;

#line 4940
    thread WeightedProbe_0 _S263 = _S260;

#line 4940
    WeightedProbe_0 _S264 = lerp_probe_0(&_S262, &_S263, _S261);

    float4 basis_7 = float4(normal_12, 1.0f);
    return max(float3(dot(_S264.sh_0.sh_r_0, basis_7), dot(_S264.sh_0.sh_g_0, basis_7), dot(_S264.sh_0.sh_b_0, basis_7)) / float3(_S264.weight_3) , _S218);
}


#line 5030
float3 probe_irradiance_0(float3 world_position_14, float3 normal_13, KernelContext_0 thread* kernelContext_36)
{
    if((kernelContext_36->frame_0->probe_counts_0.w) == 0U)
    {

#line 5033
        return float3(0.0f, 0.0f, 0.0f);
    }

#line 5041
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_14, kernelContext_36->frame_0->probe_level_origin_0[int(0)].xyz, kernelContext_36->frame_0->probe_level_inv_spacing_0[int(0)].xyz, max(float3(kernelContext_36->frame_0->probe_counts_0.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_36->frame_0->probe_levels_0.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 5043
    float3 _S265 = probe_level_irradiance_0(level_4, world_position_14, normal_13, kernelContext_36);


    if(share_0 >= 1.0f)
    {

#line 5047
        return _S265;
    }

#line 5047
    float3 _S266 = probe_level_irradiance_0(level_4 + 1U, world_position_14, normal_13, kernelContext_36);

    return _S266 * float3((1.0f - share_0))  + _S265 * float3(share_0) ;
}


#line 5495
float3 multi_bounce_occlusion_0(float visibility_10, float3 albedo_0)
{

#line 5495
    float3 _S267 = float3(visibility_10) ;

#line 5501
    return min(float3(1.0f) , max(_S267, ((_S267 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S267 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S267));
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


    float _S268 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

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
        float _S269 = kernel_0 * _S268 + FOG_KERNEL_0[term_0];

#line 3119
        int term_1 = term_0 - int(1);

#line 3119
        kernel_0 = _S269;

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
        float _S270 = - d_0;

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
            float _S271 = series_0 * _S270 + FOG_RATIO_KERNEL_0[term_2];

#line 3142
            int term_3 = term_2 - int(1);

#line 3142
            series_0 = _S271;

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


#line 5090
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 5090
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


#line 5537
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S272 [[stage_in]], bool front_facing_1 [[front_facing]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_3 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_3 [[texture(9)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 5537
    thread KernelContext_0 kernelContext_37;

#line 5537
    (&kernelContext_37)->draw_0 = draw_3;

#line 5537
    (&kernelContext_37)->visible_instances_0 = visible_instances_3;

#line 5537
    (&kernelContext_37)->instances_0 = instances_3;

#line 5537
    (&kernelContext_37)->meshes_0 = meshes_3;

#line 5537
    (&kernelContext_37)->frame_0 = frame_5;

#line 5537
    (&kernelContext_37)->vertices_0 = vertices_3;

#line 5537
    (&kernelContext_37)->ambient_occlusion_0 = ambient_occlusion_3;

#line 5537
    (&kernelContext_37)->materials_0 = materials_3;

#line 5537
    (&kernelContext_37)->base_color_textures_0 = base_color_textures_3;

#line 5537
    (&kernelContext_37)->base_color_sampler_0 = base_color_sampler_3;

#line 5537
    (&kernelContext_37)->normal_textures_0 = normal_textures_3;

#line 5537
    (&kernelContext_37)->mro_textures_0 = mro_textures_3;

#line 5537
    (&kernelContext_37)->emissive_textures_0 = emissive_textures_3;

#line 5537
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_3;

#line 5537
    (&kernelContext_37)->specular_dfg_0 = specular_dfg_3;

#line 5537
    (&kernelContext_37)->lights_0 = lights_3;

#line 5537
    (&kernelContext_37)->ltc_matrix_0 = ltc_matrix_3;

#line 5537
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_3;

#line 5537
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_3;

#line 5537
    (&kernelContext_37)->contact_shadow_0 = contact_shadow_3;

#line 5537
    (&kernelContext_37)->probes_0 = probes_3;

#line 5537
    (&kernelContext_37)->probe_visibility_0 = probe_visibility_3;

#line 5549
    float3 vertex_normal_0 = normalize(_S272.world_normal_1);

#line 5554
    float2 motion_1 = motion_vector_0(_S272.clip_position_1, _S272.previous_clip_position_1);

#line 5570
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 5572
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

#line 5626
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 5626
        float4 _S274 = occlusion_at_0(position_5.xy, &kernelContext_37);


        float value_1 = _S274.x;

#line 5628
        thread FragmentOutput_0 occlusion_1;

#line 5637
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

#line 5654
    thread GpuMaterial_natural_0 _S275 = (&kernelContext_37)->materials_0[_S272.material_16];

#line 5654
    float2 uv_6;

#line 5679
    if(((&_S275)->tiling_0) == 1U)
    {

#line 5679
        uv_6 = physical_tile_uv_0(_S272.world_position_15, vertex_normal_0, (&_S275)->tile_metres_0);

#line 5679
    }
    else
    {

#line 5679
        uv_6 = _S272.uv_5;

#line 5679
    }

#line 5679
    float4 _S276 = base_color_texel_0(&_S275, uv_6, &kernelContext_37);

#line 5701
    float4 albedo_1 = _S272.color_3 * float4((&_S275)->base_color_0)  * _S276;

#line 5701
    bool _S277 = alpha_masked_0(&_S275, albedo_1.w);

#line 5715
    if(_S277)
    {
        discard_fragment();

#line 5715
    }

#line 5715
    float3 _S278 = double_sided_normal_0(&_S275, vertex_normal_0, front_facing_1);

#line 5715
    uint _S279 = normal_layer_0(&_S275);

#line 5715
    thread VertexOutput_0 _S280;

#line 5715
    (&_S280)->position_3 = position_5;

#line 5715
    (&_S280)->world_position_1 = _S272.world_position_15;

#line 5715
    (&_S280)->world_normal_0 = _S272.world_normal_1;

#line 5715
    (&_S280)->color_2 = _S272.color_3;

#line 5715
    (&_S280)->material_6 = _S272.material_16;

#line 5715
    (&_S280)->uv_1 = _S272.uv_5;

#line 5715
    (&_S280)->clip_position_0 = _S272.clip_position_1;

#line 5715
    (&_S280)->previous_clip_position_0 = _S272.previous_clip_position_1;

#line 5715
    (&_S280)->world_tangent_0 = _S272.world_tangent_1;

#line 5715
    (&_S280)->frame_3 = _S272.frame_4;

#line 5715
    float3 _S281 = shading_normal_of_0(_S279, (&_S275)->normal_scale_0, &_S280, _S278, uv_6, &kernelContext_37);

#line 5734
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 5736
        float3 _S282 = float3(0.5f) ;

#line 5748
        (&normals_0)->lit_0 = float4(_S281 * _S282 + _S282, 1.0f);

#line 5754
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_37)->frame_0->camera_position_0.xyz - _S272.world_position_15);



    float3 _S283 = geometric_normal_of_0(_S272.world_position_15, _S278);

#line 5763
    float4 _S284 = mro_texel_0(&_S275, uv_6, &kernelContext_37);

#line 5763
    float4 _S285 = emissive_texel_0(&_S275, uv_6, &kernelContext_37);

#line 5763
    float _S286 = metallic_of_0(&_S275, _S284);

#line 5794
    float roughness_2 = clamp((&_S275)->roughness_0 * _S284.y, 0.04500000178813934f, 1.0f);
    float alpha_1 = roughness_2 * roughness_2;

#line 5828
    float _S287 = saturate(alpha_1 * alpha_1 + specular_aa_kernel_0(_S281));

#line 5828
    float3 _S288 = specular_f0_of_0(&_S275);

#line 5848
    float3 _S289 = albedo_1.xyz;

#line 5848
    float3 f0_2 = mix(_S288, _S289, float3(_S286) );
    float f90_1 = (&_S275)->specular_f90_0 + (1.0f - (&_S275)->specular_f90_0) * _S286;
    float3 _S290 = _S289 * float3((1.0f - _S286)) ;

#line 5850
    float _S291 = dielectric_diffuse_weight_0(&_S275);

#line 5850
    float3 diffuse_albedo_0 = _S290 * float3(_S291) ;

#line 5856
    float _S292 = max(dot(_S281, to_eye_1), 0.00009999999747379f);

#line 5866
    float2 _S293 = position_5.xy;

#line 5866
    uint _S294 = froxel_of_0(_S293, (((float4(_S272.world_position_15, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_37)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_37);

#line 5866
    uint base_1 = _S294 * 17U;

#line 5871
    uint _S295 = min((&kernelContext_37)->cluster_lights_0[base_1], 16U);

#line 5871
    TableTap_0 _S296 = table_tap_0(_S292, roughness_2, &kernelContext_37);

#line 5871
    thread TableTap_0 _S297 = _S296;

#line 5871
    float2 _S298 = dfg_at_0(&_S297, &kernelContext_37);

#line 5882
    float _S299 = _S298.x;

#line 5882
    float _S300 = _S298.y;

#line 5882
    float3 _S301 = f0_2 * float3(_S299)  + float3((f90_1 * _S300)) ;

#line 5888
    float3 _S302 = float3(0.0f, 0.0f, 0.0f);

#line 5888
    float3 sun_cascade_tint_0 = float3(1.0f, 1.0f, 1.0f);

#line 5888
    uint slot_0 = 0U;

#line 5888
    float3 direct_0 = _S302;

#line 5888
    float3 gloss_0 = _S302;

#line 5898
    for(;;)
    {

#line 5898
        if(slot_0 < _S295)
        {
        }
        else
        {

#line 5898
            break;
        }

#line 5898
        thread GpuLight_natural_0 _S303 = (&kernelContext_37)->lights_0[(&kernelContext_37)->cluster_lights_0[base_1 + 1U + slot_0]];

#line 5909
        bool fill_0 = (((&_S303)->flags_3) & 1U) != 0U;

#line 5909
        uint _S304 = (&_S303)->kind_0;

#line 5916
        bool _S305 = ((&_S303)->kind_0) == 0U;

#line 5916
        float3 to_light_5;

#line 5916
        float reach_2;

#line 5916
        if(_S305)
        {

#line 5916
            to_light_5 = normalize((float4((&_S303)->direction_0) ).xyz);

#line 5916
            reach_2 = 1.0f;

#line 5916
        }
        else
        {


            if(_S304 == 3U)
            {

#line 5921
                float4 _S306 = float4((&_S303)->position_0) ;

#line 5929
                float3 offset_2 = _S306.xyz - _S272.world_position_15;
                float distance_3 = length(offset_2);

                float _S307 = range_window_0(distance_3, _S306.w);

#line 5932
                to_light_5 = offset_2 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 5932
                reach_2 = _S307;

#line 5921
            }
            else
            {

#line 5921
                float4 _S308 = float4((&_S303)->position_0) ;

#line 5936
                float3 offset_3 = _S308.xyz - _S272.world_position_15;
                float distance_4 = length(offset_3);
                float3 to_light_6 = offset_3 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_3 = punctual_falloff_0(distance_4, _S308.w);
                if(_S304 == 2U)
                {

#line 5940
                    float4 _S309 = float4((&_S303)->direction_0) ;

#line 5940
                    reach_2 = reach_3 * spot_cone_0(to_light_6, _S309.xyz, _S309.w, (&_S303)->cos_inner_0);

#line 5940
                }
                else
                {

#line 5940
                    reach_2 = reach_3;

#line 5940
                }

#line 5940
                to_light_5 = to_light_6;

#line 5921
            }

#line 5916
        }

#line 5949
        float n_dot_l_3 = dot(_S281, to_light_5);

#line 5949
        float3 specular_0;

#line 5949
        float diffuse_0;


        if(_S304 == 3U)
        {

#line 5962
            thread array<float3, int(4)> corners_2;

#line 5962
            rect_corners_0(&_S303, _S272.world_position_15, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S281, to_eye_1, _S292);

#line 5964
            thread array<float3, int(4)> _S310 = corners_2;

#line 5964
            float _S311 = ltc_irradiance_0(to_local_0, &_S310);

            if(fill_0)
            {

#line 5966
                specular_0 = _S302;

#line 5966
            }
            else
            {

#line 5966
                thread TableTap_0 _S312 = _S296;

#line 5966
                float4 _S313 = ltc_at_0(&_S312, &kernelContext_37);

#line 5972
                matrix<float,int(3),int(3)>  _S314 = (((to_local_0) * (ltc_transform_0(_S313))));

#line 5972
                thread array<float3, int(4)> _S315 = corners_2;

#line 5972
                float _S316 = ltc_irradiance_0(_S314, &_S315);

#line 5972
                specular_0 = float3(_S316)  * _S301;

#line 5966
            }

#line 5966
            diffuse_0 = _S311;

#line 5952
        }
        else
        {

#line 5979
            float _S317 = max(n_dot_l_3, 0.0f);

            if(fill_0)
            {

#line 5981
                specular_0 = _S302;

#line 5981
            }
            else
            {

#line 5992
                float3 half_vector_0 = normalize(to_light_5 + to_eye_1);

#line 5992
                specular_0 = ggx_lobe_0(_S287, f0_2, f90_1, _S317, _S292, max(dot(_S281, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S317) ;

#line 5981
            }

#line 5981
            diffuse_0 = _S317;

#line 5952
        }

#line 5952
        float reach_4;

#line 6017
        if(_S305)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 6020
            float _S318 = sun_visibility_0(_S272.world_position_15, to_light_5, n_dot_l_3, _S283, _S293, &sun_cascade_0, &sun_fade_0, &kernelContext_37);

#line 6020
            float _S319 = contact_at_0(_S293, &kernelContext_37);

#line 6029
            float _S320 = _S318 * _S319;

#line 6029
            sun_cascade_tint_0 = cascade_tint_0(sun_cascade_0, sun_fade_0);

#line 6029
            reach_4 = _S320;

#line 6017
        }
        else
        {

#line 6040
            if(_S304 == 1U)
            {

#line 6040
                uint _S321 = (&_S303)->shadow_tile_0;

#line 6052
                if(((&_S303)->shadow_tile_0) <= 8U)
                {



                    if(n_dot_l_3 > 0.0f)
                    {
                        thread uint face_1;

#line 6059
                        float _S322 = point_map_world_0(&_S303, _S272.world_position_15, &face_1);

#line 6059
                        float _S323 = punctual_visibility_0(_S321 + face_1, _S272.world_position_15, to_light_5, n_dot_l_3, _S322, _S283, _S293, &kernelContext_37);

#line 6059
                        reach_4 = reach_2 * _S323;

#line 6057
                    }
                    else
                    {

#line 6057
                        reach_4 = reach_2;

#line 6057
                    }

#line 6052
                }
                else
                {

#line 6052
                    reach_4 = reach_2;

#line 6052
                }

#line 6040
            }
            else
            {

#line 6040
                uint _S324 = (&_S303)->shadow_tile_0;

#line 6068
                if(((&_S303)->shadow_tile_0) < 14U)
                {
                    if(n_dot_l_3 > 0.0f)
                    {

#line 6070
                        float _S325 = spot_map_world_0(&_S303, _S272.world_position_15);

#line 6070
                        float _S326 = punctual_visibility_0(_S324, _S272.world_position_15, to_light_5, n_dot_l_3, _S325, _S283, _S293, &kernelContext_37);

#line 6070
                        reach_4 = reach_2 * _S326;

#line 6070
                    }
                    else
                    {

#line 6070
                        reach_4 = reach_2;

#line 6070
                    }

#line 6068
                }
                else
                {

#line 6068
                    reach_4 = reach_2;

#line 6068
                }

#line 6040
            }

#line 6017
        }

#line 6080
        float3 _S327 = (float4((&_S303)->color_0) ).xyz;

#line 6080
        float3 direct_1 = direct_0 + _S327 * float3((diffuse_0 * reach_4)) ;
        float3 gloss_1 = gloss_0 + _S327 * (specular_0 * float3(reach_4) );

#line 5898
        slot_0 = slot_0 + 1U;

#line 5898
        direct_0 = direct_1;

#line 5898
        gloss_0 = gloss_1;

#line 5898
    }

#line 6100
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S299 + _S300);

#line 6100
    float4 _S328 = occlusion_at_0(_S293, &kernelContext_37);

#line 6119
    float occluded_0 = _S328.x;

#line 6128
    float3 bent_normal_0 = bent_normal_at_0(_S328, _S281);

#line 6152
    float3 _S329 = frame_5->ambient_0.xyz;

#line 6152
    float3 _S330 = sky_irradiance_0(bent_normal_0, &kernelContext_37);

#line 6152
    float3 _S331 = _S329 + _S330;

#line 6152
    float3 _S332 = probe_irradiance_0(_S272.world_position_15, bent_normal_0, &kernelContext_37);

#line 6208
    float3 lit_1 = diffuse_albedo_0 * ((_S331 + _S332) * (multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) * float3(_S284.x) ) + direct_0) + gloss_2;

#line 6208
    float3 _S333 = emissive_of_0(&_S275);

#line 6250
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_37)->frame_0->fog_params_0.x, (&kernelContext_37)->frame_0->fog_params_0.y, (&kernelContext_37)->frame_0->camera_position_0.y - (&kernelContext_37)->frame_0->fog_params_0.z, _S272.world_position_15.y - (&kernelContext_37)->frame_0->fog_params_0.z, length((&kernelContext_37)->frame_0->camera_position_0.xyz - _S272.world_position_15)));
    float3 lit_2 = (lit_1 + _S333 * _S285.xyz) * float3(fog_survives_0)  + (&kernelContext_37)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;

    thread FragmentOutput_0 output_2;

#line 6263
    (&output_2)->lit_0 = float4(lit_2, 1.0f);

#line 6271
    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;

#line 6281
    if((frame_5->ambient_0.w) <= -0.5f)
    {
        (&output_2)->lit_0 = float4(lit_2 * sun_cascade_tint_0, 1.0f);

#line 6290
        (&output_2)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);

#line 6281
    }

#line 6292
    return output_2;
}


#line 6292
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


#line 6325
[[fragment]] void depthMaskedFragmentMain(pixelInput_1 _S334 [[stage_in]], float4 position_6 [[position]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_4 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_4 [[texture(9)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 6325
    thread KernelContext_0 kernelContext_38;

#line 6325
    (&kernelContext_38)->draw_0 = draw_4;

#line 6325
    (&kernelContext_38)->visible_instances_0 = visible_instances_4;

#line 6325
    (&kernelContext_38)->instances_0 = instances_4;

#line 6325
    (&kernelContext_38)->meshes_0 = meshes_4;

#line 6325
    (&kernelContext_38)->frame_0 = frame_7;

#line 6325
    (&kernelContext_38)->vertices_0 = vertices_4;

#line 6325
    (&kernelContext_38)->ambient_occlusion_0 = ambient_occlusion_4;

#line 6325
    (&kernelContext_38)->materials_0 = materials_4;

#line 6325
    (&kernelContext_38)->base_color_textures_0 = base_color_textures_4;

#line 6325
    (&kernelContext_38)->base_color_sampler_0 = base_color_sampler_4;

#line 6325
    (&kernelContext_38)->normal_textures_0 = normal_textures_4;

#line 6325
    (&kernelContext_38)->mro_textures_0 = mro_textures_4;

#line 6325
    (&kernelContext_38)->emissive_textures_0 = emissive_textures_4;

#line 6325
    (&kernelContext_38)->cluster_lights_0 = cluster_lights_4;

#line 6325
    (&kernelContext_38)->specular_dfg_0 = specular_dfg_4;

#line 6325
    (&kernelContext_38)->lights_0 = lights_4;

#line 6325
    (&kernelContext_38)->ltc_matrix_0 = ltc_matrix_4;

#line 6325
    (&kernelContext_38)->shadow_atlas_0 = shadow_atlas_4;

#line 6325
    (&kernelContext_38)->shadow_sampler_0 = shadow_sampler_4;

#line 6325
    (&kernelContext_38)->contact_shadow_0 = contact_shadow_4;

#line 6325
    (&kernelContext_38)->probes_0 = probes_4;

#line 6325
    (&kernelContext_38)->probe_visibility_0 = probe_visibility_4;

#line 6325
    thread GpuMaterial_natural_0 _S335 = materials_4[_S334.material_17];

#line 6325
    float2 uv_8;

#line 6334
    if(((&_S335)->tiling_0) == 1U)
    {

#line 6334
        uv_8 = physical_tile_uv_0(_S334.world_position_16, normalize(_S334.world_normal_2), (&_S335)->tile_metres_0);

#line 6334
    }
    else
    {

#line 6334
        uv_8 = _S334.uv_7;

#line 6334
    }

#line 6334
    float4 _S336 = base_color_texel_0(&_S335, uv_8, &kernelContext_38);

#line 6334
    bool _S337 = alpha_masked_0(&_S335, _S334.color_4.w * (float4((&_S335)->base_color_0) ).w * _S336.w);

#line 6343
    if(_S337)
    {
        discard_fragment();

#line 6343
    }



    return;
}


#line 6377
struct RsmOutput_0
{
    float4 albedo_2 [[color(0)]];
    float4 normal_14 [[color(1)]];
    float4 world_0 [[color(2)]];
};


#line 6377
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


#line 6420
[[fragment]] RsmOutput_0 rsmFragmentMain(pixelInput_2 _S338 [[stage_in]], bool front_facing_2 [[front_facing]], float4 position_7 [[position]], DrawConstants_0 constant* draw_5 [[buffer(3)]], uint device* visible_instances_5 [[buffer(5)]], GpuInstance_natural_0 device* instances_5 [[buffer(2)]], GpuMesh_0 device* meshes_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_9 [[buffer(0)]], uint device* vertices_5 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_5 [[texture(2)]], GpuMaterial_natural_0 device* materials_5 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_5 [[texture(0)]], sampler base_color_sampler_5 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_5 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_5 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_5 [[texture(9)]], uint device* cluster_lights_5 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_5 [[texture(3)]], GpuLight_natural_0 device* lights_5 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_5 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_5 [[texture(6)]], GpuProbe_natural_0 device* probes_5 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_5 [[texture(7)]])
{

#line 6420
    thread KernelContext_0 kernelContext_39;

#line 6420
    (&kernelContext_39)->draw_0 = draw_5;

#line 6420
    (&kernelContext_39)->visible_instances_0 = visible_instances_5;

#line 6420
    (&kernelContext_39)->instances_0 = instances_5;

#line 6420
    (&kernelContext_39)->meshes_0 = meshes_5;

#line 6420
    (&kernelContext_39)->frame_0 = frame_9;

#line 6420
    (&kernelContext_39)->vertices_0 = vertices_5;

#line 6420
    (&kernelContext_39)->ambient_occlusion_0 = ambient_occlusion_5;

#line 6420
    (&kernelContext_39)->materials_0 = materials_5;

#line 6420
    (&kernelContext_39)->base_color_textures_0 = base_color_textures_5;

#line 6420
    (&kernelContext_39)->base_color_sampler_0 = base_color_sampler_5;

#line 6420
    (&kernelContext_39)->normal_textures_0 = normal_textures_5;

#line 6420
    (&kernelContext_39)->mro_textures_0 = mro_textures_5;

#line 6420
    (&kernelContext_39)->emissive_textures_0 = emissive_textures_5;

#line 6420
    (&kernelContext_39)->cluster_lights_0 = cluster_lights_5;

#line 6420
    (&kernelContext_39)->specular_dfg_0 = specular_dfg_5;

#line 6420
    (&kernelContext_39)->lights_0 = lights_5;

#line 6420
    (&kernelContext_39)->ltc_matrix_0 = ltc_matrix_5;

#line 6420
    (&kernelContext_39)->shadow_atlas_0 = shadow_atlas_5;

#line 6420
    (&kernelContext_39)->shadow_sampler_0 = shadow_sampler_5;

#line 6420
    (&kernelContext_39)->contact_shadow_0 = contact_shadow_5;

#line 6420
    (&kernelContext_39)->probes_0 = probes_5;

#line 6420
    (&kernelContext_39)->probe_visibility_0 = probe_visibility_5;

#line 6425
    float3 vertex_normal_1 = normalize(_S338.world_normal_3);

#line 6425
    thread GpuMaterial_natural_0 _S339 = materials_5[_S338.material_18];

#line 6425
    float2 uv_10;

#line 6432
    if(((&_S339)->tiling_0) == 1U)
    {

#line 6432
        uv_10 = physical_tile_uv_0(_S338.world_position_17, vertex_normal_1, (&_S339)->tile_metres_0);

#line 6432
    }
    else
    {

#line 6432
        uv_10 = _S338.uv_9;

#line 6432
    }

#line 6432
    float4 _S340 = base_color_texel_0(&_S339, uv_10, &kernelContext_39);

#line 6437
    float4 albedo_3 = _S338.color_5 * float4((&_S339)->base_color_0)  * _S340;

#line 6437
    bool _S341 = alpha_masked_0(&_S339, albedo_3.w);

#line 6443
    if(_S341)
    {
        discard_fragment();

#line 6443
    }

#line 6448
    thread RsmOutput_0 written_0;

#line 6459
    float3 _S342 = albedo_3.xyz;

#line 6459
    float4 _S343 = mro_texel_0(&_S339, uv_10, &kernelContext_39);

#line 6459
    float _S344 = metallic_of_0(&_S339, _S343);

#line 6459
    float3 _S345 = _S342 * float3((1.0f - _S344)) ;

#line 6459
    float _S346 = dielectric_diffuse_weight_0(&_S339);

#line 6458
    (&written_0)->albedo_2 = float4(_S345 * float3(_S346) , 1.0f);

#line 6458
    float3 _S347 = double_sided_normal_0(&_S339, vertex_normal_1, front_facing_2);

#line 6458
    float3 _S348 = float3(0.5f) ;

#line 6467
    (&written_0)->normal_14 = float4(_S347 * _S348 + _S348, 1.0f);

    (&written_0)->world_0 = float4(_S338.world_position_17, 1.0f);
    return written_0;
}


#line 6470
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


#line 6470
[[vertex]] vertexMain_Result_0 vertexMain(uint index_8 [[vertex_id]], uint instance_id_1 [[instance_id]], uint base_instance_1 [[base_instance]], DrawConstants_0 constant* draw_6 [[buffer(3)]], uint device* visible_instances_6 [[buffer(5)]], GpuInstance_natural_0 device* instances_6 [[buffer(2)]], GpuMesh_0 device* meshes_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_11 [[buffer(0)]], uint device* vertices_6 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_6 [[texture(2)]], GpuMaterial_natural_0 device* materials_6 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_6 [[texture(0)]], sampler base_color_sampler_6 [[sampler(0)]], texture2d_array<float, access::sample> normal_textures_6 [[texture(4)]], texture2d_array<float, access::sample> mro_textures_6 [[texture(8)]], texture2d_array<float, access::sample> emissive_textures_6 [[texture(9)]], uint device* cluster_lights_6 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_6 [[texture(3)]], GpuLight_natural_0 device* lights_6 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_6 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_6 [[texture(6)]], GpuProbe_natural_0 device* probes_6 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_6 [[texture(7)]])
{

#line 6470
    thread KernelContext_0 kernelContext_40;

#line 6470
    (&kernelContext_40)->draw_0 = draw_6;

#line 6470
    (&kernelContext_40)->visible_instances_0 = visible_instances_6;

#line 6470
    (&kernelContext_40)->instances_0 = instances_6;

#line 6470
    (&kernelContext_40)->meshes_0 = meshes_6;

#line 6470
    (&kernelContext_40)->frame_0 = frame_11;

#line 6470
    (&kernelContext_40)->vertices_0 = vertices_6;

#line 6470
    (&kernelContext_40)->ambient_occlusion_0 = ambient_occlusion_6;

#line 6470
    (&kernelContext_40)->materials_0 = materials_6;

#line 6470
    (&kernelContext_40)->base_color_textures_0 = base_color_textures_6;

#line 6470
    (&kernelContext_40)->base_color_sampler_0 = base_color_sampler_6;

#line 6470
    (&kernelContext_40)->normal_textures_0 = normal_textures_6;

#line 6470
    (&kernelContext_40)->mro_textures_0 = mro_textures_6;

#line 6470
    (&kernelContext_40)->emissive_textures_0 = emissive_textures_6;

#line 6470
    (&kernelContext_40)->cluster_lights_0 = cluster_lights_6;

#line 6470
    (&kernelContext_40)->specular_dfg_0 = specular_dfg_6;

#line 6470
    (&kernelContext_40)->lights_0 = lights_6;

#line 6470
    (&kernelContext_40)->ltc_matrix_0 = ltc_matrix_6;

#line 6470
    (&kernelContext_40)->shadow_atlas_0 = shadow_atlas_6;

#line 6470
    (&kernelContext_40)->shadow_sampler_0 = shadow_sampler_6;

#line 6470
    (&kernelContext_40)->contact_shadow_0 = contact_shadow_6;

#line 6470
    (&kernelContext_40)->probes_0 = probes_6;

#line 6470
    (&kernelContext_40)->probe_visibility_0 = probe_visibility_6;

#line 6470
    uint _S349 = instance_id_1 - base_instance_1;

#line 6470
    DrawSource_0 _S350 = draw_source_0(0U, &kernelContext_40);

#line 6470
    GpuInstance_natural_0 device* _S351 = (&kernelContext_40)->instances_0+(&kernelContext_40)->visible_instances_0[_S350.run_0 + _S349];

#line 2286
    GpuMesh_0 mesh_4 = (&kernelContext_40)->meshes_0[_S350.mesh_0];

#line 2294
    bool _S352 = ((_S351->flags_0) & 2U) != 0U;

#line 2294
    uint base_vertex_3;
    if(_S352)
    {

#line 2295
        base_vertex_3 = _S351->base_vertex_0;

#line 2295
    }
    else
    {

#line 2295
        base_vertex_3 = mesh_4.base_vertex_1;

#line 2295
    }

#line 2295
    MeshVertex_0 _S353 = load_vertex_0(index_8 + base_vertex_3, float4(mesh_4.uv_scale_u_0, mesh_4.uv_scale_v_0, mesh_4.uv_offset_u_0, mesh_4.uv_offset_v_0), &kernelContext_40);

#line 2295
    uint previous_base_0;

#line 2308
    if(_S352)
    {

#line 2308
        previous_base_0 = _S351->previous_base_vertex_0;

#line 2308
    }
    else
    {

#line 2308
        previous_base_0 = base_vertex_3;

#line 2308
    }

#line 2308
    float3 _S354 = load_position_0(index_8 + previous_base_0, &kernelContext_40);

#line 2308
    matrix<float,int(4),int(4)>  _S355 = matrix<float,int(4),int(4)> (_S351->transform_0.data_0[int(0)][int(0)], _S351->transform_0.data_0[int(1)][int(0)], _S351->transform_0.data_0[int(2)][int(0)], _S351->transform_0.data_0[int(3)][int(0)], _S351->transform_0.data_0[int(0)][int(1)], _S351->transform_0.data_0[int(1)][int(1)], _S351->transform_0.data_0[int(2)][int(1)], _S351->transform_0.data_0[int(3)][int(1)], _S351->transform_0.data_0[int(0)][int(2)], _S351->transform_0.data_0[int(1)][int(2)], _S351->transform_0.data_0[int(2)][int(2)], _S351->transform_0.data_0[int(3)][int(2)], _S351->transform_0.data_0[int(0)][int(3)], _S351->transform_0.data_0[int(1)][int(3)], _S351->transform_0.data_0[int(2)][int(3)], _S351->transform_0.data_0[int(3)][int(3)]);



    float4 world_1 = (((float4(_S353.position_1, 1.0f)) * (_S355)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_1) * (matrix<float,int(4),int(4)> ((&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_40)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_1.xyz;

#line 2322
    matrix<float,int(3),int(3)>  _S356 = matrix<float,int(3),int(3)> (_S355[int(0)].xyz, _S355[int(1)].xyz, _S355[int(2)].xyz);

#line 2322
    (&output_3)->world_normal_0 = (((_S353.basis_1.normal_0) * (normal_basis_0(_S356))));

#line 2328
    (&output_3)->world_tangent_0 = (((_S353.basis_1.tangent_1) * (_S356)));

#line 2328
    thread TangentFrame_0 _S357 = _S353.basis_1;

#line 2328
    uint _S358 = frame_word_0(mesh_4.flags_1, &_S357);
    (&output_3)->frame_3 = _S358;

#line 2329
    float4 _S359;

#line 2336
    if(((&kernelContext_40)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 2336
        _S359 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 2336
    }
    else
    {

#line 2336
        _S359 = _S353.color_1;

#line 2336
    }

#line 2335
    (&output_3)->color_2 = _S359;

#line 2342
    (&output_3)->material_6 = _S351->material_0;
    (&output_3)->uv_1 = _S353.uv0_0;

#line 2349
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S354, 1.0f)) * (matrix<float,int(4),int(4)> (_S351->previous_transform_0.data_0[int(0)][int(0)], _S351->previous_transform_0.data_0[int(1)][int(0)], _S351->previous_transform_0.data_0[int(2)][int(0)], _S351->previous_transform_0.data_0[int(3)][int(0)], _S351->previous_transform_0.data_0[int(0)][int(1)], _S351->previous_transform_0.data_0[int(1)][int(1)], _S351->previous_transform_0.data_0[int(2)][int(1)], _S351->previous_transform_0.data_0[int(3)][int(1)], _S351->previous_transform_0.data_0[int(0)][int(2)], _S351->previous_transform_0.data_0[int(1)][int(2)], _S351->previous_transform_0.data_0[int(2)][int(2)], _S351->previous_transform_0.data_0[int(3)][int(2)], _S351->previous_transform_0.data_0[int(0)][int(3)], _S351->previous_transform_0.data_0[int(1)][int(3)], _S351->previous_transform_0.data_0[int(2)][int(3)], _S351->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_40)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S360 = output_3;

#line 2353
    thread vertexMain_Result_0 _S361;

#line 2353
    (&_S361)->position_8 = _S360.position_3;

#line 2353
    (&_S361)->world_position_18 = _S360.world_position_1;

#line 2353
    (&_S361)->world_normal_4 = _S360.world_normal_0;

#line 2353
    (&_S361)->color_6 = _S360.color_2;

#line 2353
    (&_S361)->material_19 = _S360.material_6;

#line 2353
    (&_S361)->uv_11 = _S360.uv_1;

#line 2353
    (&_S361)->clip_position_4 = _S360.clip_position_0;

#line 2353
    (&_S361)->previous_clip_position_4 = _S360.previous_clip_position_0;

#line 2353
    (&_S361)->world_tangent_4 = _S360.world_tangent_0;

#line 2353
    (&_S361)->frame_10 = _S360.frame_3;

#line 2353
    return _S361;
}

