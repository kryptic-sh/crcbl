#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2326 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2321
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2593
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2653
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2806
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2668
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2696
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1087
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1630
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1630
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


#line 1636
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1636
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


#line 1494
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1617
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1617
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1619
        word_4 = 1U;

#line 1619
    }
    else
    {

#line 1619
        word_4 = 0U;

#line 1619
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1623
        word_4 = word_4 | 2U;

#line 1623
    }

#line 1622
    return word_4;
}


#line 1622
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1737
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 1737
    thread KernelContext_0 kernelContext_2;

#line 1737
    (&kernelContext_2)->draw_0 = draw_1;

#line 1737
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1737
    (&kernelContext_2)->instances_0 = instances_1;

#line 1737
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1737
    (&kernelContext_2)->frame_0 = frame_1;

#line 1737
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1737
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1737
    (&kernelContext_2)->materials_0 = materials_1;

#line 1737
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1737
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1737
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1737
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1737
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1737
    (&kernelContext_2)->lights_0 = lights_1;

#line 1737
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1737
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1737
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1737
    (&kernelContext_2)->probes_0 = probes_1;

#line 1737
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1740
    uint base_vertex_2;

#line 1746
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1746
        base_vertex_2 = _S7->base_vertex_0;

#line 1746
    }
    else
    {

#line 1746
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1746
    }

#line 1746
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1746
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1746
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 1749
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 1770
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1770
    thread KernelContext_0 kernelContext_3;

#line 1770
    (&kernelContext_3)->draw_0 = draw_2;

#line 1770
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 1770
    (&kernelContext_3)->instances_0 = instances_2;

#line 1770
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 1770
    (&kernelContext_3)->frame_0 = frame_2;

#line 1770
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 1770
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1770
    (&kernelContext_3)->materials_0 = materials_2;

#line 1770
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 1770
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 1770
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 1770
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 1770
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 1770
    (&kernelContext_3)->lights_0 = lights_2;

#line 1770
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 1770
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 1770
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 1770
    (&kernelContext_3)->probes_0 = probes_2;

#line 1770
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 4054
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 4056
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4022
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4022
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4028
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)).x);
}


#line 3772
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 3776
    float _S16 = axis_0.y;

#line 3776
    bool _S17;

#line 3776
    if(_S15 >= _S16)
    {

#line 3776
        _S17 = _S15 >= (axis_0.z);

#line 3776
    }
    else
    {

#line 3776
        _S17 = false;

#line 3776
    }

#line 3776
    float2 planar_0;

#line 3776
    if(_S17)
    {

#line 3776
        planar_0 = world_position_0.zy;

#line 3776
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 3780
            planar_0 = world_position_0.xz;

#line 3780
        }
        else
        {

#line 3780
            planar_0 = world_position_0.xy;

#line 3780
        }

#line 3776
    }

#line 3788
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 941
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 3809
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S18 = normal_2.z;

#line 3811
    float sign_z_0;

#line 3811
    if(_S18 >= 0.0f)
    {

#line 3811
        sign_z_0 = 1.0f;

#line 3811
    }
    else
    {

#line 3811
        sign_z_0 = -1.0f;

#line 3811
    }
    float a_0 = -1.0f / (sign_z_0 + _S18);
    float _S19 = normal_2.x;

#line 3813
    float _S20 = sign_z_0 * _S19;

#line 3813
    return float3(1.0f + _S20 * _S19 * a_0, _S20 * normal_2.y * a_0, - sign_z_0 * _S19);
}


#line 3863
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S21 = duvdy_0.y;

#line 3865
    float _S22 = duvdx_0.y;

#line 3865
    float winding_0;
    if((duvdx_0.x * _S21 - duvdy_0.x * _S22) < 0.0f)
    {

#line 3866
        winding_0 = -1.0f;

#line 3866
    }
    else
    {

#line 3866
        winding_0 = 1.0f;

#line 3866
    }
    float3 tangent_2 = (float3(_S21)  * dpdx_0 - float3(_S22)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 3875
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 3876
    float3 _S23;

#line 3885
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 3885
        _S23 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 3885
    }
    else
    {

#line 3885
        _S23 = orthonormal_tangent_0(normal_3);

#line 3885
    }

#line 3885
    (&basis_4)->tangent_1 = _S23;

    (&basis_4)->bitangent_0 = cross(normal_3, _S23);
    return basis_4;
}


#line 1501
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


#line 3945
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 3957
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 3967
    uint _S24 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 3976
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 3978
        float3 _S25;

#line 3983
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 3983
            _S25 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 3983
        }
        else
        {

#line 3983
            _S25 = orthonormal_tangent_0(normal_4);

#line 3983
        }

#line 3983
        (&basis_5)->tangent_1 = _S25;

#line 3989
        float3 _S26 = cross((&basis_5)->normal_0, _S25);

#line 3989
        float _S27;
        if((_S24 & 2U) != 0U)
        {

#line 3990
            _S27 = -1.0f;

#line 3990
        }
        else
        {

#line 3990
            _S27 = 1.0f;

#line 3990
        }

#line 3989
        (&basis_5)->bitangent_0 = _S26 * float3(_S27) ;

#line 3968
    }
    else
    {

#line 3994
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 3968
    }

#line 3998
    float3 _S28 = float3(uv_1, float(layer_0));
    float3 _S29 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S28)).xy, uint(((_S28)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 3999
    thread float3 tangent_space_0 = _S29;
    tangent_space_0.xy = _S29.xy * float2(normal_scale_1) ;

#line 4005
    float3 _S30 = normalize(tangent_space_0);

#line 4005
    tangent_space_0 = _S30;
    return normalize(float3(_S30.x)  * (&basis_5)->tangent_1 + float3(_S30.y)  * (&basis_5)->bitangent_0 + float3(_S30.z)  * (&basis_5)->normal_0);
}


#line 2461
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2472
    float3 _S31;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2473
        _S31 = - facet_1;

#line 2473
    }
    else
    {

#line 2473
        _S31 = facet_1;

#line 2473
    }

#line 2473
    return _S31;
}


#line 926
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3570
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S32 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S33 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S34 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 3580
    uint _S36 = uint(pixel_0.x) / _S35;

#line 3580
    uint _S37 = min(_S36, _S32 - 1U);
    uint _S38 = uint(pixel_0.y) / _S35;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3591
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S34 - 1U))) * _S33 + min(_S38, _S33 - 1U)) * _S32 + _S37;
}


#line 1893
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 1914
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 1914
    texture2d<float, access::sample> _S39 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S39).get_width(0)),(*((&height_1)) = (_S39).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1920
    float2 _S40 = float2(1.0f) ;
    float2 _S41 = extent_1 - _S40;

#line 1921
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S41);
    float2 high_1 = min(low_1 + _S40, _S41);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 1939
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 1951
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S42 = tap_1->lo_0.x;

#line 1953
    int _S43 = tap_1->lo_0.y;

#line 1953
    int3 _S44 = int3(_S42, _S43, int(0));
    int _S45 = tap_1->hi_0.x;

#line 1954
    int3 _S46 = int3(_S45, _S43, int(0));
    float2 _S47 = float2(tap_1->weight_0.x) ;
    int _S48 = tap_1->hi_0.y;

#line 1956
    int3 _S49 = int3(_S42, _S48, int(0));
    int3 _S50 = int3(_S45, _S48, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)))), _S47), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), _S47), float2(tap_1->weight_0.y) );
}


#line 3521
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3537
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3549
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3556
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2280
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2280
    float4 _S51 = float4(light_0->tangent_0) ;

    float3 _S52 = _S51.xyz;

#line 2282
    float3 across_0 = _S52 * float3(_S51.w) ;

#line 2282
    float4 _S53 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S52, _S53.xyz) * float3(_S53.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S54 = centre_0 - across_0;

#line 2285
    (*corners_0)[int(0)] = _S54 - down_0;
    float3 _S55 = centre_0 + across_0;

#line 2286
    (*corners_0)[int(1)] = _S55 - down_0;
    (*corners_0)[int(2)] = _S55 + down_0;
    (*corners_0)[int(3)] = _S54 + down_0;
    return;
}


#line 2038
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2041
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2042
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2042
    }
    else
    {

#line 2042
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2042
    }

#line 2042
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2043
        tangent_5 = across_1 / float3(span_0) ;

#line 2043
    }
    else
    {

#line 2043
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2043
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 2019
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2109
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2109
    float3 _S56 = polygon_0->corner_0[int(0)];

#line 2109
    float3 _S57 = polygon_0->corner_0[int(1)];

#line 2109
    float3 _S58 = polygon_0->corner_0[int(2)];

#line 2109
    float3 _S59 = polygon_0->corner_0[int(3)];

#line 2115
    float3 _S60 = float3(0.0f, 0.0f, 0.0f);


    float _S61 = polygon_0->corner_0[int(0)].z;

#line 2118
    int count_1;

#line 2118
    if(_S61 > 0.0f)
    {

#line 2118
        count_1 = int(1);

#line 2118
    }
    else
    {

#line 2118
        count_1 = int(0);

#line 2118
    }
    float _S62 = _S57.z;

#line 2119
    int _S63;

#line 2119
    if(_S62 > 0.0f)
    {

#line 2119
        _S63 = int(2);

#line 2119
    }
    else
    {

#line 2119
        _S63 = int(0);

#line 2119
    }

#line 2119
    int config_0 = count_1 + _S63;
    float _S64 = _S58.z;

#line 2120
    if(_S64 > 0.0f)
    {

#line 2120
        count_1 = int(4);

#line 2120
    }
    else
    {

#line 2120
        count_1 = int(0);

#line 2120
    }

#line 2120
    int config_1 = config_0 + count_1;
    float _S65 = _S59.z;

#line 2121
    if(_S65 > 0.0f)
    {

#line 2121
        count_1 = int(8);

#line 2121
    }
    else
    {

#line 2121
        count_1 = int(0);

#line 2121
    }

#line 2121
    int config_2 = config_1 + count_1;

#line 2121
    float3 l0_0;

#line 2121
    float3 l1_0;

#line 2121
    float3 l2_0;

#line 2121
    float3 l3_0;

#line 2121
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2124
        float3 _S66 = float3(_S61) ;


        float3 _S67 = float3(- _S62)  * _S56 + _S66 * _S57;
        float3 _S68 = float3(- _S65)  * _S56 + _S66 * _S59;

#line 2128
        count_1 = int(3);

#line 2128
        l0_0 = _S56;

#line 2128
        l1_0 = _S67;

#line 2128
        l2_0 = _S68;

#line 2128
        l3_0 = _S59;

#line 2128
        l4_0 = _S60;

#line 2124
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2130
            float3 _S69 = float3(_S62) ;


            float3 _S70 = float3(- _S61)  * _S57 + _S69 * _S56;
            float3 _S71 = float3(- _S64)  * _S57 + _S69 * _S58;

#line 2134
            count_1 = int(3);

#line 2134
            l0_0 = _S70;

#line 2134
            l1_0 = _S57;

#line 2134
            l2_0 = _S71;

#line 2134
            l3_0 = _S59;

#line 2134
            l4_0 = _S60;

#line 2130
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S72 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                float3 _S73 = float3(- _S65)  * _S56 + float3(_S61)  * _S59;

#line 2140
                count_1 = int(4);

#line 2140
                l0_0 = _S56;

#line 2140
                l1_0 = _S57;

#line 2140
                l2_0 = _S72;

#line 2140
                l3_0 = _S73;

#line 2140
                l4_0 = _S60;

#line 2136
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2142
                    float3 _S74 = float3(_S64) ;


                    float3 _S75 = float3(- _S65)  * _S58 + _S74 * _S59;
                    float3 _S76 = float3(- _S62)  * _S58 + _S74 * _S57;

#line 2146
                    count_1 = int(3);

#line 2146
                    l0_0 = _S75;

#line 2146
                    l1_0 = _S76;

#line 2146
                    l2_0 = _S58;

#line 2146
                    l3_0 = _S59;

#line 2146
                    l4_0 = _S60;

#line 2142
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S77 = float3(- _S61)  * _S57 + float3(_S62)  * _S56;
                        float3 _S78 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;

#line 2152
                        count_1 = int(4);

#line 2152
                        l0_0 = _S77;

#line 2152
                        l1_0 = _S57;

#line 2152
                        l2_0 = _S58;

#line 2152
                        l3_0 = _S78;

#line 2152
                        l4_0 = _S60;

#line 2148
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2154
                            float3 _S79 = float3(- _S65) ;


                            float3 _S80 = _S79 * _S56 + float3(_S61)  * _S59;
                            float3 _S81 = _S79 * _S58 + float3(_S64)  * _S59;

#line 2158
                            count_1 = int(5);

#line 2158
                            l0_0 = _S56;

#line 2158
                            l1_0 = _S57;

#line 2158
                            l2_0 = _S58;

#line 2158
                            l3_0 = _S81;

#line 2158
                            l4_0 = _S80;

#line 2154
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2160
                                float3 _S82 = float3(_S65) ;


                                float3 _S83 = float3(- _S61)  * _S59 + _S82 * _S56;
                                float3 _S84 = float3(- _S64)  * _S59 + _S82 * _S58;

#line 2164
                                count_1 = int(3);

#line 2164
                                l0_0 = _S83;

#line 2164
                                l1_0 = _S84;

#line 2164
                                l2_0 = _S59;

#line 2164
                                l3_0 = _S59;

#line 2164
                                l4_0 = _S60;

#line 2160
                            }
                            else
                            {

#line 2167
                                if(config_2 == int(9))
                                {

                                    float3 _S85 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;
                                    float3 _S86 = float3(- _S64)  * _S59 + float3(_S65)  * _S58;

#line 2171
                                    count_1 = int(4);

#line 2171
                                    l0_0 = _S56;

#line 2171
                                    l1_0 = _S85;

#line 2171
                                    l2_0 = _S86;

#line 2171
                                    l3_0 = _S59;

#line 2171
                                    l4_0 = _S60;

#line 2167
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S87 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;
                                        float3 _S88 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;

#line 2178
                                        count_1 = int(5);

#line 2178
                                        l0_0 = _S56;

#line 2178
                                        l1_0 = _S57;

#line 2178
                                        l2_0 = _S88;

#line 2178
                                        l3_0 = _S87;

#line 2178
                                        l4_0 = _S59;

#line 2173
                                    }
                                    else
                                    {

#line 2180
                                        if(config_2 == int(12))
                                        {

                                            float3 _S89 = float3(- _S62)  * _S58 + float3(_S64)  * _S57;
                                            float3 _S90 = float3(- _S61)  * _S59 + float3(_S65)  * _S56;

#line 2184
                                            count_1 = int(4);

#line 2184
                                            l0_0 = _S90;

#line 2184
                                            l1_0 = _S89;

#line 2184
                                            l2_0 = _S58;

#line 2184
                                            l3_0 = _S59;

#line 2184
                                            l4_0 = _S60;

#line 2180
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S91 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                                                float3 _S92 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;

#line 2192
                                                count_1 = int(5);

#line 2192
                                                l0_0 = _S56;

#line 2192
                                                l1_0 = _S92;

#line 2192
                                                l2_0 = _S91;

#line 2192
                                                l3_0 = _S58;

#line 2192
                                                l4_0 = _S59;

#line 2186
                                            }
                                            else
                                            {

#line 2194
                                                if(config_2 == int(14))
                                                {

#line 2194
                                                    float3 _S93 = float3(- _S61) ;


                                                    float3 _S94 = _S93 * _S59 + float3(_S65)  * _S56;
                                                    float3 _S95 = _S93 * _S57 + float3(_S62)  * _S56;

#line 2198
                                                    count_1 = int(5);

#line 2198
                                                    l0_0 = _S95;

#line 2198
                                                    l1_0 = _S94;

#line 2194
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2200
                                                        count_1 = int(4);

#line 2200
                                                    }
                                                    else
                                                    {

#line 2200
                                                        count_1 = int(0);

#line 2200
                                                    }

#line 2200
                                                    l0_0 = _S56;

#line 2200
                                                    l1_0 = _S60;

#line 2194
                                                }

#line 2115
                                                float3 _S96 = l1_0;

#line 2115
                                                l1_0 = _S57;

#line 2115
                                                l2_0 = _S58;

#line 2115
                                                l3_0 = _S59;

#line 2115
                                                l4_0 = _S96;

#line 2186
                                            }

#line 2180
                                        }

#line 2173
                                    }

#line 2167
                                }

#line 2160
                            }

#line 2154
                        }

#line 2148
                    }

#line 2142
                }

#line 2136
            }

#line 2130
        }

#line 2124
    }

#line 2208
    if(count_1 <= int(3))
    {

#line 2208
        l3_0 = l0_0;

#line 2208
        l4_0 = l0_0;

#line 2208
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2213
            l4_0 = l0_0;

#line 2213
        }

#line 2208
    }

#line 2218
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2081
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2087
    float weight_1;

#line 2092
    if(cosine_0 > 0.0f)
    {

#line 2092
        weight_1 = fit_0;

#line 2092
    }
    else
    {

#line 2092
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2092
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2238
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2240
    int corner_1 = int(0);
    for(;;)
    {

#line 2241
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2241
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2241
        corner_1 = corner_1 + int(1);

#line 2241
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2246
    thread LtcPolygon_0 _S97 = polygon_1;

#line 2246
    LtcPolygon_0 _S98 = ltc_clip_0(&_S97);
    polygon_1 = _S98;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2250
    int at_2 = int(0);

    for(;;)
    {

#line 2252
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2252
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2252
        at_2 = at_2 + int(1);

#line 2252
    }

#line 2259
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2259
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2260
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2260
    }
    else
    {

#line 2260
        sum_1 = sum_0;

#line 2260
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2264
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2264
    }

#line 2271
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 1967
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S99 = tap_2->lo_0.x;

#line 1969
    int _S100 = tap_2->lo_0.y;

#line 1969
    int3 _S101 = int3(_S99, _S100, int(0));
    int _S102 = tap_2->hi_0.x;

#line 1970
    int3 _S103 = int3(_S102, _S100, int(0));
    float4 _S104 = float4(tap_2->weight_0.x) ;
    int _S105 = tap_2->hi_0.y;

#line 1972
    int3 _S106 = int3(_S99, _S105, int(0));
    int3 _S107 = int3(_S102, _S105, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S101)).xy), uint(((_S101)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S103)).xy), uint(((_S103)).z))), _S104), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S107)).xy), uint(((_S107)).z))), _S104), float4(tap_2->weight_0.y) );
}


#line 2054
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 1849
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 1856
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1863
    float _S108 = 1.0f - alpha2_0;

#line 1868
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S108 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S108 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2841
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 2841
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 2901
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 2873
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2512
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 2828
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 2853
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 2853
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 321
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3023
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_15)
{
    float2 texel_1 = kernelContext_15->frame_0->shadow_params_0.xy;

#line 3025
    float4 _S109 = atlas_rect_0(cascade_0, kernelContext_15);

#line 3025
    float2 _S110 = atlas_step_0(_S109, kernelContext_15);


    float2 _S111 = float2(0.5f, 0.5f) * _S110;


    float2 _S112 = float2(1.0f, 1.0f);

#line 3031
    float2 _S113 = _S112 / texel_1;

#line 3031
    uint index_2 = 0U;

#line 3031
    float sum_2 = 0.0f;

#line 3031
    float found_0 = 0.0f;



    for(;;)
    {

#line 3035
        if(index_2 < 16U)
        {
        }
        else
        {

#line 3035
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_2] * float2(8.0f) ;
        float _S114 = spoke_0.x;

#line 3038
        float _S115 = rotation_0.x;

#line 3038
        float _S116 = spoke_0.y;

#line 3038
        float _S117 = rotation_0.y;

#line 3046
        int3 _S118 = int3(int2(min(atlas_uv_0(_S109, clamp(tile_uv_1 + float2(_S114 * _S115 - _S116 * _S117, _S114 * _S117 + _S116 * _S115) * _S110, _S111, float2(1.0f)  - _S111)) * _S113, _S113 - _S112)), int(0));

#line 3046
        float depth_1 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 3050
            sum_2 = sum_2 + depth_1;

#line 3050
            found_0 = found_1;

#line 3047
        }

#line 3035
        index_2 = index_2 + 1U;

#line 3035
    }

#line 3054
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3065
    float _S119 = 2.0f * kernelContext_15->frame_0->cascade_far_0[cascade_0];

#line 3065
    float separation_0 = (sum_2 / found_0 - reference_0) * (_S119 + 40.0f);

#line 3065
    float _S120 = tile_texels_0(_S109, kernelContext_15);

    return clamp(separation_0 * 0.01999999955296516f / (_S119 / _S120), 2.0f, 8.0f);
}


#line 2923
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S121 = spoke_1.x;

#line 2928
    float _S122 = rotation_1.x;

#line 2928
    float _S123 = spoke_1.y;

#line 2928
    float _S124 = rotation_1.y;


    float _S125 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_2 + float2(_S121 * _S122 - _S123 * _S124, _S121 * _S124 + _S123 * _S122) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2931
    return _S125;
}


#line 2953
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_2, KernelContext_0 thread* kernelContext_17)
{
    float2 _S126 = shadow_rotation_0(pixel_2);

#line 2955
    float4 _S127 = atlas_rect_1(tile_2, kernelContext_17);

    if(atlas_rect_is_empty_0(_S127))
    {
        return 1.0f;
    }

#line 2959
    float2 _S128 = atlas_step_1(_S127, kernelContext_17);

#line 2959
    uint spot_0 = 0U;

#line 2959
    float probe_0 = 0.0f;

#line 2964
    for(;;)
    {

#line 2964
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2964
            break;
        }

#line 2964
        float _S129 = tile_tap_0(_S127, _S128, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S126, reference_2, kernelContext_17);

        float probe_1 = probe_0 + _S129;

#line 2964
        spot_0 = spot_0 + 1U;

#line 2964
        probe_0 = probe_1;

#line 2964
    }

#line 2973
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2979
    uint index_3 = 0U;

#line 2979
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2983
        if(index_3 < 32U)
        {
        }
        else
        {

#line 2983
            break;
        }

#line 2983
        float _S130 = tile_tap_0(_S127, _S128, tile_uv_3, SHADOW_DISC_0[index_3] * float2(radius_2) , _S126, reference_2, kernelContext_17);

        float visibility_1 = visibility_0 + _S130;

#line 2983
        index_3 = index_3 + 1U;

#line 2983
        visibility_0 = visibility_1;

#line 2983
    }

#line 2988
    return visibility_0 / 32.0f;
}


#line 3119
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_18)
{

#line 3120
    float4 _S131 = atlas_rect_0(cascade_1, kernelContext_18);

#line 3154
    if(atlas_rect_is_empty_0(_S131))
    {


        return 1.0f;
    }
    float _S132 = 2.0f * kernelContext_18->frame_0->cascade_far_0[cascade_1];

#line 3160
    float _S133 = tile_texels_0(_S131, kernelContext_18);

#line 3160
    float texel_world_0 = _S132 / _S133;

#line 3167
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_18->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_18->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3171
    bool _S134;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3172
        _S134 = true;

#line 3172
    }
    else
    {

#line 3172
        _S134 = (ndc_0.z) <= 0.0f;

#line 3172
    }

#line 3172
    if(_S134)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3199
    float _S135 = ndc_0.z;

#line 3199
    float _S136 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S135, shadow_rotation_0(pixel_3), kernelContext_18);

#line 3199
    float _S137 = tile_pcf_0(cascade_1, tile_uv_4, _S135, pixel_3, _S136, kernelContext_18);
    return _S137;
}


#line 3216
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_19)
{

#line 3217
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3229
    float eye_distance_0 = length(world_position_5 - kernelContext_19->frame_0->camera_position_0.xyz);

#line 3229
    uint index_4 = 0U;

    for(;;)
    {

#line 3231
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3231
            cascade_2 = 1U;

#line 3231
            break;
        }
        if(eye_distance_0 < kernelContext_19->frame_0->cascade_far_0[index_4])
        {

#line 3233
            cascade_2 = index_4;


            break;
        }

#line 3231
        index_4 = index_4 + 1U;

#line 3231
    }

#line 3231
    float _S138 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_19);

#line 3242
    uint _S139 = cascade_2 + 1U;

#line 3242
    if(_S139 >= 2U)
    {



        return _S138;
    }

#line 3255
    float band_0 = kernelContext_19->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_19->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S138;
    }

#line 3259
    float _S140 = cascade_visibility_0(_S139, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_19);

#line 3270
    return mix(_S138, _S140, blend_0);
}


#line 3473
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S141 = axis_2.x;

#line 3476
    float _S142 = axis_2.y;

#line 3476
    bool _S143;

#line 3476
    if(_S141 >= _S142)
    {

#line 3476
        _S143 = _S141 >= (axis_2.z);

#line 3476
    }
    else
    {

#line 3476
        _S143 = false;

#line 3476
    }

#line 3476
    uint _S144;

#line 3476
    if(_S143)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3478
            _S144 = 0U;

#line 3478
        }
        else
        {

#line 3478
            _S144 = 1U;

#line 3478
        }

#line 3478
        return _S144;
    }
    if(_S142 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3482
            _S144 = 2U;

#line 3482
        }
        else
        {

#line 3482
            _S144 = 3U;

#line 3482
        }

#line 3482
        return _S144;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3484
        _S144 = 4U;

#line 3484
    }
    else
    {

#line 3484
        _S144 = 5U;

#line 3484
    }

#line 3484
    return _S144;
}


#line 308
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 3377
float punctual_visibility_0(uint tile_4, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_20)
{

    uint atlas_0 = light_tile_0(tile_4);

#line 3380
    float4 _S145 = atlas_rect_0(atlas_0, kernelContext_20);

    if(atlas_rect_is_empty_0(_S145))
    {


        return 1.0f;
    }

#line 3386
    float _S146 = tile_texels_0(_S145, kernelContext_20);

    float texel_world_1 = map_world_0 / _S146;

#line 3398
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_20->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 3405
    float _S147 = clip_1.w;

#line 3405
    if(_S147 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S147) ;

#line 3409
    bool _S148;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3410
        _S148 = true;

#line 3410
    }
    else
    {

#line 3410
        _S148 = (ndc_1.z) <= 0.0f;

#line 3410
    }

#line 3410
    if(_S148)
    {

#line 3410
        _S148 = true;

#line 3410
    }
    else
    {

#line 3410
        _S148 = (ndc_1.z) > 1.0f;

#line 3410
    }

#line 3410
    if(_S148)
    {

#line 3417
        return 1.0f;
    }

#line 3417
    float _S149 = tile_pcf_0(atlas_0, float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_20);

#line 3427
    return _S149;
}


#line 3492
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_21)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3500
    float _S150 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_6, kernelContext_21);

#line 3506
    return _S150;
}


#line 3434
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_5, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_22)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3441
    float4 _S151 = float4(light_2->direction_0) ;

#line 3448
    float cos_outer_1 = _S151.w;

#line 3448
    float _S152 = punctual_visibility_0(tile_5, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S151.xyz)), 0.0f), geometric_normal_5, pixel_7, kernelContext_22);

#line 3455
    return _S152;
}


#line 1995
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 3699
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_23)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_23->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_23->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_23->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 978
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 3601
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_24)
{

    GpuProbe_natural_0 _S153 = kernelContext_24->probes_0[min((cell_1.z * kernelContext_24->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_24->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_24->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 3604
    GpuProbe_0 _S154 = { float4(_S153.sh_r_0) , float4(_S153.sh_g_0) , float4(_S153.sh_b_0)  };

#line 3604
    return _S154;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_1, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S155 = float4(t_1) ;

#line 3612
    (&blended_0)->sh_r_0 = mix(a_1->sh_r_0, b_0->sh_r_0, _S155);
    (&blended_0)->sh_g_0 = mix(a_1->sh_g_0, b_0->sh_g_0, _S155);
    (&blended_0)->sh_b_0 = mix(a_1->sh_b_0, b_0->sh_b_0, _S155);
    return blended_0;
}


#line 3652
float3 probe_irradiance_0(float3 world_position_9, float3 normal_7, KernelContext_0 thread* kernelContext_25)
{

#line 3652
    float3 _S156 = float3(1.0f) ;

#line 3657
    float3 _S157 = float3(0.0f, 0.0f, 0.0f);

#line 3657
    float3 last_0 = max(float3(kernelContext_25->frame_0->probe_counts_0.xyz) - _S156, _S157);
    float3 grid_0 = clamp((world_position_9 - kernelContext_25->frame_0->probe_origin_0.xyz) * kernelContext_25->frame_0->probe_inv_spacing_0.xyz, _S157, last_0);

    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S158 = uint3(base_2);



    uint3 _S159 = uint3(min(base_2 + _S156, last_0));

#line 3674
    uint _S160 = _S158.x;

#line 3674
    uint _S161 = _S158.y;

#line 3674
    uint _S162 = _S158.z;

#line 3674
    GpuProbe_0 _S163 = probe_at_0(uint3(_S160, _S161, _S162), kernelContext_25);

#line 3674
    uint _S164 = _S159.x;

#line 3674
    GpuProbe_0 _S165 = probe_at_0(uint3(_S164, _S161, _S162), kernelContext_25);

#line 3674
    float _S166 = f_0.x;

#line 3674
    thread GpuProbe_0 _S167 = _S163;

#line 3674
    thread GpuProbe_0 _S168 = _S165;

#line 3674
    GpuProbe_0 _S169 = lerp_probe_0(&_S167, &_S168, _S166);
    uint _S170 = _S159.y;

#line 3675
    GpuProbe_0 _S171 = probe_at_0(uint3(_S160, _S170, _S162), kernelContext_25);

#line 3675
    GpuProbe_0 _S172 = probe_at_0(uint3(_S164, _S170, _S162), kernelContext_25);

#line 3675
    thread GpuProbe_0 _S173 = _S171;

#line 3675
    thread GpuProbe_0 _S174 = _S172;

#line 3675
    GpuProbe_0 _S175 = lerp_probe_0(&_S173, &_S174, _S166);
    uint _S176 = _S159.z;

#line 3676
    GpuProbe_0 _S177 = probe_at_0(uint3(_S160, _S161, _S176), kernelContext_25);

#line 3676
    GpuProbe_0 _S178 = probe_at_0(uint3(_S164, _S161, _S176), kernelContext_25);

#line 3676
    thread GpuProbe_0 _S179 = _S177;

#line 3676
    thread GpuProbe_0 _S180 = _S178;

#line 3676
    GpuProbe_0 _S181 = lerp_probe_0(&_S179, &_S180, _S166);

#line 3676
    GpuProbe_0 _S182 = probe_at_0(uint3(_S160, _S170, _S176), kernelContext_25);

#line 3676
    GpuProbe_0 _S183 = probe_at_0(uint3(_S164, _S170, _S176), kernelContext_25);

#line 3676
    thread GpuProbe_0 _S184 = _S182;

#line 3676
    thread GpuProbe_0 _S185 = _S183;

#line 3676
    GpuProbe_0 _S186 = lerp_probe_0(&_S184, &_S185, _S166);

    float _S187 = f_0.y;

#line 3678
    thread GpuProbe_0 _S188 = _S169;

#line 3678
    thread GpuProbe_0 _S189 = _S175;

#line 3678
    GpuProbe_0 _S190 = lerp_probe_0(&_S188, &_S189, _S187);

#line 3678
    thread GpuProbe_0 _S191 = _S181;

#line 3678
    thread GpuProbe_0 _S192 = _S186;

#line 3678
    GpuProbe_0 _S193 = lerp_probe_0(&_S191, &_S192, _S187);

    float _S194 = f_0.z;

#line 3680
    thread GpuProbe_0 _S195 = _S190;

#line 3680
    thread GpuProbe_0 _S196 = _S193;

#line 3680
    GpuProbe_0 _S197 = lerp_probe_0(&_S195, &_S196, _S194);

    float4 basis_7 = float4(normal_7, 1.0f);
    return max(float3(dot(_S197.sh_r_0, basis_7), dot(_S197.sh_g_0, basis_7), dot(_S197.sh_b_0, basis_7)), _S157);
}


#line 951
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2346
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S198 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2354
    float kernel_0 = 0.0001984127011383f;

#line 2354
    int term_0 = int(6);

    for(;;)
    {

#line 2356
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2356
            break;
        }
        float _S199 = kernel_0 * _S198 + FOG_KERNEL_0[term_0];

#line 2356
        int term_1 = term_0 - int(1);

#line 2356
        kernel_0 = _S199;

#line 2356
        term_0 = term_1;

#line 2356
    }

#line 2363
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2373
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S200 = - d_0;

#line 2377
        float series_0 = 0.00833333376795053f;

#line 2377
        int term_2 = int(3);

        for(;;)
        {

#line 2379
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2379
                break;
            }
            float _S201 = series_0 * _S200 + FOG_RATIO_KERNEL_0[term_2];

#line 2379
            int term_3 = term_2 - int(1);

#line 2379
            series_0 = _S201;

#line 2379
            term_2 = term_3;

#line 2379
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2407
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2418
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2426
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 3725
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 3725
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


#line 4064
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S202 [[stage_in]], float4 position_4 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]])
{

#line 4064
    thread KernelContext_0 kernelContext_26;

#line 4064
    (&kernelContext_26)->draw_0 = draw_3;

#line 4064
    (&kernelContext_26)->visible_instances_0 = visible_instances_3;

#line 4064
    (&kernelContext_26)->instances_0 = instances_3;

#line 4064
    (&kernelContext_26)->meshes_0 = meshes_3;

#line 4064
    (&kernelContext_26)->frame_0 = frame_5;

#line 4064
    (&kernelContext_26)->vertices_0 = vertices_3;

#line 4064
    (&kernelContext_26)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4064
    (&kernelContext_26)->materials_0 = materials_3;

#line 4064
    (&kernelContext_26)->normal_textures_0 = normal_textures_3;

#line 4064
    (&kernelContext_26)->base_color_sampler_0 = base_color_sampler_3;

#line 4064
    (&kernelContext_26)->base_color_textures_0 = base_color_textures_3;

#line 4064
    (&kernelContext_26)->cluster_lights_0 = cluster_lights_3;

#line 4064
    (&kernelContext_26)->specular_dfg_0 = specular_dfg_3;

#line 4064
    (&kernelContext_26)->lights_0 = lights_3;

#line 4064
    (&kernelContext_26)->ltc_matrix_0 = ltc_matrix_3;

#line 4064
    (&kernelContext_26)->shadow_atlas_0 = shadow_atlas_3;

#line 4064
    (&kernelContext_26)->shadow_sampler_0 = shadow_sampler_3;

#line 4064
    (&kernelContext_26)->probes_0 = probes_3;

#line 4076
    float3 vertex_normal_0 = normalize(_S202.world_normal_1);

#line 4081
    float2 motion_1 = motion_vector_0(_S202.clip_position_1, _S202.previous_clip_position_1);

#line 4090
    if((frame_5->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 4132
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 4132
        float _S203 = occlusion_at_0(position_4.xy, &kernelContext_26);

        thread FragmentOutput_0 occlusion_0;

#line 4143
        (&occlusion_0)->lit_0 = float4(_S203, _S203, _S203, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S202.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 4160
    thread GpuMaterial_natural_0 _S204 = (&kernelContext_26)->materials_0[_S202.material_5];

#line 4160
    float2 uv_3;

#line 4185
    if(((&_S204)->tiling_0) == 1U)
    {

#line 4185
        uv_3 = physical_tile_uv_0(_S202.world_position_10, vertex_normal_0, (&_S204)->tile_metres_0);

#line 4185
    }
    else
    {

#line 4185
        uv_3 = _S202.uv_2;

#line 4185
    }

#line 4185
    uint _S205 = normal_layer_0(&_S204);

#line 4185
    thread VertexOutput_0 _S206;

#line 4185
    (&_S206)->position_3 = position_4;

#line 4185
    (&_S206)->world_position_1 = _S202.world_position_10;

#line 4185
    (&_S206)->world_normal_0 = _S202.world_normal_1;

#line 4185
    (&_S206)->color_2 = _S202.color_3;

#line 4185
    (&_S206)->material_2 = _S202.material_5;

#line 4185
    (&_S206)->uv_0 = _S202.uv_2;

#line 4185
    (&_S206)->clip_position_0 = _S202.clip_position_1;

#line 4185
    (&_S206)->previous_clip_position_0 = _S202.previous_clip_position_1;

#line 4185
    (&_S206)->world_tangent_0 = _S202.world_tangent_1;

#line 4185
    (&_S206)->frame_3 = _S202.frame_4;

#line 4185
    float3 _S207 = shading_normal_of_0(_S205, (&_S204)->normal_scale_0, &_S206, vertex_normal_0, uv_3, &kernelContext_26);

#line 4192
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 4194
        float3 _S208 = float3(0.5f) ;

#line 4206
        (&normals_0)->lit_0 = float4(_S207 * _S208 + _S208, 1.0f);

#line 4212
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_26)->frame_0->camera_position_0.xyz - _S202.world_position_10);



    float3 _S209 = geometric_normal_of_0(_S202.world_position_10, vertex_normal_0);

#line 4221
    uint _S210 = base_color_layer_0(&_S204);

#line 4236
    float3 _S211 = float3(uv_3, float(_S210));
    float4 albedo_0 = _S202.color_3 * float4((&_S204)->base_color_0)  * (((&kernelContext_26)->base_color_textures_0).sample(((&kernelContext_26)->base_color_sampler_0), ((_S211)).xy, uint(((_S211)).z)));

#line 4243
    float metallic_1 = saturate((&_S204)->metallic_0);
    float roughness_2 = clamp((&_S204)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S212 = alpha_0 * alpha_0;

#line 4252
    float3 _S213 = albedo_0.xyz;

#line 4252
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S213, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S213 * float3((1.0f - metallic_1)) ;

#line 4259
    float _S214 = max(dot(_S207, to_eye_1), 0.00009999999747379f);

#line 4269
    float2 _S215 = position_4.xy;

#line 4269
    uint _S216 = froxel_of_0(_S215, (((float4(_S202.world_position_10, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_26);

#line 4269
    uint base_3 = _S216 * 17U;

#line 4274
    uint _S217 = min((&kernelContext_26)->cluster_lights_0[base_3], 16U);

#line 4274
    TableTap_0 _S218 = table_tap_0(_S214, roughness_2, &kernelContext_26);

#line 4274
    thread TableTap_0 _S219 = _S218;

#line 4274
    float2 _S220 = dfg_at_0(&_S219, &kernelContext_26);

#line 4283
    float _S221 = _S220.x;

#line 4283
    float _S222 = _S220.y;

#line 4283
    float3 _S223 = f0_2 * float3(_S221)  + float3(_S222) ;

#line 4289
    float3 _S224 = float3(0.0f, 0.0f, 0.0f);

#line 4289
    uint slot_0 = 0U;

#line 4289
    float3 direct_0 = _S224;

#line 4289
    float3 gloss_0 = _S224;

    for(;;)
    {

#line 4291
        if(slot_0 < _S217)
        {
        }
        else
        {

#line 4291
            break;
        }

#line 4291
        thread GpuLight_natural_0 _S225 = (&kernelContext_26)->lights_0[(&kernelContext_26)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 4291
        uint _S226 = (&_S225)->kind_0;

#line 4300
        bool _S227 = ((&_S225)->kind_0) == 0U;

#line 4300
        float3 to_light_7;

#line 4300
        float reach_0;

#line 4300
        if(_S227)
        {

#line 4300
            to_light_7 = normalize((float4((&_S225)->direction_0) ).xyz);

#line 4300
            reach_0 = 1.0f;

#line 4300
        }
        else
        {


            if(_S226 == 3U)
            {

#line 4305
                float4 _S228 = float4((&_S225)->position_0) ;

#line 4313
                float3 offset_0 = _S228.xyz - _S202.world_position_10;
                float distance_3 = length(offset_0);

                float _S229 = range_window_0(distance_3, _S228.w);

#line 4316
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 4316
                reach_0 = _S229;

#line 4305
            }
            else
            {

#line 4305
                float4 _S230 = float4((&_S225)->position_0) ;

#line 4320
                float3 offset_1 = _S230.xyz - _S202.world_position_10;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_1 = punctual_falloff_0(distance_4, _S230.w);
                if(_S226 == 2U)
                {

#line 4324
                    float4 _S231 = float4((&_S225)->direction_0) ;

#line 4324
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S231.xyz, _S231.w, (&_S225)->cos_inner_0);

#line 4324
                }
                else
                {

#line 4324
                    reach_0 = reach_1;

#line 4324
                }

#line 4324
                to_light_7 = to_light_8;

#line 4305
            }

#line 4300
        }

#line 4333
        float n_dot_l_5 = dot(_S207, to_light_7);

#line 4333
        float3 specular_0;

#line 4333
        float diffuse_0;


        if(_S226 == 3U)
        {

#line 4346
            thread array<float3, int(4)> corners_2;

#line 4346
            rect_corners_0(&_S225, _S202.world_position_10, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S207, to_eye_1, _S214);

#line 4348
            thread array<float3, int(4)> _S232 = corners_2;

#line 4348
            float _S233 = ltc_irradiance_0(to_local_0, &_S232);

#line 4348
            thread TableTap_0 _S234 = _S218;

#line 4348
            float4 _S235 = ltc_at_0(&_S234, &kernelContext_26);

            matrix<float,int(3),int(3)>  _S236 = (((to_local_0) * (ltc_transform_0(_S235))));

#line 4350
            thread array<float3, int(4)> _S237 = corners_2;

#line 4350
            float _S238 = ltc_irradiance_0(_S236, &_S237);
            float3 _S239 = float3(_S238)  * _S223;

#line 4351
            diffuse_0 = _S233;

#line 4351
            specular_0 = _S239;

#line 4336
        }
        else
        {

#line 4356
            float _S240 = max(n_dot_l_5, 0.0f);

#line 4363
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 4371
            float3 specular_1 = ggx_lobe_0(_S212, f0_2, _S240, _S214, max(dot(_S207, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S240) ;

#line 4371
            diffuse_0 = _S240;

#line 4371
            specular_0 = specular_1;

#line 4336
        }

#line 4336
        float3 specular_2;

#line 4379
        if((((&_S225)->flags_3) & 1U) != 0U)
        {

#line 4379
            specular_2 = _S224;

#line 4379
        }
        else
        {

#line 4379
            specular_2 = specular_0;

#line 4379
        }

#line 4379
        float reach_2;

#line 4397
        if(_S227)
        {

#line 4397
            float _S241 = sun_visibility_0(_S202.world_position_10, to_light_7, n_dot_l_5, _S209, _S215, &kernelContext_26);

#line 4397
            reach_2 = _S241;

#line 4397
        }
        else
        {


            if(_S226 == 1U)
            {

#line 4402
                uint _S242 = (&_S225)->shadow_tile_0;

#line 4414
                if(((&_S225)->shadow_tile_0) <= 8U)
                {

#line 4414
                    float _S243 = point_visibility_0(&_S225, _S242, _S202.world_position_10, to_light_7, n_dot_l_5, _S209, _S215, &kernelContext_26);

#line 4414
                    reach_2 = reach_0 * _S243;

#line 4414
                }
                else
                {

#line 4414
                    reach_2 = reach_0;

#line 4414
                }

#line 4402
            }
            else
            {

#line 4402
                uint _S244 = (&_S225)->shadow_tile_0;

#line 4420
                if(((&_S225)->shadow_tile_0) < 14U)
                {

#line 4420
                    float _S245 = spot_visibility_0(&_S225, _S244, _S202.world_position_10, to_light_7, n_dot_l_5, _S209, _S215, &kernelContext_26);

#line 4420
                    reach_2 = reach_0 * _S245;

#line 4420
                }
                else
                {

#line 4420
                    reach_2 = reach_0;

#line 4420
                }

#line 4402
            }

#line 4397
        }

#line 4428
        float3 _S246 = (float4((&_S225)->color_0) ).xyz;

#line 4428
        float3 direct_1 = direct_0 + _S246 * float3((diffuse_0 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S246 * (specular_2 * float3(reach_2) );

#line 4291
        slot_0 = slot_0 + 1U;

#line 4291
        direct_0 = direct_1;

#line 4291
        gloss_0 = gloss_1;

#line 4291
    }

#line 4443
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S221 + _S222);

#line 4443
    float _S247 = occlusion_at_0(_S215, &kernelContext_26);

#line 4479
    float3 _S248 = frame_5->ambient_0.xyz;

#line 4479
    float3 _S249 = sky_irradiance_0(_S207, &kernelContext_26);

#line 4479
    float3 _S250 = _S248 + _S249;

#line 4479
    float3 _S251 = probe_irradiance_0(_S202.world_position_10, _S207, &kernelContext_26);

#line 4500
    float3 lit_1 = diffuse_albedo_0 * ((_S250 + _S251) * float3(_S247)  + direct_0) + gloss_2;

#line 4500
    float3 _S252 = emissive_of_0(&_S204);

#line 4536
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_26)->frame_0->fog_params_0.x, (&kernelContext_26)->frame_0->fog_params_0.y, (&kernelContext_26)->frame_0->camera_position_0.y - (&kernelContext_26)->frame_0->fog_params_0.z, _S202.world_position_10.y - (&kernelContext_26)->frame_0->fog_params_0.z, length((&kernelContext_26)->frame_0->camera_position_0.xyz - _S202.world_position_10)));


    thread FragmentOutput_0 output_2;



    (&output_2)->lit_0 = float4((lit_1 + _S252) * float3(fog_survives_0)  + (&kernelContext_26)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;
    return output_2;
}


#line 4549
struct vertexMain_Result_0
{
    float4 position_5 [[position]];
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


#line 4549
[[vertex]] vertexMain_Result_0 vertexMain(uint index_5 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]])
{

#line 4549
    thread KernelContext_0 kernelContext_27;

#line 4549
    (&kernelContext_27)->draw_0 = draw_4;

#line 4549
    (&kernelContext_27)->visible_instances_0 = visible_instances_4;

#line 4549
    (&kernelContext_27)->instances_0 = instances_4;

#line 4549
    (&kernelContext_27)->meshes_0 = meshes_4;

#line 4549
    (&kernelContext_27)->frame_0 = frame_7;

#line 4549
    (&kernelContext_27)->vertices_0 = vertices_4;

#line 4549
    (&kernelContext_27)->ambient_occlusion_0 = ambient_occlusion_4;

#line 4549
    (&kernelContext_27)->materials_0 = materials_4;

#line 4549
    (&kernelContext_27)->normal_textures_0 = normal_textures_4;

#line 4549
    (&kernelContext_27)->base_color_sampler_0 = base_color_sampler_4;

#line 4549
    (&kernelContext_27)->base_color_textures_0 = base_color_textures_4;

#line 4549
    (&kernelContext_27)->cluster_lights_0 = cluster_lights_4;

#line 4549
    (&kernelContext_27)->specular_dfg_0 = specular_dfg_4;

#line 4549
    (&kernelContext_27)->lights_0 = lights_4;

#line 4549
    (&kernelContext_27)->ltc_matrix_0 = ltc_matrix_4;

#line 4549
    (&kernelContext_27)->shadow_atlas_0 = shadow_atlas_4;

#line 4549
    (&kernelContext_27)->shadow_sampler_0 = shadow_sampler_4;

#line 4549
    (&kernelContext_27)->probes_0 = probes_4;

#line 4549
    GpuInstance_natural_0 device* _S253 = instances_4+visible_instances_4[draw_4->base_0 + instance_id_1];

#line 1636
    GpuMesh_0 mesh_3 = meshes_4[draw_4->mesh_0];

#line 1644
    bool _S254 = ((_S253->flags_0) & 2U) != 0U;

#line 1644
    uint base_vertex_3;
    if(_S254)
    {

#line 1645
        base_vertex_3 = _S253->base_vertex_0;

#line 1645
    }
    else
    {

#line 1645
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1645
    }

#line 1645
    MeshVertex_0 _S255 = load_vertex_0(index_5 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_27);

#line 1645
    uint previous_base_0;

#line 1658
    if(_S254)
    {

#line 1658
        previous_base_0 = _S253->previous_base_vertex_0;

#line 1658
    }
    else
    {

#line 1658
        previous_base_0 = base_vertex_3;

#line 1658
    }

#line 1658
    float3 _S256 = load_position_0(index_5 + previous_base_0, &kernelContext_27);

#line 1658
    matrix<float,int(4),int(4)>  _S257 = matrix<float,int(4),int(4)> (_S253->transform_0.data_0[int(0)][int(0)], _S253->transform_0.data_0[int(1)][int(0)], _S253->transform_0.data_0[int(2)][int(0)], _S253->transform_0.data_0[int(3)][int(0)], _S253->transform_0.data_0[int(0)][int(1)], _S253->transform_0.data_0[int(1)][int(1)], _S253->transform_0.data_0[int(2)][int(1)], _S253->transform_0.data_0[int(3)][int(1)], _S253->transform_0.data_0[int(0)][int(2)], _S253->transform_0.data_0[int(1)][int(2)], _S253->transform_0.data_0[int(2)][int(2)], _S253->transform_0.data_0[int(3)][int(2)], _S253->transform_0.data_0[int(0)][int(3)], _S253->transform_0.data_0[int(1)][int(3)], _S253->transform_0.data_0[int(2)][int(3)], _S253->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S255.position_1, 1.0f)) * (_S257)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_27)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_0.xyz;

#line 1672
    matrix<float,int(3),int(3)>  _S258 = matrix<float,int(3),int(3)> (_S257[int(0)].xyz, _S257[int(1)].xyz, _S257[int(2)].xyz);

#line 1672
    (&output_3)->world_normal_0 = (((_S255.basis_1.normal_0) * (normal_basis_0(_S258))));

#line 1678
    (&output_3)->world_tangent_0 = (((_S255.basis_1.tangent_1) * (_S258)));

#line 1678
    thread TangentFrame_0 _S259 = _S255.basis_1;

#line 1678
    uint _S260 = frame_word_0(mesh_3.flags_1, &_S259);
    (&output_3)->frame_3 = _S260;

#line 1679
    float4 _S261;

#line 1686
    if(((&kernelContext_27)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1686
        _S261 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1686
    }
    else
    {

#line 1686
        _S261 = _S255.color_1;

#line 1686
    }

#line 1685
    (&output_3)->color_2 = _S261;

#line 1692
    (&output_3)->material_2 = _S253->material_0;
    (&output_3)->uv_0 = _S255.uv0_0;

#line 1699
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S256, 1.0f)) * (matrix<float,int(4),int(4)> (_S253->previous_transform_0.data_0[int(0)][int(0)], _S253->previous_transform_0.data_0[int(1)][int(0)], _S253->previous_transform_0.data_0[int(2)][int(0)], _S253->previous_transform_0.data_0[int(3)][int(0)], _S253->previous_transform_0.data_0[int(0)][int(1)], _S253->previous_transform_0.data_0[int(1)][int(1)], _S253->previous_transform_0.data_0[int(2)][int(1)], _S253->previous_transform_0.data_0[int(3)][int(1)], _S253->previous_transform_0.data_0[int(0)][int(2)], _S253->previous_transform_0.data_0[int(1)][int(2)], _S253->previous_transform_0.data_0[int(2)][int(2)], _S253->previous_transform_0.data_0[int(3)][int(2)], _S253->previous_transform_0.data_0[int(0)][int(3)], _S253->previous_transform_0.data_0[int(1)][int(3)], _S253->previous_transform_0.data_0[int(2)][int(3)], _S253->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_27)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S262 = output_3;

#line 1703
    thread vertexMain_Result_0 _S263;

#line 1703
    (&_S263)->position_5 = _S262.position_3;

#line 1703
    (&_S263)->world_position_11 = _S262.world_position_1;

#line 1703
    (&_S263)->world_normal_2 = _S262.world_normal_0;

#line 1703
    (&_S263)->color_4 = _S262.color_2;

#line 1703
    (&_S263)->material_6 = _S262.material_2;

#line 1703
    (&_S263)->uv_4 = _S262.uv_0;

#line 1703
    (&_S263)->clip_position_2 = _S262.clip_position_0;

#line 1703
    (&_S263)->previous_clip_position_2 = _S262.previous_clip_position_0;

#line 1703
    (&_S263)->world_tangent_2 = _S262.world_tangent_0;

#line 1703
    (&_S263)->frame_6 = _S262.frame_3;

#line 1703
    return _S263;
}

