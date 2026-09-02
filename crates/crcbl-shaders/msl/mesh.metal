#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2445 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2440
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2712
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2772
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2925
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2787
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2815
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1105
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1749
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1749
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


#line 774
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


#line 1755
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1755
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
    texture2d_array<float, access::sample> probe_visibility_0;
};


#line 1148
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


#line 1159
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1162
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1613
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1736
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1736
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1738
        word_4 = 1U;

#line 1738
    }
    else
    {

#line 1738
        word_4 = 0U;

#line 1738
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1742
        word_4 = word_4 | 2U;

#line 1742
    }

#line 1741
    return word_4;
}


#line 1741
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1856
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_1 [[texture(6)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(7)]])
{

#line 1856
    thread KernelContext_0 kernelContext_2;

#line 1856
    (&kernelContext_2)->draw_0 = draw_1;

#line 1856
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1856
    (&kernelContext_2)->instances_0 = instances_1;

#line 1856
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1856
    (&kernelContext_2)->frame_0 = frame_1;

#line 1856
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1856
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1856
    (&kernelContext_2)->materials_0 = materials_1;

#line 1856
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1856
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1856
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1856
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1856
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1856
    (&kernelContext_2)->lights_0 = lights_1;

#line 1856
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1856
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1856
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1856
    (&kernelContext_2)->contact_shadow_0 = contact_shadow_1;

#line 1856
    (&kernelContext_2)->probes_0 = probes_1;

#line 1856
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 1856
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1859
    uint base_vertex_2;

#line 1865
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1865
        base_vertex_2 = _S7->base_vertex_0;

#line 1865
    }
    else
    {

#line 1865
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1865
    }

#line 1865
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1865
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1865
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 1868
struct vertexOutput_1
{
    float4 output_1 [[position]];
};


#line 1889
[[vertex]] vertexOutput_1 depthClearVertexMain(uint index_1 [[vertex_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_2 [[texture(6)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(7)]])
{

#line 1889
    thread KernelContext_0 kernelContext_3;

#line 1889
    (&kernelContext_3)->draw_0 = draw_2;

#line 1889
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 1889
    (&kernelContext_3)->instances_0 = instances_2;

#line 1889
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 1889
    (&kernelContext_3)->frame_0 = frame_2;

#line 1889
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 1889
    (&kernelContext_3)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1889
    (&kernelContext_3)->materials_0 = materials_2;

#line 1889
    (&kernelContext_3)->normal_textures_0 = normal_textures_2;

#line 1889
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 1889
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 1889
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 1889
    (&kernelContext_3)->specular_dfg_0 = specular_dfg_2;

#line 1889
    (&kernelContext_3)->lights_0 = lights_2;

#line 1889
    (&kernelContext_3)->ltc_matrix_0 = ltc_matrix_2;

#line 1889
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 1889
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 1889
    (&kernelContext_3)->contact_shadow_0 = contact_shadow_2;

#line 1889
    (&kernelContext_3)->probes_0 = probes_2;

#line 1889
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_2;

#line 1889
    vertexOutput_1 _S11 = { float4(float2(float((index_1 << 1U) & 2U), float(index_1 & 2U)) * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f) };


    return _S11;
}


#line 4482
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S12 = previous_0.w;

#line 4484
    if(_S12 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S12) ) * float2(0.5f, -0.5f);
}


#line 4358
float4 occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_4)
{

#line 4358
    texture2d<float, access::sample> _S13 = kernelContext_4->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S13).get_width(0)),(*((&height_0)) = (_S13).get_height(0));

    int3 _S14 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 4364
    return ((kernelContext_4->ambient_occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z)));
}


#line 4092
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S15 = axis_0.x;

#line 4096
    float _S16 = axis_0.y;

#line 4096
    bool _S17;

#line 4096
    if(_S15 >= _S16)
    {

#line 4096
        _S17 = _S15 >= (axis_0.z);

#line 4096
    }
    else
    {

#line 4096
        _S17 = false;

#line 4096
    }

#line 4096
    float2 planar_0;

#line 4096
    if(_S17)
    {

#line 4096
        planar_0 = world_position_0.zy;

#line 4096
    }
    else
    {

        if(_S16 >= (axis_0.z))
        {

#line 4100
            planar_0 = world_position_0.xz;

#line 4100
        }
        else
        {

#line 4100
            planar_0 = world_position_0.xy;

#line 4100
        }

#line 4096
    }

#line 4108
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 959
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 4129
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S18 = normal_2.z;

#line 4131
    float sign_z_0;

#line 4131
    if(_S18 >= 0.0f)
    {

#line 4131
        sign_z_0 = 1.0f;

#line 4131
    }
    else
    {

#line 4131
        sign_z_0 = -1.0f;

#line 4131
    }
    float a_0 = -1.0f / (sign_z_0 + _S18);
    float _S19 = normal_2.x;

#line 4133
    float _S20 = sign_z_0 * _S19;

#line 4133
    return float3(1.0f + _S20 * _S19 * a_0, _S20 * normal_2.y * a_0, - sign_z_0 * _S19);
}


#line 4183
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S21 = duvdy_0.y;

#line 4185
    float _S22 = duvdx_0.y;

#line 4185
    float winding_0;
    if((duvdx_0.x * _S21 - duvdy_0.x * _S22) < 0.0f)
    {

#line 4186
        winding_0 = -1.0f;

#line 4186
    }
    else
    {

#line 4186
        winding_0 = 1.0f;

#line 4186
    }
    float3 tangent_2 = (float3(_S21)  * dpdx_0 - float3(_S22)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 4195
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 4196
    float3 _S23;

#line 4205
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 4205
        _S23 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 4205
    }
    else
    {

#line 4205
        _S23 = orthonormal_tangent_0(normal_3);

#line 4205
    }

#line 4205
    (&basis_4)->tangent_1 = _S23;

    (&basis_4)->bitangent_0 = cross(normal_3, _S23);
    return basis_4;
}


#line 1620
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


#line 4265
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_5)
{

#line 4277
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 4287
    uint _S24 = input_0->frame_3;
    if(((input_0->frame_3) & 1U) != 0U)
    {

#line 4296
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 4298
        float3 _S25;

#line 4303
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 4303
            _S25 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 4303
        }
        else
        {

#line 4303
            _S25 = orthonormal_tangent_0(normal_4);

#line 4303
        }

#line 4303
        (&basis_5)->tangent_1 = _S25;

#line 4309
        float3 _S26 = cross((&basis_5)->normal_0, _S25);

#line 4309
        float _S27;
        if((_S24 & 2U) != 0U)
        {

#line 4310
            _S27 = -1.0f;

#line 4310
        }
        else
        {

#line 4310
            _S27 = 1.0f;

#line 4310
        }

#line 4309
        (&basis_5)->bitangent_0 = _S26 * float3(_S27) ;

#line 4288
    }
    else
    {

#line 4314
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 4288
    }

#line 4318
    float3 _S28 = float3(uv_1, float(layer_0));
    float3 _S29 = ((kernelContext_5->normal_textures_0).sample((kernelContext_5->base_color_sampler_0), ((_S28)).xy, uint(((_S28)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 4319
    thread float3 tangent_space_0 = _S29;
    tangent_space_0.xy = _S29.xy * float2(normal_scale_1) ;

#line 4325
    float3 _S30 = normalize(tangent_space_0);

#line 4325
    tangent_space_0 = _S30;
    return normalize(float3(_S30.x)  * (&basis_5)->tangent_1 + float3(_S30.y)  * (&basis_5)->bitangent_0 + float3(_S30.z)  * (&basis_5)->normal_0);
}


#line 2580
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2591
    float3 _S31;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2592
        _S31 = - facet_1;

#line 2592
    }
    else
    {

#line 2592
        _S31 = facet_1;

#line 2592
    }

#line 2592
    return _S31;
}


#line 944
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3689
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_6)
{
    uint _S32 = max(kernelContext_6->frame_0->cluster_grid_0.x, 1U);
    uint _S33 = max(kernelContext_6->frame_0->cluster_grid_0.y, 1U);
    uint _S34 = max(kernelContext_6->frame_0->cluster_grid_0.z, 1U);
    uint _S35 = max(kernelContext_6->frame_0->cluster_grid_0.w, 1U);

#line 3699
    uint _S36 = uint(pixel_0.x) / _S35;

#line 3699
    uint _S37 = min(_S36, _S32 - 1U);
    uint _S38 = uint(pixel_0.y) / _S35;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3710
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S34 - 1U))) * _S33 + min(_S38, _S33 - 1U)) * _S32 + _S37;
}


#line 2012
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 2033
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 2033
    texture2d<float, access::sample> _S39 = kernelContext_7->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S39).get_width(0)),(*((&height_1)) = (_S39).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 2039
    float2 _S40 = float2(1.0f) ;
    float2 _S41 = extent_1 - _S40;

#line 2040
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S41);
    float2 high_1 = min(low_1 + _S40, _S41);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 2058
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 2070
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_8)
{
    int _S42 = tap_1->lo_0.x;

#line 2072
    int _S43 = tap_1->lo_0.y;

#line 2072
    int3 _S44 = int3(_S42, _S43, int(0));
    int _S45 = tap_1->hi_0.x;

#line 2073
    int3 _S46 = int3(_S45, _S43, int(0));
    float2 _S47 = float2(tap_1->weight_0.x) ;
    int _S48 = tap_1->hi_0.y;

#line 2075
    int3 _S49 = int3(_S42, _S48, int(0));
    int3 _S50 = int3(_S45, _S48, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)))), _S47), mix(decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), decode_dfg_pair_0(((kernelContext_8->specular_dfg_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)))), _S47), float2(tap_1->weight_0.y) );
}


#line 3640
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3656
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3668
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3675
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2399
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2399
    float4 _S51 = float4(light_0->tangent_0) ;

    float3 _S52 = _S51.xyz;

#line 2401
    float3 across_0 = _S52 * float3(_S51.w) ;

#line 2401
    float4 _S53 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S52, _S53.xyz) * float3(_S53.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S54 = centre_0 - across_0;

#line 2404
    (*corners_0)[int(0)] = _S54 - down_0;
    float3 _S55 = centre_0 + across_0;

#line 2405
    (*corners_0)[int(1)] = _S55 - down_0;
    (*corners_0)[int(2)] = _S55 + down_0;
    (*corners_0)[int(3)] = _S54 + down_0;
    return;
}


#line 2157
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2160
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2161
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2161
    }
    else
    {

#line 2161
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2161
    }

#line 2161
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2162
        tangent_5 = across_1 / float3(span_0) ;

#line 2162
    }
    else
    {

#line 2162
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2162
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 2138
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2228
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2228
    float3 _S56 = polygon_0->corner_0[int(0)];

#line 2228
    float3 _S57 = polygon_0->corner_0[int(1)];

#line 2228
    float3 _S58 = polygon_0->corner_0[int(2)];

#line 2228
    float3 _S59 = polygon_0->corner_0[int(3)];

#line 2234
    float3 _S60 = float3(0.0f, 0.0f, 0.0f);


    float _S61 = polygon_0->corner_0[int(0)].z;

#line 2237
    int count_1;

#line 2237
    if(_S61 > 0.0f)
    {

#line 2237
        count_1 = int(1);

#line 2237
    }
    else
    {

#line 2237
        count_1 = int(0);

#line 2237
    }
    float _S62 = _S57.z;

#line 2238
    int _S63;

#line 2238
    if(_S62 > 0.0f)
    {

#line 2238
        _S63 = int(2);

#line 2238
    }
    else
    {

#line 2238
        _S63 = int(0);

#line 2238
    }

#line 2238
    int config_0 = count_1 + _S63;
    float _S64 = _S58.z;

#line 2239
    if(_S64 > 0.0f)
    {

#line 2239
        count_1 = int(4);

#line 2239
    }
    else
    {

#line 2239
        count_1 = int(0);

#line 2239
    }

#line 2239
    int config_1 = config_0 + count_1;
    float _S65 = _S59.z;

#line 2240
    if(_S65 > 0.0f)
    {

#line 2240
        count_1 = int(8);

#line 2240
    }
    else
    {

#line 2240
        count_1 = int(0);

#line 2240
    }

#line 2240
    int config_2 = config_1 + count_1;

#line 2240
    float3 l0_0;

#line 2240
    float3 l1_0;

#line 2240
    float3 l2_0;

#line 2240
    float3 l3_0;

#line 2240
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2243
        float3 _S66 = float3(_S61) ;


        float3 _S67 = float3(- _S62)  * _S56 + _S66 * _S57;
        float3 _S68 = float3(- _S65)  * _S56 + _S66 * _S59;

#line 2247
        count_1 = int(3);

#line 2247
        l0_0 = _S56;

#line 2247
        l1_0 = _S67;

#line 2247
        l2_0 = _S68;

#line 2247
        l3_0 = _S59;

#line 2247
        l4_0 = _S60;

#line 2243
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2249
            float3 _S69 = float3(_S62) ;


            float3 _S70 = float3(- _S61)  * _S57 + _S69 * _S56;
            float3 _S71 = float3(- _S64)  * _S57 + _S69 * _S58;

#line 2253
            count_1 = int(3);

#line 2253
            l0_0 = _S70;

#line 2253
            l1_0 = _S57;

#line 2253
            l2_0 = _S71;

#line 2253
            l3_0 = _S59;

#line 2253
            l4_0 = _S60;

#line 2249
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S72 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                float3 _S73 = float3(- _S65)  * _S56 + float3(_S61)  * _S59;

#line 2259
                count_1 = int(4);

#line 2259
                l0_0 = _S56;

#line 2259
                l1_0 = _S57;

#line 2259
                l2_0 = _S72;

#line 2259
                l3_0 = _S73;

#line 2259
                l4_0 = _S60;

#line 2255
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2261
                    float3 _S74 = float3(_S64) ;


                    float3 _S75 = float3(- _S65)  * _S58 + _S74 * _S59;
                    float3 _S76 = float3(- _S62)  * _S58 + _S74 * _S57;

#line 2265
                    count_1 = int(3);

#line 2265
                    l0_0 = _S75;

#line 2265
                    l1_0 = _S76;

#line 2265
                    l2_0 = _S58;

#line 2265
                    l3_0 = _S59;

#line 2265
                    l4_0 = _S60;

#line 2261
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S77 = float3(- _S61)  * _S57 + float3(_S62)  * _S56;
                        float3 _S78 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;

#line 2271
                        count_1 = int(4);

#line 2271
                        l0_0 = _S77;

#line 2271
                        l1_0 = _S57;

#line 2271
                        l2_0 = _S58;

#line 2271
                        l3_0 = _S78;

#line 2271
                        l4_0 = _S60;

#line 2267
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2273
                            float3 _S79 = float3(- _S65) ;


                            float3 _S80 = _S79 * _S56 + float3(_S61)  * _S59;
                            float3 _S81 = _S79 * _S58 + float3(_S64)  * _S59;

#line 2277
                            count_1 = int(5);

#line 2277
                            l0_0 = _S56;

#line 2277
                            l1_0 = _S57;

#line 2277
                            l2_0 = _S58;

#line 2277
                            l3_0 = _S81;

#line 2277
                            l4_0 = _S80;

#line 2273
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2279
                                float3 _S82 = float3(_S65) ;


                                float3 _S83 = float3(- _S61)  * _S59 + _S82 * _S56;
                                float3 _S84 = float3(- _S64)  * _S59 + _S82 * _S58;

#line 2283
                                count_1 = int(3);

#line 2283
                                l0_0 = _S83;

#line 2283
                                l1_0 = _S84;

#line 2283
                                l2_0 = _S59;

#line 2283
                                l3_0 = _S59;

#line 2283
                                l4_0 = _S60;

#line 2279
                            }
                            else
                            {

#line 2286
                                if(config_2 == int(9))
                                {

                                    float3 _S85 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;
                                    float3 _S86 = float3(- _S64)  * _S59 + float3(_S65)  * _S58;

#line 2290
                                    count_1 = int(4);

#line 2290
                                    l0_0 = _S56;

#line 2290
                                    l1_0 = _S85;

#line 2290
                                    l2_0 = _S86;

#line 2290
                                    l3_0 = _S59;

#line 2290
                                    l4_0 = _S60;

#line 2286
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S87 = float3(- _S65)  * _S58 + float3(_S64)  * _S59;
                                        float3 _S88 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;

#line 2297
                                        count_1 = int(5);

#line 2297
                                        l0_0 = _S56;

#line 2297
                                        l1_0 = _S57;

#line 2297
                                        l2_0 = _S88;

#line 2297
                                        l3_0 = _S87;

#line 2297
                                        l4_0 = _S59;

#line 2292
                                    }
                                    else
                                    {

#line 2299
                                        if(config_2 == int(12))
                                        {

                                            float3 _S89 = float3(- _S62)  * _S58 + float3(_S64)  * _S57;
                                            float3 _S90 = float3(- _S61)  * _S59 + float3(_S65)  * _S56;

#line 2303
                                            count_1 = int(4);

#line 2303
                                            l0_0 = _S90;

#line 2303
                                            l1_0 = _S89;

#line 2303
                                            l2_0 = _S58;

#line 2303
                                            l3_0 = _S59;

#line 2303
                                            l4_0 = _S60;

#line 2299
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S91 = float3(- _S64)  * _S57 + float3(_S62)  * _S58;
                                                float3 _S92 = float3(- _S62)  * _S56 + float3(_S61)  * _S57;

#line 2311
                                                count_1 = int(5);

#line 2311
                                                l0_0 = _S56;

#line 2311
                                                l1_0 = _S92;

#line 2311
                                                l2_0 = _S91;

#line 2311
                                                l3_0 = _S58;

#line 2311
                                                l4_0 = _S59;

#line 2305
                                            }
                                            else
                                            {

#line 2313
                                                if(config_2 == int(14))
                                                {

#line 2313
                                                    float3 _S93 = float3(- _S61) ;


                                                    float3 _S94 = _S93 * _S59 + float3(_S65)  * _S56;
                                                    float3 _S95 = _S93 * _S57 + float3(_S62)  * _S56;

#line 2317
                                                    count_1 = int(5);

#line 2317
                                                    l0_0 = _S95;

#line 2317
                                                    l1_0 = _S94;

#line 2313
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2319
                                                        count_1 = int(4);

#line 2319
                                                    }
                                                    else
                                                    {

#line 2319
                                                        count_1 = int(0);

#line 2319
                                                    }

#line 2319
                                                    l0_0 = _S56;

#line 2319
                                                    l1_0 = _S60;

#line 2313
                                                }

#line 2234
                                                float3 _S96 = l1_0;

#line 2234
                                                l1_0 = _S57;

#line 2234
                                                l2_0 = _S58;

#line 2234
                                                l3_0 = _S59;

#line 2234
                                                l4_0 = _S96;

#line 2305
                                            }

#line 2299
                                        }

#line 2292
                                    }

#line 2286
                                }

#line 2279
                            }

#line 2273
                        }

#line 2267
                    }

#line 2261
                }

#line 2255
            }

#line 2249
        }

#line 2243
    }

#line 2327
    if(count_1 <= int(3))
    {

#line 2327
        l3_0 = l0_0;

#line 2327
        l4_0 = l0_0;

#line 2327
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2332
            l4_0 = l0_0;

#line 2332
        }

#line 2327
    }

#line 2337
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2200
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2206
    float weight_1;

#line 2211
    if(cosine_0 > 0.0f)
    {

#line 2211
        weight_1 = fit_0;

#line 2211
    }
    else
    {

#line 2211
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2211
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2357
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2359
    int corner_1 = int(0);
    for(;;)
    {

#line 2360
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2360
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2360
        corner_1 = corner_1 + int(1);

#line 2360
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2365
    thread LtcPolygon_0 _S97 = polygon_1;

#line 2365
    LtcPolygon_0 _S98 = ltc_clip_0(&_S97);
    polygon_1 = _S98;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2369
    int at_2 = int(0);

    for(;;)
    {

#line 2371
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2371
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2371
        at_2 = at_2 + int(1);

#line 2371
    }

#line 2378
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2378
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2379
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2379
    }
    else
    {

#line 2379
        sum_1 = sum_0;

#line 2379
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2383
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2383
    }

#line 2390
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 2086
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_9)
{
    int _S99 = tap_2->lo_0.x;

#line 2088
    int _S100 = tap_2->lo_0.y;

#line 2088
    int3 _S101 = int3(_S99, _S100, int(0));
    int _S102 = tap_2->hi_0.x;

#line 2089
    int3 _S103 = int3(_S102, _S100, int(0));
    float4 _S104 = float4(tap_2->weight_0.x) ;
    int _S105 = tap_2->hi_0.y;

#line 2091
    int3 _S106 = int3(_S99, _S105, int(0));
    int3 _S107 = int3(_S102, _S105, int(0));

    return mix(mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S101)).xy), uint(((_S101)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S103)).xy), uint(((_S103)).z))), _S104), mix(((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), ((kernelContext_9->ltc_matrix_0).read(vec<uint,2>(((_S107)).xy), uint(((_S107)).z))), _S104), float4(tap_2->weight_0.y) );
}


#line 2173
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 1968
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 1975
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1982
    float _S108 = 1.0f - alpha2_0;

#line 1987
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S108 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S108 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2960
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 2960
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 3020
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 2992
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return rect_1.x / kernelContext_12->frame_0->shadow_params_0.x;
}


#line 2631
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 2947
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 2972
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 2972
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_14)
{
    return kernelContext_14->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 321
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 3142
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_15)
{
    float2 texel_1 = kernelContext_15->frame_0->shadow_params_0.xy;

#line 3144
    float4 _S109 = atlas_rect_0(cascade_0, kernelContext_15);

#line 3144
    float2 _S110 = atlas_step_0(_S109, kernelContext_15);


    float2 _S111 = float2(0.5f, 0.5f) * _S110;


    float2 _S112 = float2(1.0f, 1.0f);

#line 3150
    float2 _S113 = _S112 / texel_1;

#line 3150
    uint index_2 = 0U;

#line 3150
    float sum_2 = 0.0f;

#line 3150
    float found_0 = 0.0f;



    for(;;)
    {

#line 3154
        if(index_2 < 16U)
        {
        }
        else
        {

#line 3154
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_2] * float2(8.0f) ;
        float _S114 = spoke_0.x;

#line 3157
        float _S115 = rotation_0.x;

#line 3157
        float _S116 = spoke_0.y;

#line 3157
        float _S117 = rotation_0.y;

#line 3165
        int3 _S118 = int3(int2(min(atlas_uv_0(_S109, clamp(tile_uv_1 + float2(_S114 * _S115 - _S116 * _S117, _S114 * _S117 + _S116 * _S115) * _S110, _S111, float2(1.0f)  - _S111)) * _S113, _S113 - _S112)), int(0));

#line 3165
        float depth_1 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 3169
            sum_2 = sum_2 + depth_1;

#line 3169
            found_0 = found_1;

#line 3166
        }

#line 3154
        index_2 = index_2 + 1U;

#line 3154
    }

#line 3173
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3184
    float _S119 = 2.0f * kernelContext_15->frame_0->cascade_far_0[cascade_0];

#line 3184
    float separation_0 = (sum_2 / found_0 - reference_0) * (_S119 + 40.0f);

#line 3184
    float _S120 = tile_texels_0(_S109, kernelContext_15);

    return clamp(separation_0 * 0.01999999955296516f / (_S119 / _S120), 2.0f, 8.0f);
}


#line 3042
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_16)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S121 = spoke_1.x;

#line 3047
    float _S122 = rotation_1.x;

#line 3047
    float _S123 = spoke_1.y;

#line 3047
    float _S124 = rotation_1.y;


    float _S125 = ((kernelContext_16->shadow_atlas_0).sample_compare((kernelContext_16->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_2 + float2(_S121 * _S122 - _S123 * _S124, _S121 * _S124 + _S123 * _S122) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 3050
    return _S125;
}


#line 3072
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_2, KernelContext_0 thread* kernelContext_17)
{
    float2 _S126 = shadow_rotation_0(pixel_2);

#line 3074
    float4 _S127 = atlas_rect_1(tile_2, kernelContext_17);

    if(atlas_rect_is_empty_0(_S127))
    {
        return 1.0f;
    }

#line 3078
    float2 _S128 = atlas_step_1(_S127, kernelContext_17);

#line 3078
    uint spot_0 = 0U;

#line 3078
    float probe_0 = 0.0f;

#line 3083
    for(;;)
    {

#line 3083
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 3083
            break;
        }

#line 3083
        float _S129 = tile_tap_0(_S127, _S128, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S126, reference_2, kernelContext_17);

        float probe_1 = probe_0 + _S129;

#line 3083
        spot_0 = spot_0 + 1U;

#line 3083
        probe_0 = probe_1;

#line 3083
    }

#line 3092
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 3098
    uint index_3 = 0U;

#line 3098
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 3102
        if(index_3 < 32U)
        {
        }
        else
        {

#line 3102
            break;
        }

#line 3102
        float _S130 = tile_tap_0(_S127, _S128, tile_uv_3, SHADOW_DISC_0[index_3] * float2(radius_2) , _S126, reference_2, kernelContext_17);

        float visibility_1 = visibility_0 + _S130;

#line 3102
        index_3 = index_3 + 1U;

#line 3102
        visibility_0 = visibility_1;

#line 3102
    }

#line 3107
    return visibility_0 / 32.0f;
}


#line 3238
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_18)
{

#line 3239
    float4 _S131 = atlas_rect_0(cascade_1, kernelContext_18);

#line 3273
    if(atlas_rect_is_empty_0(_S131))
    {


        return 1.0f;
    }
    float _S132 = 2.0f * kernelContext_18->frame_0->cascade_far_0[cascade_1];

#line 3279
    float _S133 = tile_texels_0(_S131, kernelContext_18);

#line 3279
    float texel_world_0 = _S132 / _S133;

#line 3286
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_18->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_18->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_18->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3290
    bool _S134;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3291
        _S134 = true;

#line 3291
    }
    else
    {

#line 3291
        _S134 = (ndc_0.z) <= 0.0f;

#line 3291
    }

#line 3291
    if(_S134)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3318
    float _S135 = ndc_0.z;

#line 3318
    float _S136 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S135, shadow_rotation_0(pixel_3), kernelContext_18);

#line 3318
    float _S137 = tile_pcf_0(cascade_1, tile_uv_4, _S135, pixel_3, _S136, kernelContext_18);
    return _S137;
}


#line 3335
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_19)
{

#line 3336
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3348
    float eye_distance_0 = length(world_position_5 - kernelContext_19->frame_0->camera_position_0.xyz);

#line 3348
    uint index_4 = 0U;

    for(;;)
    {

#line 3350
        if(index_4 < 2U)
        {
        }
        else
        {

#line 3350
            cascade_2 = 1U;

#line 3350
            break;
        }
        if(eye_distance_0 < kernelContext_19->frame_0->cascade_far_0[index_4])
        {

#line 3352
            cascade_2 = index_4;


            break;
        }

#line 3350
        index_4 = index_4 + 1U;

#line 3350
    }

#line 3350
    float _S138 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_19);

#line 3361
    uint _S139 = cascade_2 + 1U;

#line 3361
    if(_S139 >= 2U)
    {



        return _S138;
    }

#line 3374
    float band_0 = kernelContext_19->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_19->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S138;
    }

#line 3378
    float _S140 = cascade_visibility_0(_S139, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_19);

#line 3389
    return mix(_S138, _S140, blend_0);
}


#line 4394
float contact_at_0(float2 position_4, KernelContext_0 thread* kernelContext_20)
{

#line 4394
    texture2d<float, access::sample> _S141 = kernelContext_20->contact_shadow_0;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (_S141).get_width(0)),(*((&height_2)) = (_S141).get_height(0));

    int3 _S142 = int3(min(int2(position_4), int2(int(width_2), int(height_2)) - int2(int(1)) ), int(0));

#line 4400
    return ((kernelContext_20->contact_shadow_0).read(vec<uint,2>(((_S142)).xy), uint(((_S142)).z)).x);
}


#line 3592
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S143 = axis_2.x;

#line 3595
    float _S144 = axis_2.y;

#line 3595
    bool _S145;

#line 3595
    if(_S143 >= _S144)
    {

#line 3595
        _S145 = _S143 >= (axis_2.z);

#line 3595
    }
    else
    {

#line 3595
        _S145 = false;

#line 3595
    }

#line 3595
    uint _S146;

#line 3595
    if(_S145)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3597
            _S146 = 0U;

#line 3597
        }
        else
        {

#line 3597
            _S146 = 1U;

#line 3597
        }

#line 3597
        return _S146;
    }
    if(_S144 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3601
            _S146 = 2U;

#line 3601
        }
        else
        {

#line 3601
            _S146 = 3U;

#line 3601
        }

#line 3601
        return _S146;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3603
        _S146 = 4U;

#line 3603
    }
    else
    {

#line 3603
        _S146 = 5U;

#line 3603
    }

#line 3603
    return _S146;
}


#line 308
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 3496
float punctual_visibility_0(uint tile_4, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_21)
{

    uint atlas_0 = light_tile_0(tile_4);

#line 3499
    float4 _S147 = atlas_rect_0(atlas_0, kernelContext_21);

    if(atlas_rect_is_empty_0(_S147))
    {


        return 1.0f;
    }

#line 3505
    float _S148 = tile_texels_0(_S147, kernelContext_21);

    float texel_world_1 = map_world_0 / _S148;

#line 3517
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_21->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 3524
    float _S149 = clip_1.w;

#line 3524
    if(_S149 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S149) ;

#line 3528
    bool _S150;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3529
        _S150 = true;

#line 3529
    }
    else
    {

#line 3529
        _S150 = (ndc_1.z) <= 0.0f;

#line 3529
    }

#line 3529
    if(_S150)
    {

#line 3529
        _S150 = true;

#line 3529
    }
    else
    {

#line 3529
        _S150 = (ndc_1.z) > 1.0f;

#line 3529
    }

#line 3529
    if(_S150)
    {

#line 3536
        return 1.0f;
    }

#line 3536
    float _S151 = tile_pcf_0(atlas_0, float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_21);

#line 3546
    return _S151;
}


#line 3611
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_22)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3619
    float _S152 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_6, kernelContext_22);

#line 3625
    return _S152;
}


#line 3553
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_5, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_23)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3560
    float4 _S153 = float4(light_2->direction_0) ;

#line 3567
    float cos_outer_1 = _S153.w;

#line 3567
    float _S154 = punctual_visibility_0(tile_5, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S153.xyz)), 0.0f), geometric_normal_5, pixel_7, kernelContext_23);

#line 3574
    return _S154;
}


#line 2114
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 4381
float3 bent_normal_at_0(float4 occlusion_0, float3 shading_normal_1)
{
    float3 decoded_0 = occlusion_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 4383
    float3 _S155;
    if((length(decoded_0)) < 0.5f)
    {

#line 4384
        _S155 = shading_normal_1;

#line 4384
    }
    else
    {

#line 4384
        _S155 = normalize(decoded_0);

#line 4384
    }

#line 4384
    return _S155;
}


#line 4019
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_24)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_24->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_24->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_24->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 3848
uint probe_row_0(uint3 cell_1, KernelContext_0 thread* kernelContext_25)
{

    return min((cell_1.z * kernelContext_25->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_25->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_25->frame_0->probe_counts_0.w, 1U) - 1U);
}


#line 3744
float sign_not_zero_0(float value_0)
{

#line 3744
    float _S156;

    if(value_0 >= 0.0f)
    {

#line 3746
        _S156 = 1.0f;

#line 3746
    }
    else
    {

#line 3746
        _S156 = -1.0f;

#line 3746
    }

#line 3746
    return _S156;
}


#line 3763
float2 oct_encode_0(float3 direction_1)
{
    float _S157 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S157) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 3766
    float2 p_1;
    if(_S157 < 0.0f)
    {
        float _S158 = p_0.y;

#line 3769
        float _S159 = p_0.x;

#line 3769
        p_1 = float2((1.0f - abs(_S158)) * sign_not_zero_0(_S159), (1.0f - abs(_S159)) * sign_not_zero_0(_S158));

#line 3767
    }
    else
    {

#line 3767
        p_1 = p_0;

#line 3767
    }

#line 3772
    return p_1;
}


#line 3785
float2 probe_moments_0(uint index_5, float3 direction_2, KernelContext_0 thread* kernelContext_26)
{

#line 3785
    texture2d_array<float, access::sample> _S160 = kernelContext_26->probe_visibility_0;

    thread uint width_3;
    thread uint height_3;
    thread uint layers_0;
    (*((&width_3)) = (_S160).get_width(0)),(*((&height_3)) = (_S160).get_height(0)),(*((&layers_0)) = (_S160).get_array_size());

#line 3790
    float2 _S161 = float2(0.5f) ;

#line 3790
    float2 _S162 = float2(1.0f) ;


    float2 scaled_1 = (oct_encode_0(direction_2) * _S161 + _S161) * float2(16.0f)  + _S162 - _S161;
    float2 _S163 = float2(float(width_3), float(height_3)) - _S162;

#line 3794
    float2 low_2 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S163);
    float2 high_2 = min(low_2 + _S162, _S163);
    float2 weight_2 = clamp(scaled_1 - low_2, float2(0.0f) , float2(1.0f) );
    int layer_1 = int(min(index_5, max(layers_0, 1U) - 1U));

    int _S164 = int(low_2.x);

#line 3799
    int _S165 = int(low_2.y);

#line 3799
    int4 _S166 = int4(_S164, _S165, layer_1, int(0));
    int _S167 = int(high_2.x);

#line 3800
    int4 _S168 = int4(_S167, _S165, layer_1, int(0));
    int _S169 = int(high_2.y);

#line 3801
    int4 _S170 = int4(_S164, _S169, layer_1, int(0));
    int4 _S171 = int4(_S167, _S169, layer_1, int(0));
    float2 _S172 = float2(weight_2.x) ;

#line 3803
    return mix(mix(((kernelContext_26->probe_visibility_0).read(vec<uint,2>(((_S166)).xy), uint(((_S166)).z), uint(((_S166)).w))).xy, ((kernelContext_26->probe_visibility_0).read(vec<uint,2>(((_S168)).xy), uint(((_S168)).z), uint(((_S168)).w))).xy, _S172), mix(((kernelContext_26->probe_visibility_0).read(vec<uint,2>(((_S170)).xy), uint(((_S170)).z), uint(((_S170)).w))).xy, ((kernelContext_26->probe_visibility_0).read(vec<uint,2>(((_S171)).xy), uint(((_S171)).z), uint(((_S171)).w))).xy, _S172), float2(weight_2.y) );
}


#line 3821
float probe_weight_0(uint index_6, float3 probe_position_0, float3 world_position_9, float3 normal_7, KernelContext_0 thread* kernelContext_27)
{
    float3 to_probe_0 = probe_position_0 - (world_position_9 + normal_7 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 3824
    float2 _S173 = probe_moments_0(index_6, - to_probe_0, kernelContext_27);

#line 3830
    float _S174 = _S173.x;

#line 3830
    float _S175 = max(_S173.y - _S174 * _S174, 0.0f);
    float behind_0 = to_surface_0 - _S174;
    float bound_0 = _S175 / (_S175 + behind_0 * behind_0);

#line 3832
    float visible_0;
    if(to_surface_0 <= _S174)
    {

#line 3833
        visible_0 = 1.0f;

#line 3833
    }
    else
    {

#line 3833
        visible_0 = bound_0 * bound_0 * bound_0;

#line 3833
    }
    return max(visible_0, 0.00009999999747379f);
}


#line 996
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 3861
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_3;
};


#line 3888
WeightedProbe_0 probe_corner_0(uint3 cell_2, float3 spacing_0, float3 world_position_10, float3 normal_8, KernelContext_0 thread* kernelContext_28)
{

#line 3888
    uint _S176 = probe_row_0(cell_2, kernelContext_28);


    GpuProbe_natural_0 stored_0 = kernelContext_28->probes_0[_S176];

#line 3891
    float _S177 = probe_weight_0(_S176, kernelContext_28->frame_0->probe_origin_0.xyz + float3(cell_2) * spacing_0, world_position_10, normal_8, kernelContext_28);



    thread WeightedProbe_0 corner_2;

#line 3895
    float4 _S178 = float4(_S177) ;
    (&(&corner_2)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S178;
    (&(&corner_2)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S178;
    (&(&corner_2)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S178;
    (&corner_2)->weight_3 = _S177;
    return corner_2;
}


#line 3872
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_1, const WeightedProbe_0 thread* b_0, float t_1)
{
    thread WeightedProbe_0 blended_0;
    float4 _S179 = float4(t_1) ;

#line 3875
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_1->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S179);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_1->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S179);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_1->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S179);
    (&blended_0)->weight_3 = mix(a_1->weight_3, b_0->weight_3, t_1);
    return blended_0;
}


#line 3958
float3 probe_irradiance_0(float3 world_position_11, float3 normal_9, KernelContext_0 thread* kernelContext_29)
{

#line 3958
    float3 _S180 = float3(1.0f) ;

#line 3963
    float3 _S181 = float3(0.0f, 0.0f, 0.0f);

#line 3963
    float3 last_0 = max(float3(kernelContext_29->frame_0->probe_counts_0.xyz) - _S180, _S181);
    float3 grid_0 = clamp((world_position_11 - kernelContext_29->frame_0->probe_origin_0.xyz) * kernelContext_29->frame_0->probe_inv_spacing_0.xyz, _S181, last_0);

    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S182 = uint3(base_2);



    uint3 _S183 = uint3(min(base_2 + _S180, last_0));

#line 3979
    float3 inv_0 = kernelContext_29->frame_0->probe_inv_spacing_0.xyz;
    float _S184 = inv_0.x;

#line 3980
    float _S185;

#line 3980
    if(_S184 != 0.0f)
    {

#line 3980
        _S185 = 1.0f / _S184;

#line 3980
    }
    else
    {

#line 3980
        _S185 = 0.0f;

#line 3980
    }
    float _S186 = inv_0.y;

#line 3981
    float _S187;

#line 3981
    if(_S186 != 0.0f)
    {

#line 3981
        _S187 = 1.0f / _S186;

#line 3981
    }
    else
    {

#line 3981
        _S187 = 0.0f;

#line 3981
    }
    float _S188 = inv_0.z;

#line 3982
    float _S189;

#line 3982
    if(_S188 != 0.0f)
    {

#line 3982
        _S189 = 1.0f / _S188;

#line 3982
    }
    else
    {

#line 3982
        _S189 = 0.0f;

#line 3982
    }

#line 3980
    float3 spacing_1 = float3(_S185, _S187, _S189);

#line 3989
    uint _S190 = _S182.x;

#line 3989
    uint _S191 = _S182.y;

#line 3989
    uint _S192 = _S182.z;

#line 3989
    WeightedProbe_0 _S193 = probe_corner_0(uint3(_S190, _S191, _S192), spacing_1, world_position_11, normal_9, kernelContext_29);
    uint _S194 = _S183.x;

#line 3990
    WeightedProbe_0 _S195 = probe_corner_0(uint3(_S194, _S191, _S192), spacing_1, world_position_11, normal_9, kernelContext_29);

#line 3990
    float _S196 = f_0.x;

#line 3990
    thread WeightedProbe_0 _S197 = _S193;

#line 3990
    thread WeightedProbe_0 _S198 = _S195;

#line 3990
    WeightedProbe_0 _S199 = lerp_probe_0(&_S197, &_S198, _S196);
    uint _S200 = _S183.y;

#line 3991
    WeightedProbe_0 _S201 = probe_corner_0(uint3(_S190, _S200, _S192), spacing_1, world_position_11, normal_9, kernelContext_29);

#line 3991
    WeightedProbe_0 _S202 = probe_corner_0(uint3(_S194, _S200, _S192), spacing_1, world_position_11, normal_9, kernelContext_29);

#line 3991
    thread WeightedProbe_0 _S203 = _S201;

#line 3991
    thread WeightedProbe_0 _S204 = _S202;

#line 3991
    WeightedProbe_0 _S205 = lerp_probe_0(&_S203, &_S204, _S196);

    uint _S206 = _S183.z;

#line 3993
    WeightedProbe_0 _S207 = probe_corner_0(uint3(_S190, _S191, _S206), spacing_1, world_position_11, normal_9, kernelContext_29);

#line 3993
    WeightedProbe_0 _S208 = probe_corner_0(uint3(_S194, _S191, _S206), spacing_1, world_position_11, normal_9, kernelContext_29);

#line 3993
    thread WeightedProbe_0 _S209 = _S207;

#line 3993
    thread WeightedProbe_0 _S210 = _S208;

#line 3993
    WeightedProbe_0 _S211 = lerp_probe_0(&_S209, &_S210, _S196);

#line 3993
    WeightedProbe_0 _S212 = probe_corner_0(uint3(_S190, _S200, _S206), spacing_1, world_position_11, normal_9, kernelContext_29);

#line 3993
    WeightedProbe_0 _S213 = probe_corner_0(uint3(_S194, _S200, _S206), spacing_1, world_position_11, normal_9, kernelContext_29);

#line 3993
    thread WeightedProbe_0 _S214 = _S212;

#line 3993
    thread WeightedProbe_0 _S215 = _S213;

#line 3993
    WeightedProbe_0 _S216 = lerp_probe_0(&_S214, &_S215, _S196);



    float _S217 = f_0.y;

#line 3997
    thread WeightedProbe_0 _S218 = _S199;

#line 3997
    thread WeightedProbe_0 _S219 = _S205;

#line 3997
    WeightedProbe_0 _S220 = lerp_probe_0(&_S218, &_S219, _S217);

#line 3997
    thread WeightedProbe_0 _S221 = _S211;

#line 3997
    thread WeightedProbe_0 _S222 = _S216;

#line 3997
    WeightedProbe_0 _S223 = lerp_probe_0(&_S221, &_S222, _S217);

    float _S224 = f_0.z;

#line 3999
    thread WeightedProbe_0 _S225 = _S220;

#line 3999
    thread WeightedProbe_0 _S226 = _S223;

#line 3999
    WeightedProbe_0 _S227 = lerp_probe_0(&_S225, &_S226, _S224);

    float4 basis_7 = float4(normal_9, 1.0f);
    return max(float3(dot(_S227.sh_0.sh_r_0, basis_7), dot(_S227.sh_0.sh_g_0, basis_7), dot(_S227.sh_0.sh_b_0, basis_7)) / float3(_S227.weight_3) , _S181);
}


#line 4450
float3 multi_bounce_occlusion_0(float visibility_2, float3 albedo_0)
{

#line 4450
    float3 _S228 = float3(visibility_2) ;

#line 4456
    return min(float3(1.0f) , max(_S228, ((_S228 * (float3(2.04040002822875977f)  * albedo_0 - float3(0.33239999413490295f) ) + (float3(-4.79510021209716797f)  * albedo_0 + float3(0.64170002937316895f) )) * _S228 + (float3(2.75519990921020508f)  * albedo_0 + float3(0.69029998779296875f) )) * _S228));
}


#line 969
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2465
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S229 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2473
    float kernel_0 = 0.0001984127011383f;

#line 2473
    int term_0 = int(6);

    for(;;)
    {

#line 2475
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2475
            break;
        }
        float _S230 = kernel_0 * _S229 + FOG_KERNEL_0[term_0];

#line 2475
        int term_1 = term_0 - int(1);

#line 2475
        kernel_0 = _S230;

#line 2475
        term_0 = term_1;

#line 2475
    }

#line 2482
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2492
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S231 = - d_0;

#line 2496
        float series_0 = 0.00833333376795053f;

#line 2496
        int term_2 = int(3);

        for(;;)
        {

#line 2498
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2498
                break;
            }
            float _S232 = series_0 * _S231 + FOG_RATIO_KERNEL_0[term_2];

#line 2498
            int term_3 = term_2 - int(1);

#line 2498
            series_0 = _S232;

#line 2498
            term_2 = term_3;

#line 2498
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2526
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2537
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2545
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 4045
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 4045
struct pixelInput_0
{
    float3 world_position_12 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    [[flat]] uint material_5 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
    float3 world_tangent_1 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_4 [[user(TEXCOORD_5)]];
};


#line 4492
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S233 [[stage_in]], float4 position_5 [[position]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_3 [[texture(6)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_3 [[texture(7)]])
{

#line 4492
    thread KernelContext_0 kernelContext_30;

#line 4492
    (&kernelContext_30)->draw_0 = draw_3;

#line 4492
    (&kernelContext_30)->visible_instances_0 = visible_instances_3;

#line 4492
    (&kernelContext_30)->instances_0 = instances_3;

#line 4492
    (&kernelContext_30)->meshes_0 = meshes_3;

#line 4492
    (&kernelContext_30)->frame_0 = frame_5;

#line 4492
    (&kernelContext_30)->vertices_0 = vertices_3;

#line 4492
    (&kernelContext_30)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4492
    (&kernelContext_30)->materials_0 = materials_3;

#line 4492
    (&kernelContext_30)->normal_textures_0 = normal_textures_3;

#line 4492
    (&kernelContext_30)->base_color_sampler_0 = base_color_sampler_3;

#line 4492
    (&kernelContext_30)->base_color_textures_0 = base_color_textures_3;

#line 4492
    (&kernelContext_30)->cluster_lights_0 = cluster_lights_3;

#line 4492
    (&kernelContext_30)->specular_dfg_0 = specular_dfg_3;

#line 4492
    (&kernelContext_30)->lights_0 = lights_3;

#line 4492
    (&kernelContext_30)->ltc_matrix_0 = ltc_matrix_3;

#line 4492
    (&kernelContext_30)->shadow_atlas_0 = shadow_atlas_3;

#line 4492
    (&kernelContext_30)->shadow_sampler_0 = shadow_sampler_3;

#line 4492
    (&kernelContext_30)->contact_shadow_0 = contact_shadow_3;

#line 4492
    (&kernelContext_30)->probes_0 = probes_3;

#line 4492
    (&kernelContext_30)->probe_visibility_0 = probe_visibility_3;

#line 4504
    float3 vertex_normal_0 = normalize(_S233.world_normal_1);

#line 4509
    float2 motion_1 = motion_vector_0(_S233.clip_position_1, _S233.previous_clip_position_1);

#line 4525
    if((frame_5->ambient_0.w) >= 5.5f)
    {
        thread FragmentOutput_0 bent_0;

#line 4527
        float4 _S234 = occlusion_at_0(position_5.xy, &kernelContext_30);



        (&bent_0)->lit_0 = float4(_S234.yzw, 1.0f);


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

#line 4581
    if((frame_5->ambient_0.w) >= 3.5f)
    {

#line 4581
        float4 _S235 = occlusion_at_0(position_5.xy, &kernelContext_30);


        float value_1 = _S235.x;

#line 4583
        thread FragmentOutput_0 occlusion_1;

#line 4592
        (&occlusion_1)->lit_0 = float4(value_1, value_1, value_1, 1.0f);


        (&occlusion_1)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_1)->motion_0 = motion_1;
        return occlusion_1;
    }

    if((frame_5->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S233.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 4609
    thread GpuMaterial_natural_0 _S236 = (&kernelContext_30)->materials_0[_S233.material_5];

#line 4609
    float2 uv_3;

#line 4634
    if(((&_S236)->tiling_0) == 1U)
    {

#line 4634
        uv_3 = physical_tile_uv_0(_S233.world_position_12, vertex_normal_0, (&_S236)->tile_metres_0);

#line 4634
    }
    else
    {

#line 4634
        uv_3 = _S233.uv_2;

#line 4634
    }

#line 4634
    uint _S237 = normal_layer_0(&_S236);

#line 4634
    thread VertexOutput_0 _S238;

#line 4634
    (&_S238)->position_3 = position_5;

#line 4634
    (&_S238)->world_position_1 = _S233.world_position_12;

#line 4634
    (&_S238)->world_normal_0 = _S233.world_normal_1;

#line 4634
    (&_S238)->color_2 = _S233.color_3;

#line 4634
    (&_S238)->material_2 = _S233.material_5;

#line 4634
    (&_S238)->uv_0 = _S233.uv_2;

#line 4634
    (&_S238)->clip_position_0 = _S233.clip_position_1;

#line 4634
    (&_S238)->previous_clip_position_0 = _S233.previous_clip_position_1;

#line 4634
    (&_S238)->world_tangent_0 = _S233.world_tangent_1;

#line 4634
    (&_S238)->frame_3 = _S233.frame_4;

#line 4634
    float3 _S239 = shading_normal_of_0(_S237, (&_S236)->normal_scale_0, &_S238, vertex_normal_0, uv_3, &kernelContext_30);

#line 4641
    if((frame_5->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 4643
        float3 _S240 = float3(0.5f) ;

#line 4655
        (&normals_0)->lit_0 = float4(_S239 * _S240 + _S240, 1.0f);

#line 4661
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_30)->frame_0->camera_position_0.xyz - _S233.world_position_12);



    float3 _S241 = geometric_normal_of_0(_S233.world_position_12, vertex_normal_0);

#line 4670
    uint _S242 = base_color_layer_0(&_S236);

#line 4685
    float3 _S243 = float3(uv_3, float(_S242));
    float4 albedo_1 = _S233.color_3 * float4((&_S236)->base_color_0)  * (((&kernelContext_30)->base_color_textures_0).sample(((&kernelContext_30)->base_color_sampler_0), ((_S243)).xy, uint(((_S243)).z)));

#line 4692
    float metallic_1 = saturate((&_S236)->metallic_0);
    float roughness_2 = clamp((&_S236)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S244 = alpha_0 * alpha_0;

#line 4701
    float3 _S245 = albedo_1.xyz;

#line 4701
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S245, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S245 * float3((1.0f - metallic_1)) ;

#line 4708
    float _S246 = max(dot(_S239, to_eye_1), 0.00009999999747379f);

#line 4718
    float2 _S247 = position_5.xy;

#line 4718
    uint _S248 = froxel_of_0(_S247, (((float4(_S233.world_position_12, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_30)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_30)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_30);

#line 4718
    uint base_3 = _S248 * 17U;

#line 4723
    uint _S249 = min((&kernelContext_30)->cluster_lights_0[base_3], 16U);

#line 4723
    TableTap_0 _S250 = table_tap_0(_S246, roughness_2, &kernelContext_30);

#line 4723
    thread TableTap_0 _S251 = _S250;

#line 4723
    float2 _S252 = dfg_at_0(&_S251, &kernelContext_30);

#line 4732
    float _S253 = _S252.x;

#line 4732
    float _S254 = _S252.y;

#line 4732
    float3 _S255 = f0_2 * float3(_S253)  + float3(_S254) ;

#line 4738
    float3 _S256 = float3(0.0f, 0.0f, 0.0f);

#line 4738
    uint slot_0 = 0U;

#line 4738
    float3 direct_0 = _S256;

#line 4738
    float3 gloss_0 = _S256;

    for(;;)
    {

#line 4740
        if(slot_0 < _S249)
        {
        }
        else
        {

#line 4740
            break;
        }

#line 4740
        thread GpuLight_natural_0 _S257 = (&kernelContext_30)->lights_0[(&kernelContext_30)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 4740
        uint _S258 = (&_S257)->kind_0;

#line 4749
        bool _S259 = ((&_S257)->kind_0) == 0U;

#line 4749
        float3 to_light_7;

#line 4749
        float reach_0;

#line 4749
        if(_S259)
        {

#line 4749
            to_light_7 = normalize((float4((&_S257)->direction_0) ).xyz);

#line 4749
            reach_0 = 1.0f;

#line 4749
        }
        else
        {


            if(_S258 == 3U)
            {

#line 4754
                float4 _S260 = float4((&_S257)->position_0) ;

#line 4762
                float3 offset_0 = _S260.xyz - _S233.world_position_12;
                float distance_3 = length(offset_0);

                float _S261 = range_window_0(distance_3, _S260.w);

#line 4765
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 4765
                reach_0 = _S261;

#line 4754
            }
            else
            {

#line 4754
                float4 _S262 = float4((&_S257)->position_0) ;

#line 4769
                float3 offset_1 = _S262.xyz - _S233.world_position_12;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_1 = punctual_falloff_0(distance_4, _S262.w);
                if(_S258 == 2U)
                {

#line 4773
                    float4 _S263 = float4((&_S257)->direction_0) ;

#line 4773
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S263.xyz, _S263.w, (&_S257)->cos_inner_0);

#line 4773
                }
                else
                {

#line 4773
                    reach_0 = reach_1;

#line 4773
                }

#line 4773
                to_light_7 = to_light_8;

#line 4754
            }

#line 4749
        }

#line 4782
        float n_dot_l_5 = dot(_S239, to_light_7);

#line 4782
        float3 specular_0;

#line 4782
        float diffuse_0;


        if(_S258 == 3U)
        {

#line 4795
            thread array<float3, int(4)> corners_2;

#line 4795
            rect_corners_0(&_S257, _S233.world_position_12, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S239, to_eye_1, _S246);

#line 4797
            thread array<float3, int(4)> _S264 = corners_2;

#line 4797
            float _S265 = ltc_irradiance_0(to_local_0, &_S264);

#line 4797
            thread TableTap_0 _S266 = _S250;

#line 4797
            float4 _S267 = ltc_at_0(&_S266, &kernelContext_30);

            matrix<float,int(3),int(3)>  _S268 = (((to_local_0) * (ltc_transform_0(_S267))));

#line 4799
            thread array<float3, int(4)> _S269 = corners_2;

#line 4799
            float _S270 = ltc_irradiance_0(_S268, &_S269);
            float3 _S271 = float3(_S270)  * _S255;

#line 4800
            diffuse_0 = _S265;

#line 4800
            specular_0 = _S271;

#line 4785
        }
        else
        {

#line 4805
            float _S272 = max(n_dot_l_5, 0.0f);

#line 4812
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 4820
            float3 specular_1 = ggx_lobe_0(_S244, f0_2, _S272, _S246, max(dot(_S239, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S272) ;

#line 4820
            diffuse_0 = _S272;

#line 4820
            specular_0 = specular_1;

#line 4785
        }

#line 4785
        float3 specular_2;

#line 4828
        if((((&_S257)->flags_3) & 1U) != 0U)
        {

#line 4828
            specular_2 = _S256;

#line 4828
        }
        else
        {

#line 4828
            specular_2 = specular_0;

#line 4828
        }

#line 4828
        float reach_2;

#line 4846
        if(_S259)
        {

#line 4846
            float _S273 = sun_visibility_0(_S233.world_position_12, to_light_7, n_dot_l_5, _S241, _S247, &kernelContext_30);

#line 4846
            float _S274 = contact_at_0(_S247, &kernelContext_30);

#line 4846
            reach_2 = _S273 * _S274;

#line 4846
        }
        else
        {

#line 4858
            if(_S258 == 1U)
            {

#line 4858
                uint _S275 = (&_S257)->shadow_tile_0;

#line 4870
                if(((&_S257)->shadow_tile_0) <= 8U)
                {

#line 4870
                    float _S276 = point_visibility_0(&_S257, _S275, _S233.world_position_12, to_light_7, n_dot_l_5, _S241, _S247, &kernelContext_30);

#line 4870
                    reach_2 = reach_0 * _S276;

#line 4870
                }
                else
                {

#line 4870
                    reach_2 = reach_0;

#line 4870
                }

#line 4858
            }
            else
            {

#line 4858
                uint _S277 = (&_S257)->shadow_tile_0;

#line 4876
                if(((&_S257)->shadow_tile_0) < 14U)
                {

#line 4876
                    float _S278 = spot_visibility_0(&_S257, _S277, _S233.world_position_12, to_light_7, n_dot_l_5, _S241, _S247, &kernelContext_30);

#line 4876
                    reach_2 = reach_0 * _S278;

#line 4876
                }
                else
                {

#line 4876
                    reach_2 = reach_0;

#line 4876
                }

#line 4858
            }

#line 4846
        }

#line 4884
        float3 _S279 = (float4((&_S257)->color_0) ).xyz;

#line 4884
        float3 direct_1 = direct_0 + _S279 * float3((diffuse_0 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S279 * (specular_2 * float3(reach_2) );

#line 4740
        slot_0 = slot_0 + 1U;

#line 4740
        direct_0 = direct_1;

#line 4740
        gloss_0 = gloss_1;

#line 4740
    }

#line 4899
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S253 + _S254);

#line 4899
    float4 _S280 = occlusion_at_0(_S247, &kernelContext_30);

#line 4918
    float occluded_0 = _S280.x;

#line 4927
    float3 bent_normal_0 = bent_normal_at_0(_S280, _S239);

#line 4950
    float3 _S281 = frame_5->ambient_0.xyz;

#line 4950
    float3 _S282 = sky_irradiance_0(bent_normal_0, &kernelContext_30);

#line 4950
    float3 _S283 = _S281 + _S282;

#line 4950
    float3 _S284 = probe_irradiance_0(_S233.world_position_12, bent_normal_0, &kernelContext_30);

#line 4986
    float3 lit_1 = diffuse_albedo_0 * ((_S283 + _S284) * multi_bounce_occlusion_0(occluded_0, diffuse_albedo_0) + direct_0) + gloss_2;

#line 4986
    float3 _S285 = emissive_of_0(&_S236);

#line 5022
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_30)->frame_0->fog_params_0.x, (&kernelContext_30)->frame_0->fog_params_0.y, (&kernelContext_30)->frame_0->camera_position_0.y - (&kernelContext_30)->frame_0->fog_params_0.z, _S233.world_position_12.y - (&kernelContext_30)->frame_0->fog_params_0.z, length((&kernelContext_30)->frame_0->camera_position_0.xyz - _S233.world_position_12)));


    thread FragmentOutput_0 output_2;



    (&output_2)->lit_0 = float4((lit_1 + _S285) * float3(fog_survives_0)  + (&kernelContext_30)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_1.w);


    (&output_2)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_2)->motion_0 = motion_1;
    return output_2;
}


#line 5035
struct vertexMain_Result_0
{
    float4 position_6 [[position]];
    float3 world_position_13 [[user(POSITION)]];
    float3 world_normal_2 [[user(NORMAL)]];
    float4 color_4 [[user(COLOR)]];
    uint material_6 [[user(TEXCOORD)]];
    float2 uv_4 [[user(TEXCOORD_1)]];
    float4 clip_position_2 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_2 [[user(TEXCOORD_3)]];
    float3 world_tangent_2 [[user(TEXCOORD_4)]];
    uint frame_6 [[user(TEXCOORD_5)]];
};


#line 5035
[[vertex]] vertexMain_Result_0 vertexMain(uint index_7 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_4 [[buffer(3)]], uint device* visible_instances_4 [[buffer(5)]], GpuInstance_natural_0 device* instances_4 [[buffer(2)]], GpuMesh_0 device* meshes_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], uint device* vertices_4 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_4 [[texture(2)]], GpuMaterial_natural_0 device* materials_4 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_4 [[texture(4)]], sampler base_color_sampler_4 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_4 [[texture(0)]], uint device* cluster_lights_4 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_4 [[texture(3)]], GpuLight_natural_0 device* lights_4 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_4 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d<float, access::sample> contact_shadow_4 [[texture(6)]], GpuProbe_natural_0 device* probes_4 [[buffer(9)]], texture2d_array<float, access::sample> probe_visibility_4 [[texture(7)]])
{

#line 5035
    thread KernelContext_0 kernelContext_31;

#line 5035
    (&kernelContext_31)->draw_0 = draw_4;

#line 5035
    (&kernelContext_31)->visible_instances_0 = visible_instances_4;

#line 5035
    (&kernelContext_31)->instances_0 = instances_4;

#line 5035
    (&kernelContext_31)->meshes_0 = meshes_4;

#line 5035
    (&kernelContext_31)->frame_0 = frame_7;

#line 5035
    (&kernelContext_31)->vertices_0 = vertices_4;

#line 5035
    (&kernelContext_31)->ambient_occlusion_0 = ambient_occlusion_4;

#line 5035
    (&kernelContext_31)->materials_0 = materials_4;

#line 5035
    (&kernelContext_31)->normal_textures_0 = normal_textures_4;

#line 5035
    (&kernelContext_31)->base_color_sampler_0 = base_color_sampler_4;

#line 5035
    (&kernelContext_31)->base_color_textures_0 = base_color_textures_4;

#line 5035
    (&kernelContext_31)->cluster_lights_0 = cluster_lights_4;

#line 5035
    (&kernelContext_31)->specular_dfg_0 = specular_dfg_4;

#line 5035
    (&kernelContext_31)->lights_0 = lights_4;

#line 5035
    (&kernelContext_31)->ltc_matrix_0 = ltc_matrix_4;

#line 5035
    (&kernelContext_31)->shadow_atlas_0 = shadow_atlas_4;

#line 5035
    (&kernelContext_31)->shadow_sampler_0 = shadow_sampler_4;

#line 5035
    (&kernelContext_31)->contact_shadow_0 = contact_shadow_4;

#line 5035
    (&kernelContext_31)->probes_0 = probes_4;

#line 5035
    (&kernelContext_31)->probe_visibility_0 = probe_visibility_4;

#line 5035
    GpuInstance_natural_0 device* _S286 = instances_4+visible_instances_4[draw_4->base_0 + instance_id_1];

#line 1755
    GpuMesh_0 mesh_3 = meshes_4[draw_4->mesh_0];

#line 1763
    bool _S287 = ((_S286->flags_0) & 2U) != 0U;

#line 1763
    uint base_vertex_3;
    if(_S287)
    {

#line 1764
        base_vertex_3 = _S286->base_vertex_0;

#line 1764
    }
    else
    {

#line 1764
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1764
    }

#line 1764
    MeshVertex_0 _S288 = load_vertex_0(index_7 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_31);

#line 1764
    uint previous_base_0;

#line 1777
    if(_S287)
    {

#line 1777
        previous_base_0 = _S286->previous_base_vertex_0;

#line 1777
    }
    else
    {

#line 1777
        previous_base_0 = base_vertex_3;

#line 1777
    }

#line 1777
    float3 _S289 = load_position_0(index_7 + previous_base_0, &kernelContext_31);

#line 1777
    matrix<float,int(4),int(4)>  _S290 = matrix<float,int(4),int(4)> (_S286->transform_0.data_0[int(0)][int(0)], _S286->transform_0.data_0[int(1)][int(0)], _S286->transform_0.data_0[int(2)][int(0)], _S286->transform_0.data_0[int(3)][int(0)], _S286->transform_0.data_0[int(0)][int(1)], _S286->transform_0.data_0[int(1)][int(1)], _S286->transform_0.data_0[int(2)][int(1)], _S286->transform_0.data_0[int(3)][int(1)], _S286->transform_0.data_0[int(0)][int(2)], _S286->transform_0.data_0[int(1)][int(2)], _S286->transform_0.data_0[int(2)][int(2)], _S286->transform_0.data_0[int(3)][int(2)], _S286->transform_0.data_0[int(0)][int(3)], _S286->transform_0.data_0[int(1)][int(3)], _S286->transform_0.data_0[int(2)][int(3)], _S286->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S288.position_1, 1.0f)) * (_S290)));

    thread VertexOutput_0 output_3;
    (&output_3)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_31)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_31)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_3)->world_position_1 = world_0.xyz;

#line 1791
    matrix<float,int(3),int(3)>  _S291 = matrix<float,int(3),int(3)> (_S290[int(0)].xyz, _S290[int(1)].xyz, _S290[int(2)].xyz);

#line 1791
    (&output_3)->world_normal_0 = (((_S288.basis_1.normal_0) * (normal_basis_0(_S291))));

#line 1797
    (&output_3)->world_tangent_0 = (((_S288.basis_1.tangent_1) * (_S291)));

#line 1797
    thread TangentFrame_0 _S292 = _S288.basis_1;

#line 1797
    uint _S293 = frame_word_0(mesh_3.flags_1, &_S292);
    (&output_3)->frame_3 = _S293;

#line 1798
    float4 _S294;

#line 1805
    if(((&kernelContext_31)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1805
        _S294 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1805
    }
    else
    {

#line 1805
        _S294 = _S288.color_1;

#line 1805
    }

#line 1804
    (&output_3)->color_2 = _S294;

#line 1811
    (&output_3)->material_2 = _S286->material_0;
    (&output_3)->uv_0 = _S288.uv0_0;

#line 1818
    (&output_3)->clip_position_0 = (&output_3)->position_3;
    (&output_3)->previous_clip_position_0 = ((((((float4(_S289, 1.0f)) * (matrix<float,int(4),int(4)> (_S286->previous_transform_0.data_0[int(0)][int(0)], _S286->previous_transform_0.data_0[int(1)][int(0)], _S286->previous_transform_0.data_0[int(2)][int(0)], _S286->previous_transform_0.data_0[int(3)][int(0)], _S286->previous_transform_0.data_0[int(0)][int(1)], _S286->previous_transform_0.data_0[int(1)][int(1)], _S286->previous_transform_0.data_0[int(2)][int(1)], _S286->previous_transform_0.data_0[int(3)][int(1)], _S286->previous_transform_0.data_0[int(0)][int(2)], _S286->previous_transform_0.data_0[int(1)][int(2)], _S286->previous_transform_0.data_0[int(2)][int(2)], _S286->previous_transform_0.data_0[int(3)][int(2)], _S286->previous_transform_0.data_0[int(0)][int(3)], _S286->previous_transform_0.data_0[int(1)][int(3)], _S286->previous_transform_0.data_0[int(2)][int(3)], _S286->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_31)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S295 = output_3;

#line 1822
    thread vertexMain_Result_0 _S296;

#line 1822
    (&_S296)->position_6 = _S295.position_3;

#line 1822
    (&_S296)->world_position_13 = _S295.world_position_1;

#line 1822
    (&_S296)->world_normal_2 = _S295.world_normal_0;

#line 1822
    (&_S296)->color_4 = _S295.color_2;

#line 1822
    (&_S296)->material_6 = _S295.material_2;

#line 1822
    (&_S296)->uv_4 = _S295.uv_0;

#line 1822
    (&_S296)->clip_position_2 = _S295.clip_position_0;

#line 1822
    (&_S296)->previous_clip_position_2 = _S295.previous_clip_position_0;

#line 1822
    (&_S296)->world_tangent_2 = _S295.world_tangent_0;

#line 1822
    (&_S296)->frame_6 = _S295.frame_3;

#line 1822
    return _S296;
}

