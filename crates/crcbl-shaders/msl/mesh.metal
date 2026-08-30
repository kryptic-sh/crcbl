#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2289 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2284
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2556
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2616
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2769
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2631
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2659
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1077
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1620
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1620
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


#line 746
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


#line 1626
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1626
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 325 "shaders/mesh.slang"
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


#line 325
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


#line 325
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


#line 325
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 325
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


#line 1120
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


#line 1131
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1134
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1484
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1607
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1607
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1609
        word_4 = 1U;

#line 1609
    }
    else
    {

#line 1609
        word_4 = 0U;

#line 1609
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1613
        word_4 = word_4 | 2U;

#line 1613
    }

#line 1612
    return word_4;
}


#line 1612
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1727
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 1727
    thread KernelContext_0 kernelContext_2;

#line 1727
    (&kernelContext_2)->draw_0 = draw_1;

#line 1727
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1727
    (&kernelContext_2)->instances_0 = instances_1;

#line 1727
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1727
    (&kernelContext_2)->frame_0 = frame_1;

#line 1727
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1727
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1727
    (&kernelContext_2)->materials_0 = materials_1;

#line 1727
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1727
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1727
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1727
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1727
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1727
    (&kernelContext_2)->lights_0 = lights_1;

#line 1727
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1727
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1727
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1727
    (&kernelContext_2)->probes_0 = probes_1;

#line 1727
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1730
    uint base_vertex_2;

#line 1736
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1736
        base_vertex_2 = _S7->base_vertex_0;

#line 1736
    }
    else
    {

#line 1736
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1736
    }

#line 1736
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1736
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1736
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 4017
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S11 = previous_0.w;

#line 4019
    if(_S11 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S11) ) * float2(0.5f, -0.5f);
}


#line 3985
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_3)
{

#line 3985
    texture2d<float, access::sample> _S12 = kernelContext_3->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S12).get_width(0)),(*((&height_0)) = (_S12).get_height(0));

    int3 _S13 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 3991
    return ((kernelContext_3->ambient_occlusion_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z)).x);
}


#line 3735
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S14 = axis_0.x;

#line 3739
    float _S15 = axis_0.y;

#line 3739
    bool _S16;

#line 3739
    if(_S14 >= _S15)
    {

#line 3739
        _S16 = _S14 >= (axis_0.z);

#line 3739
    }
    else
    {

#line 3739
        _S16 = false;

#line 3739
    }

#line 3739
    float2 planar_0;

#line 3739
    if(_S16)
    {

#line 3739
        planar_0 = world_position_0.zy;

#line 3739
    }
    else
    {

        if(_S15 >= (axis_0.z))
        {

#line 3743
            planar_0 = world_position_0.xz;

#line 3743
        }
        else
        {

#line 3743
            planar_0 = world_position_0.xy;

#line 3743
        }

#line 3739
    }

#line 3751
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 931
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 3772
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S17 = normal_2.z;

#line 3774
    float sign_z_0;

#line 3774
    if(_S17 >= 0.0f)
    {

#line 3774
        sign_z_0 = 1.0f;

#line 3774
    }
    else
    {

#line 3774
        sign_z_0 = -1.0f;

#line 3774
    }
    float a_0 = -1.0f / (sign_z_0 + _S17);
    float _S18 = normal_2.x;

#line 3776
    float _S19 = sign_z_0 * _S18;

#line 3776
    return float3(1.0f + _S19 * _S18 * a_0, _S19 * normal_2.y * a_0, - sign_z_0 * _S18);
}


#line 3826
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S20 = duvdy_0.y;

#line 3828
    float _S21 = duvdx_0.y;

#line 3828
    float winding_0;
    if((duvdx_0.x * _S20 - duvdy_0.x * _S21) < 0.0f)
    {

#line 3829
        winding_0 = -1.0f;

#line 3829
    }
    else
    {

#line 3829
        winding_0 = 1.0f;

#line 3829
    }
    float3 tangent_2 = (float3(_S20)  * dpdx_0 - float3(_S21)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 3838
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 3839
    float3 _S22;

#line 3848
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 3848
        _S22 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 3848
    }
    else
    {

#line 3848
        _S22 = orthonormal_tangent_0(normal_3);

#line 3848
    }

#line 3848
    (&basis_4)->tangent_1 = _S22;

    (&basis_4)->bitangent_0 = cross(normal_3, _S22);
    return basis_4;
}


#line 1491
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
    [[flat]] uint frame_2;
};


#line 3908
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_4)
{

#line 3920
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 3930
    uint _S23 = input_0->frame_2;
    if(((input_0->frame_2) & 1U) != 0U)
    {

#line 3939
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 3941
        float3 _S24;

#line 3946
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 3946
            _S24 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 3946
        }
        else
        {

#line 3946
            _S24 = orthonormal_tangent_0(normal_4);

#line 3946
        }

#line 3946
        (&basis_5)->tangent_1 = _S24;

#line 3952
        float3 _S25 = cross((&basis_5)->normal_0, _S24);

#line 3952
        float _S26;
        if((_S23 & 2U) != 0U)
        {

#line 3953
            _S26 = -1.0f;

#line 3953
        }
        else
        {

#line 3953
            _S26 = 1.0f;

#line 3953
        }

#line 3952
        (&basis_5)->bitangent_0 = _S25 * float3(_S26) ;

#line 3931
    }
    else
    {

#line 3957
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 3931
    }

#line 3961
    float3 _S27 = float3(uv_1, float(layer_0));
    float3 _S28 = ((kernelContext_4->normal_textures_0).sample((kernelContext_4->base_color_sampler_0), ((_S27)).xy, uint(((_S27)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 3962
    thread float3 tangent_space_0 = _S28;
    tangent_space_0.xy = _S28.xy * float2(normal_scale_1) ;

#line 3968
    float3 _S29 = normalize(tangent_space_0);

#line 3968
    tangent_space_0 = _S29;
    return normalize(float3(_S29.x)  * (&basis_5)->tangent_1 + float3(_S29.y)  * (&basis_5)->bitangent_0 + float3(_S29.z)  * (&basis_5)->normal_0);
}


#line 2424
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2435
    float3 _S30;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2436
        _S30 = - facet_1;

#line 2436
    }
    else
    {

#line 2436
        _S30 = facet_1;

#line 2436
    }

#line 2436
    return _S30;
}


#line 916
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3533
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_5)
{
    uint _S31 = max(kernelContext_5->frame_0->cluster_grid_0.x, 1U);
    uint _S32 = max(kernelContext_5->frame_0->cluster_grid_0.y, 1U);
    uint _S33 = max(kernelContext_5->frame_0->cluster_grid_0.z, 1U);
    uint _S34 = max(kernelContext_5->frame_0->cluster_grid_0.w, 1U);

#line 3543
    uint _S35 = uint(pixel_0.x) / _S34;

#line 3543
    uint _S36 = min(_S35, _S31 - 1U);
    uint _S37 = uint(pixel_0.y) / _S34;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3554
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S33 - 1U))) * _S32 + min(_S37, _S32 - 1U)) * _S31 + _S36;
}


#line 1856
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 1877
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_6)
{

#line 1877
    texture2d<float, access::sample> _S38 = kernelContext_6->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S38).get_width(0)),(*((&height_1)) = (_S38).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1883
    float2 _S39 = float2(1.0f) ;
    float2 _S40 = extent_1 - _S39;

#line 1884
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S40);
    float2 high_1 = min(low_1 + _S39, _S40);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 1902
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 1914
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_7)
{
    int _S41 = tap_1->lo_0.x;

#line 1916
    int _S42 = tap_1->lo_0.y;

#line 1916
    int3 _S43 = int3(_S41, _S42, int(0));
    int _S44 = tap_1->hi_0.x;

#line 1917
    int3 _S45 = int3(_S44, _S42, int(0));
    float2 _S46 = float2(tap_1->weight_0.x) ;
    int _S47 = tap_1->hi_0.y;

#line 1919
    int3 _S48 = int3(_S41, _S47, int(0));
    int3 _S49 = int3(_S44, _S47, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z)))), decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z)))), _S46), mix(decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)))), decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), _S46), float2(tap_1->weight_0.y) );
}


#line 3484
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3500
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3512
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3519
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2243
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2243
    float4 _S50 = float4(light_0->tangent_0) ;

    float3 _S51 = _S50.xyz;

#line 2245
    float3 across_0 = _S51 * float3(_S50.w) ;

#line 2245
    float4 _S52 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S51, _S52.xyz) * float3(_S52.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S53 = centre_0 - across_0;

#line 2248
    (*corners_0)[int(0)] = _S53 - down_0;
    float3 _S54 = centre_0 + across_0;

#line 2249
    (*corners_0)[int(1)] = _S54 - down_0;
    (*corners_0)[int(2)] = _S54 + down_0;
    (*corners_0)[int(3)] = _S53 + down_0;
    return;
}


#line 2001
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2004
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2005
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2005
    }
    else
    {

#line 2005
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2005
    }

#line 2005
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2006
        tangent_5 = across_1 / float3(span_0) ;

#line 2006
    }
    else
    {

#line 2006
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2006
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 1982
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2072
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2072
    float3 _S55 = polygon_0->corner_0[int(0)];

#line 2072
    float3 _S56 = polygon_0->corner_0[int(1)];

#line 2072
    float3 _S57 = polygon_0->corner_0[int(2)];

#line 2072
    float3 _S58 = polygon_0->corner_0[int(3)];

#line 2078
    float3 _S59 = float3(0.0f, 0.0f, 0.0f);


    float _S60 = polygon_0->corner_0[int(0)].z;

#line 2081
    int count_1;

#line 2081
    if(_S60 > 0.0f)
    {

#line 2081
        count_1 = int(1);

#line 2081
    }
    else
    {

#line 2081
        count_1 = int(0);

#line 2081
    }
    float _S61 = _S56.z;

#line 2082
    int _S62;

#line 2082
    if(_S61 > 0.0f)
    {

#line 2082
        _S62 = int(2);

#line 2082
    }
    else
    {

#line 2082
        _S62 = int(0);

#line 2082
    }

#line 2082
    int config_0 = count_1 + _S62;
    float _S63 = _S57.z;

#line 2083
    if(_S63 > 0.0f)
    {

#line 2083
        count_1 = int(4);

#line 2083
    }
    else
    {

#line 2083
        count_1 = int(0);

#line 2083
    }

#line 2083
    int config_1 = config_0 + count_1;
    float _S64 = _S58.z;

#line 2084
    if(_S64 > 0.0f)
    {

#line 2084
        count_1 = int(8);

#line 2084
    }
    else
    {

#line 2084
        count_1 = int(0);

#line 2084
    }

#line 2084
    int config_2 = config_1 + count_1;

#line 2084
    float3 l0_0;

#line 2084
    float3 l1_0;

#line 2084
    float3 l2_0;

#line 2084
    float3 l3_0;

#line 2084
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2087
        float3 _S65 = float3(_S60) ;


        float3 _S66 = float3(- _S61)  * _S55 + _S65 * _S56;
        float3 _S67 = float3(- _S64)  * _S55 + _S65 * _S58;

#line 2091
        count_1 = int(3);

#line 2091
        l0_0 = _S55;

#line 2091
        l1_0 = _S66;

#line 2091
        l2_0 = _S67;

#line 2091
        l3_0 = _S58;

#line 2091
        l4_0 = _S59;

#line 2087
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2093
            float3 _S68 = float3(_S61) ;


            float3 _S69 = float3(- _S60)  * _S56 + _S68 * _S55;
            float3 _S70 = float3(- _S63)  * _S56 + _S68 * _S57;

#line 2097
            count_1 = int(3);

#line 2097
            l0_0 = _S69;

#line 2097
            l1_0 = _S56;

#line 2097
            l2_0 = _S70;

#line 2097
            l3_0 = _S58;

#line 2097
            l4_0 = _S59;

#line 2093
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S71 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;
                float3 _S72 = float3(- _S64)  * _S55 + float3(_S60)  * _S58;

#line 2103
                count_1 = int(4);

#line 2103
                l0_0 = _S55;

#line 2103
                l1_0 = _S56;

#line 2103
                l2_0 = _S71;

#line 2103
                l3_0 = _S72;

#line 2103
                l4_0 = _S59;

#line 2099
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2105
                    float3 _S73 = float3(_S63) ;


                    float3 _S74 = float3(- _S64)  * _S57 + _S73 * _S58;
                    float3 _S75 = float3(- _S61)  * _S57 + _S73 * _S56;

#line 2109
                    count_1 = int(3);

#line 2109
                    l0_0 = _S74;

#line 2109
                    l1_0 = _S75;

#line 2109
                    l2_0 = _S57;

#line 2109
                    l3_0 = _S58;

#line 2109
                    l4_0 = _S59;

#line 2105
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S76 = float3(- _S60)  * _S56 + float3(_S61)  * _S55;
                        float3 _S77 = float3(- _S64)  * _S57 + float3(_S63)  * _S58;

#line 2115
                        count_1 = int(4);

#line 2115
                        l0_0 = _S76;

#line 2115
                        l1_0 = _S56;

#line 2115
                        l2_0 = _S57;

#line 2115
                        l3_0 = _S77;

#line 2115
                        l4_0 = _S59;

#line 2111
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2117
                            float3 _S78 = float3(- _S64) ;


                            float3 _S79 = _S78 * _S55 + float3(_S60)  * _S58;
                            float3 _S80 = _S78 * _S57 + float3(_S63)  * _S58;

#line 2121
                            count_1 = int(5);

#line 2121
                            l0_0 = _S55;

#line 2121
                            l1_0 = _S56;

#line 2121
                            l2_0 = _S57;

#line 2121
                            l3_0 = _S80;

#line 2121
                            l4_0 = _S79;

#line 2117
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2123
                                float3 _S81 = float3(_S64) ;


                                float3 _S82 = float3(- _S60)  * _S58 + _S81 * _S55;
                                float3 _S83 = float3(- _S63)  * _S58 + _S81 * _S57;

#line 2127
                                count_1 = int(3);

#line 2127
                                l0_0 = _S82;

#line 2127
                                l1_0 = _S83;

#line 2127
                                l2_0 = _S58;

#line 2127
                                l3_0 = _S58;

#line 2127
                                l4_0 = _S59;

#line 2123
                            }
                            else
                            {

#line 2130
                                if(config_2 == int(9))
                                {

                                    float3 _S84 = float3(- _S61)  * _S55 + float3(_S60)  * _S56;
                                    float3 _S85 = float3(- _S63)  * _S58 + float3(_S64)  * _S57;

#line 2134
                                    count_1 = int(4);

#line 2134
                                    l0_0 = _S55;

#line 2134
                                    l1_0 = _S84;

#line 2134
                                    l2_0 = _S85;

#line 2134
                                    l3_0 = _S58;

#line 2134
                                    l4_0 = _S59;

#line 2130
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S86 = float3(- _S64)  * _S57 + float3(_S63)  * _S58;
                                        float3 _S87 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;

#line 2141
                                        count_1 = int(5);

#line 2141
                                        l0_0 = _S55;

#line 2141
                                        l1_0 = _S56;

#line 2141
                                        l2_0 = _S87;

#line 2141
                                        l3_0 = _S86;

#line 2141
                                        l4_0 = _S58;

#line 2136
                                    }
                                    else
                                    {

#line 2143
                                        if(config_2 == int(12))
                                        {

                                            float3 _S88 = float3(- _S61)  * _S57 + float3(_S63)  * _S56;
                                            float3 _S89 = float3(- _S60)  * _S58 + float3(_S64)  * _S55;

#line 2147
                                            count_1 = int(4);

#line 2147
                                            l0_0 = _S89;

#line 2147
                                            l1_0 = _S88;

#line 2147
                                            l2_0 = _S57;

#line 2147
                                            l3_0 = _S58;

#line 2147
                                            l4_0 = _S59;

#line 2143
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S90 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;
                                                float3 _S91 = float3(- _S61)  * _S55 + float3(_S60)  * _S56;

#line 2155
                                                count_1 = int(5);

#line 2155
                                                l0_0 = _S55;

#line 2155
                                                l1_0 = _S91;

#line 2155
                                                l2_0 = _S90;

#line 2155
                                                l3_0 = _S57;

#line 2155
                                                l4_0 = _S58;

#line 2149
                                            }
                                            else
                                            {

#line 2157
                                                if(config_2 == int(14))
                                                {

#line 2157
                                                    float3 _S92 = float3(- _S60) ;


                                                    float3 _S93 = _S92 * _S58 + float3(_S64)  * _S55;
                                                    float3 _S94 = _S92 * _S56 + float3(_S61)  * _S55;

#line 2161
                                                    count_1 = int(5);

#line 2161
                                                    l0_0 = _S94;

#line 2161
                                                    l1_0 = _S93;

#line 2157
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2163
                                                        count_1 = int(4);

#line 2163
                                                    }
                                                    else
                                                    {

#line 2163
                                                        count_1 = int(0);

#line 2163
                                                    }

#line 2163
                                                    l0_0 = _S55;

#line 2163
                                                    l1_0 = _S59;

#line 2157
                                                }

#line 2078
                                                float3 _S95 = l1_0;

#line 2078
                                                l1_0 = _S56;

#line 2078
                                                l2_0 = _S57;

#line 2078
                                                l3_0 = _S58;

#line 2078
                                                l4_0 = _S95;

#line 2149
                                            }

#line 2143
                                        }

#line 2136
                                    }

#line 2130
                                }

#line 2123
                            }

#line 2117
                        }

#line 2111
                    }

#line 2105
                }

#line 2099
            }

#line 2093
        }

#line 2087
    }

#line 2171
    if(count_1 <= int(3))
    {

#line 2171
        l3_0 = l0_0;

#line 2171
        l4_0 = l0_0;

#line 2171
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2176
            l4_0 = l0_0;

#line 2176
        }

#line 2171
    }

#line 2181
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2044
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2050
    float weight_1;

#line 2055
    if(cosine_0 > 0.0f)
    {

#line 2055
        weight_1 = fit_0;

#line 2055
    }
    else
    {

#line 2055
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2055
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2201
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2203
    int corner_1 = int(0);
    for(;;)
    {

#line 2204
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2204
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2204
        corner_1 = corner_1 + int(1);

#line 2204
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2209
    thread LtcPolygon_0 _S96 = polygon_1;

#line 2209
    LtcPolygon_0 _S97 = ltc_clip_0(&_S96);
    polygon_1 = _S97;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2213
    int at_2 = int(0);

    for(;;)
    {

#line 2215
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2215
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2215
        at_2 = at_2 + int(1);

#line 2215
    }

#line 2222
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2222
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2223
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2223
    }
    else
    {

#line 2223
        sum_1 = sum_0;

#line 2223
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2227
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2227
    }

#line 2234
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 1930
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_8)
{
    int _S98 = tap_2->lo_0.x;

#line 1932
    int _S99 = tap_2->lo_0.y;

#line 1932
    int3 _S100 = int3(_S98, _S99, int(0));
    int _S101 = tap_2->hi_0.x;

#line 1933
    int3 _S102 = int3(_S101, _S99, int(0));
    float4 _S103 = float4(tap_2->weight_0.x) ;
    int _S104 = tap_2->hi_0.y;

#line 1935
    int3 _S105 = int3(_S98, _S104, int(0));
    int3 _S106 = int3(_S101, _S104, int(0));

    return mix(mix(((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S100)).xy), uint(((_S100)).z))), ((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S102)).xy), uint(((_S102)).z))), _S103), mix(((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S105)).xy), uint(((_S105)).z))), ((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), _S103), float4(tap_2->weight_0.y) );
}


#line 2017
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 1812
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 1819
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1826
    float _S107 = 1.0f - alpha2_0;

#line 1831
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S107 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S107 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2804
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_9)
{
    return kernelContext_9->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 2804
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 2864
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 2836
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_11)
{
    return rect_1.x / kernelContext_11->frame_0->shadow_params_0.x;
}


#line 2475
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 2791
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 2816
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_12)
{
    return kernelContext_12->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 2816
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_13)
{
    return kernelContext_13->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 311
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 2986
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_14)
{
    float2 texel_1 = kernelContext_14->frame_0->shadow_params_0.xy;

#line 2988
    float4 _S108 = atlas_rect_0(cascade_0, kernelContext_14);

#line 2988
    float2 _S109 = atlas_step_0(_S108, kernelContext_14);


    float2 _S110 = float2(0.5f, 0.5f) * _S109;


    float2 _S111 = float2(1.0f, 1.0f);

#line 2994
    float2 _S112 = _S111 / texel_1;

#line 2994
    uint index_1 = 0U;

#line 2994
    float sum_2 = 0.0f;

#line 2994
    float found_0 = 0.0f;



    for(;;)
    {

#line 2998
        if(index_1 < 16U)
        {
        }
        else
        {

#line 2998
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_1] * float2(8.0f) ;
        float _S113 = spoke_0.x;

#line 3001
        float _S114 = rotation_0.x;

#line 3001
        float _S115 = spoke_0.y;

#line 3001
        float _S116 = rotation_0.y;

#line 3009
        int3 _S117 = int3(int2(min(atlas_uv_0(_S108, clamp(tile_uv_1 + float2(_S113 * _S114 - _S115 * _S116, _S113 * _S116 + _S115 * _S114) * _S109, _S110, float2(1.0f)  - _S110)) * _S112, _S112 - _S111)), int(0));

#line 3009
        float depth_1 = ((kernelContext_14->shadow_atlas_0).read(vec<uint,2>(((_S117)).xy), uint(((_S117)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 3013
            sum_2 = sum_2 + depth_1;

#line 3013
            found_0 = found_1;

#line 3010
        }

#line 2998
        index_1 = index_1 + 1U;

#line 2998
    }

#line 3017
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 3028
    float _S118 = 2.0f * kernelContext_14->frame_0->cascade_far_0[cascade_0];

#line 3028
    float separation_0 = (sum_2 / found_0 - reference_0) * (_S118 + 40.0f);

#line 3028
    float _S119 = tile_texels_0(_S108, kernelContext_14);

    return clamp(separation_0 * 0.01999999955296516f / (_S118 / _S119), 2.0f, 8.0f);
}


#line 2886
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_15)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S120 = spoke_1.x;

#line 2891
    float _S121 = rotation_1.x;

#line 2891
    float _S122 = spoke_1.y;

#line 2891
    float _S123 = rotation_1.y;


    float _S124 = ((kernelContext_15->shadow_atlas_0).sample_compare((kernelContext_15->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_2 + float2(_S120 * _S121 - _S122 * _S123, _S120 * _S123 + _S122 * _S121) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2894
    return _S124;
}


#line 2916
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_2, KernelContext_0 thread* kernelContext_16)
{
    float2 _S125 = shadow_rotation_0(pixel_2);

#line 2918
    float4 _S126 = atlas_rect_1(tile_2, kernelContext_16);

    if(atlas_rect_is_empty_0(_S126))
    {
        return 1.0f;
    }

#line 2922
    float2 _S127 = atlas_step_1(_S126, kernelContext_16);

#line 2922
    uint spot_0 = 0U;

#line 2922
    float probe_0 = 0.0f;

#line 2927
    for(;;)
    {

#line 2927
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2927
            break;
        }

#line 2927
        float _S128 = tile_tap_0(_S126, _S127, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S125, reference_2, kernelContext_16);

        float probe_1 = probe_0 + _S128;

#line 2927
        spot_0 = spot_0 + 1U;

#line 2927
        probe_0 = probe_1;

#line 2927
    }

#line 2936
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2942
    uint index_2 = 0U;

#line 2942
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2946
        if(index_2 < 32U)
        {
        }
        else
        {

#line 2946
            break;
        }

#line 2946
        float _S129 = tile_tap_0(_S126, _S127, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S125, reference_2, kernelContext_16);

        float visibility_1 = visibility_0 + _S129;

#line 2946
        index_2 = index_2 + 1U;

#line 2946
        visibility_0 = visibility_1;

#line 2946
    }

#line 2951
    return visibility_0 / 32.0f;
}


#line 3082
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_17)
{

#line 3083
    float4 _S130 = atlas_rect_0(cascade_1, kernelContext_17);

#line 3117
    if(atlas_rect_is_empty_0(_S130))
    {


        return 1.0f;
    }
    float _S131 = 2.0f * kernelContext_17->frame_0->cascade_far_0[cascade_1];

#line 3123
    float _S132 = tile_texels_0(_S130, kernelContext_17);

#line 3123
    float texel_world_0 = _S131 / _S132;

#line 3130
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_17->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_17->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_17->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3134
    bool _S133;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3135
        _S133 = true;

#line 3135
    }
    else
    {

#line 3135
        _S133 = (ndc_0.z) <= 0.0f;

#line 3135
    }

#line 3135
    if(_S133)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3162
    float _S134 = ndc_0.z;

#line 3162
    float _S135 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S134, shadow_rotation_0(pixel_3), kernelContext_17);

#line 3162
    float _S136 = tile_pcf_0(cascade_1, tile_uv_4, _S134, pixel_3, _S135, kernelContext_17);
    return _S136;
}


#line 3179
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_18)
{

#line 3180
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3192
    float eye_distance_0 = length(world_position_5 - kernelContext_18->frame_0->camera_position_0.xyz);

#line 3192
    uint index_3 = 0U;

    for(;;)
    {

#line 3194
        if(index_3 < 2U)
        {
        }
        else
        {

#line 3194
            cascade_2 = 1U;

#line 3194
            break;
        }
        if(eye_distance_0 < kernelContext_18->frame_0->cascade_far_0[index_3])
        {

#line 3196
            cascade_2 = index_3;


            break;
        }

#line 3194
        index_3 = index_3 + 1U;

#line 3194
    }

#line 3194
    float _S137 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_18);

#line 3205
    uint _S138 = cascade_2 + 1U;

#line 3205
    if(_S138 >= 2U)
    {



        return _S137;
    }

#line 3218
    float band_0 = kernelContext_18->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_18->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S137;
    }

#line 3222
    float _S139 = cascade_visibility_0(_S138, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_18);

#line 3233
    return mix(_S137, _S139, blend_0);
}


#line 3436
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S140 = axis_2.x;

#line 3439
    float _S141 = axis_2.y;

#line 3439
    bool _S142;

#line 3439
    if(_S140 >= _S141)
    {

#line 3439
        _S142 = _S140 >= (axis_2.z);

#line 3439
    }
    else
    {

#line 3439
        _S142 = false;

#line 3439
    }

#line 3439
    uint _S143;

#line 3439
    if(_S142)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3441
            _S143 = 0U;

#line 3441
        }
        else
        {

#line 3441
            _S143 = 1U;

#line 3441
        }

#line 3441
        return _S143;
    }
    if(_S141 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3445
            _S143 = 2U;

#line 3445
        }
        else
        {

#line 3445
            _S143 = 3U;

#line 3445
        }

#line 3445
        return _S143;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3447
        _S143 = 4U;

#line 3447
    }
    else
    {

#line 3447
        _S143 = 5U;

#line 3447
    }

#line 3447
    return _S143;
}


#line 298
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 3340
float punctual_visibility_0(uint tile_4, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float map_world_0, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_19)
{

    uint atlas_0 = light_tile_0(tile_4);

#line 3343
    float4 _S144 = atlas_rect_0(atlas_0, kernelContext_19);

    if(atlas_rect_is_empty_0(_S144))
    {


        return 1.0f;
    }

#line 3349
    float _S145 = tile_texels_0(_S144, kernelContext_19);

    float texel_world_1 = map_world_0 / _S145;

#line 3361
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_19->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 3368
    float _S146 = clip_1.w;

#line 3368
    if(_S146 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S146) ;

#line 3372
    bool _S147;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3373
        _S147 = true;

#line 3373
    }
    else
    {

#line 3373
        _S147 = (ndc_1.z) <= 0.0f;

#line 3373
    }

#line 3373
    if(_S147)
    {

#line 3373
        _S147 = true;

#line 3373
    }
    else
    {

#line 3373
        _S147 = (ndc_1.z) > 1.0f;

#line 3373
    }

#line 3373
    if(_S147)
    {

#line 3380
        return 1.0f;
    }

#line 3380
    float _S148 = tile_pcf_0(atlas_0, float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_19);

#line 3390
    return _S148;
}


#line 3455
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_20)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3463
    float _S149 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_6, kernelContext_20);

#line 3469
    return _S149;
}


#line 3397
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_5, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_21)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3404
    float4 _S150 = float4(light_2->direction_0) ;

#line 3411
    float cos_outer_1 = _S150.w;

#line 3411
    float _S151 = punctual_visibility_0(tile_5, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S150.xyz)), 0.0f), geometric_normal_5, pixel_7, kernelContext_21);

#line 3418
    return _S151;
}


#line 1958
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 3662
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_22)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_22->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_22->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_22->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 968
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 3564
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_23)
{

    GpuProbe_natural_0 _S152 = kernelContext_23->probes_0[min((cell_1.z * kernelContext_23->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_23->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_23->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 3567
    GpuProbe_0 _S153 = { float4(_S152.sh_r_0) , float4(_S152.sh_g_0) , float4(_S152.sh_b_0)  };

#line 3567
    return _S153;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_1, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S154 = float4(t_1) ;

#line 3575
    (&blended_0)->sh_r_0 = mix(a_1->sh_r_0, b_0->sh_r_0, _S154);
    (&blended_0)->sh_g_0 = mix(a_1->sh_g_0, b_0->sh_g_0, _S154);
    (&blended_0)->sh_b_0 = mix(a_1->sh_b_0, b_0->sh_b_0, _S154);
    return blended_0;
}


#line 3615
float3 probe_irradiance_0(float3 world_position_9, float3 normal_7, KernelContext_0 thread* kernelContext_24)
{

#line 3615
    float3 _S155 = float3(1.0f) ;

#line 3620
    float3 _S156 = float3(0.0f, 0.0f, 0.0f);

#line 3620
    float3 last_0 = max(float3(kernelContext_24->frame_0->probe_counts_0.xyz) - _S155, _S156);
    float3 grid_0 = clamp((world_position_9 - kernelContext_24->frame_0->probe_origin_0.xyz) * kernelContext_24->frame_0->probe_inv_spacing_0.xyz, _S156, last_0);

    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S157 = uint3(base_2);



    uint3 _S158 = uint3(min(base_2 + _S155, last_0));

#line 3637
    uint _S159 = _S157.x;

#line 3637
    uint _S160 = _S157.y;

#line 3637
    uint _S161 = _S157.z;

#line 3637
    GpuProbe_0 _S162 = probe_at_0(uint3(_S159, _S160, _S161), kernelContext_24);

#line 3637
    uint _S163 = _S158.x;

#line 3637
    GpuProbe_0 _S164 = probe_at_0(uint3(_S163, _S160, _S161), kernelContext_24);

#line 3637
    float _S165 = f_0.x;

#line 3637
    thread GpuProbe_0 _S166 = _S162;

#line 3637
    thread GpuProbe_0 _S167 = _S164;

#line 3637
    GpuProbe_0 _S168 = lerp_probe_0(&_S166, &_S167, _S165);
    uint _S169 = _S158.y;

#line 3638
    GpuProbe_0 _S170 = probe_at_0(uint3(_S159, _S169, _S161), kernelContext_24);

#line 3638
    GpuProbe_0 _S171 = probe_at_0(uint3(_S163, _S169, _S161), kernelContext_24);

#line 3638
    thread GpuProbe_0 _S172 = _S170;

#line 3638
    thread GpuProbe_0 _S173 = _S171;

#line 3638
    GpuProbe_0 _S174 = lerp_probe_0(&_S172, &_S173, _S165);
    uint _S175 = _S158.z;

#line 3639
    GpuProbe_0 _S176 = probe_at_0(uint3(_S159, _S160, _S175), kernelContext_24);

#line 3639
    GpuProbe_0 _S177 = probe_at_0(uint3(_S163, _S160, _S175), kernelContext_24);

#line 3639
    thread GpuProbe_0 _S178 = _S176;

#line 3639
    thread GpuProbe_0 _S179 = _S177;

#line 3639
    GpuProbe_0 _S180 = lerp_probe_0(&_S178, &_S179, _S165);

#line 3639
    GpuProbe_0 _S181 = probe_at_0(uint3(_S159, _S169, _S175), kernelContext_24);

#line 3639
    GpuProbe_0 _S182 = probe_at_0(uint3(_S163, _S169, _S175), kernelContext_24);

#line 3639
    thread GpuProbe_0 _S183 = _S181;

#line 3639
    thread GpuProbe_0 _S184 = _S182;

#line 3639
    GpuProbe_0 _S185 = lerp_probe_0(&_S183, &_S184, _S165);

    float _S186 = f_0.y;

#line 3641
    thread GpuProbe_0 _S187 = _S168;

#line 3641
    thread GpuProbe_0 _S188 = _S174;

#line 3641
    GpuProbe_0 _S189 = lerp_probe_0(&_S187, &_S188, _S186);

#line 3641
    thread GpuProbe_0 _S190 = _S180;

#line 3641
    thread GpuProbe_0 _S191 = _S185;

#line 3641
    GpuProbe_0 _S192 = lerp_probe_0(&_S190, &_S191, _S186);

    float _S193 = f_0.z;

#line 3643
    thread GpuProbe_0 _S194 = _S189;

#line 3643
    thread GpuProbe_0 _S195 = _S192;

#line 3643
    GpuProbe_0 _S196 = lerp_probe_0(&_S194, &_S195, _S193);

    float4 basis_7 = float4(normal_7, 1.0f);
    return max(float3(dot(_S196.sh_r_0, basis_7), dot(_S196.sh_g_0, basis_7), dot(_S196.sh_b_0, basis_7)), _S156);
}


#line 941
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2309
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S197 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2317
    float kernel_0 = 0.0001984127011383f;

#line 2317
    int term_0 = int(6);

    for(;;)
    {

#line 2319
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2319
            break;
        }
        float _S198 = kernel_0 * _S197 + FOG_KERNEL_0[term_0];

#line 2319
        int term_1 = term_0 - int(1);

#line 2319
        kernel_0 = _S198;

#line 2319
        term_0 = term_1;

#line 2319
    }

#line 2326
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2336
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S199 = - d_0;

#line 2340
        float series_0 = 0.00833333376795053f;

#line 2340
        int term_2 = int(3);

        for(;;)
        {

#line 2342
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2342
                break;
            }
            float _S200 = series_0 * _S199 + FOG_RATIO_KERNEL_0[term_2];

#line 2342
            int term_3 = term_2 - int(1);

#line 2342
            series_0 = _S200;

#line 2342
            term_2 = term_3;

#line 2342
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2370
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2381
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2389
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 3688
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 3688
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
    [[flat]] uint frame_3 [[user(TEXCOORD_5)]];
};


#line 4027
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S201 [[stage_in]], float4 position_4 [[position]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 4027
    thread KernelContext_0 kernelContext_25;

#line 4027
    (&kernelContext_25)->draw_0 = draw_2;

#line 4027
    (&kernelContext_25)->visible_instances_0 = visible_instances_2;

#line 4027
    (&kernelContext_25)->instances_0 = instances_2;

#line 4027
    (&kernelContext_25)->meshes_0 = meshes_2;

#line 4027
    (&kernelContext_25)->frame_0 = frame_4;

#line 4027
    (&kernelContext_25)->vertices_0 = vertices_2;

#line 4027
    (&kernelContext_25)->ambient_occlusion_0 = ambient_occlusion_2;

#line 4027
    (&kernelContext_25)->materials_0 = materials_2;

#line 4027
    (&kernelContext_25)->normal_textures_0 = normal_textures_2;

#line 4027
    (&kernelContext_25)->base_color_sampler_0 = base_color_sampler_2;

#line 4027
    (&kernelContext_25)->base_color_textures_0 = base_color_textures_2;

#line 4027
    (&kernelContext_25)->cluster_lights_0 = cluster_lights_2;

#line 4027
    (&kernelContext_25)->specular_dfg_0 = specular_dfg_2;

#line 4027
    (&kernelContext_25)->lights_0 = lights_2;

#line 4027
    (&kernelContext_25)->ltc_matrix_0 = ltc_matrix_2;

#line 4027
    (&kernelContext_25)->shadow_atlas_0 = shadow_atlas_2;

#line 4027
    (&kernelContext_25)->shadow_sampler_0 = shadow_sampler_2;

#line 4027
    (&kernelContext_25)->probes_0 = probes_2;

#line 4039
    float3 vertex_normal_0 = normalize(_S201.world_normal_1);

#line 4044
    float2 motion_1 = motion_vector_0(_S201.clip_position_1, _S201.previous_clip_position_1);

#line 4053
    if((frame_4->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 4095
    if((frame_4->ambient_0.w) >= 3.5f)
    {

#line 4095
        float _S202 = occlusion_at_0(position_4.xy, &kernelContext_25);

        thread FragmentOutput_0 occlusion_0;

#line 4106
        (&occlusion_0)->lit_0 = float4(_S202, _S202, _S202, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_4->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S201.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 4123
    thread GpuMaterial_natural_0 _S203 = (&kernelContext_25)->materials_0[_S201.material_5];

#line 4123
    float2 uv_3;

#line 4148
    if(((&_S203)->tiling_0) == 1U)
    {

#line 4148
        uv_3 = physical_tile_uv_0(_S201.world_position_10, vertex_normal_0, (&_S203)->tile_metres_0);

#line 4148
    }
    else
    {

#line 4148
        uv_3 = _S201.uv_2;

#line 4148
    }

#line 4148
    uint _S204 = normal_layer_0(&_S203);

#line 4148
    thread VertexOutput_0 _S205;

#line 4148
    (&_S205)->position_3 = position_4;

#line 4148
    (&_S205)->world_position_1 = _S201.world_position_10;

#line 4148
    (&_S205)->world_normal_0 = _S201.world_normal_1;

#line 4148
    (&_S205)->color_2 = _S201.color_3;

#line 4148
    (&_S205)->material_2 = _S201.material_5;

#line 4148
    (&_S205)->uv_0 = _S201.uv_2;

#line 4148
    (&_S205)->clip_position_0 = _S201.clip_position_1;

#line 4148
    (&_S205)->previous_clip_position_0 = _S201.previous_clip_position_1;

#line 4148
    (&_S205)->world_tangent_0 = _S201.world_tangent_1;

#line 4148
    (&_S205)->frame_2 = _S201.frame_3;

#line 4148
    float3 _S206 = shading_normal_of_0(_S204, (&_S203)->normal_scale_0, &_S205, vertex_normal_0, uv_3, &kernelContext_25);

#line 4155
    if((frame_4->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 4157
        float3 _S207 = float3(0.5f) ;

#line 4169
        (&normals_0)->lit_0 = float4(_S206 * _S207 + _S207, 1.0f);

#line 4175
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_25)->frame_0->camera_position_0.xyz - _S201.world_position_10);



    float3 _S208 = geometric_normal_of_0(_S201.world_position_10, vertex_normal_0);

#line 4184
    uint _S209 = base_color_layer_0(&_S203);

#line 4199
    float3 _S210 = float3(uv_3, float(_S209));
    float4 albedo_0 = _S201.color_3 * float4((&_S203)->base_color_0)  * (((&kernelContext_25)->base_color_textures_0).sample(((&kernelContext_25)->base_color_sampler_0), ((_S210)).xy, uint(((_S210)).z)));

#line 4206
    float metallic_1 = saturate((&_S203)->metallic_0);
    float roughness_2 = clamp((&_S203)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S211 = alpha_0 * alpha_0;

#line 4215
    float3 _S212 = albedo_0.xyz;

#line 4215
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S212, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S212 * float3((1.0f - metallic_1)) ;

#line 4222
    float _S213 = max(dot(_S206, to_eye_1), 0.00009999999747379f);

#line 4232
    float2 _S214 = position_4.xy;

#line 4232
    uint _S215 = froxel_of_0(_S214, (((float4(_S201.world_position_10, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_25);

#line 4232
    uint base_3 = _S215 * 17U;

#line 4237
    uint _S216 = min((&kernelContext_25)->cluster_lights_0[base_3], 16U);

#line 4237
    TableTap_0 _S217 = table_tap_0(_S213, roughness_2, &kernelContext_25);

#line 4237
    thread TableTap_0 _S218 = _S217;

#line 4237
    float2 _S219 = dfg_at_0(&_S218, &kernelContext_25);

#line 4246
    float _S220 = _S219.x;

#line 4246
    float _S221 = _S219.y;

#line 4246
    float3 _S222 = f0_2 * float3(_S220)  + float3(_S221) ;

#line 4252
    float3 _S223 = float3(0.0f, 0.0f, 0.0f);

#line 4252
    uint slot_0 = 0U;

#line 4252
    float3 direct_0 = _S223;

#line 4252
    float3 gloss_0 = _S223;

    for(;;)
    {

#line 4254
        if(slot_0 < _S216)
        {
        }
        else
        {

#line 4254
            break;
        }

#line 4254
        thread GpuLight_natural_0 _S224 = (&kernelContext_25)->lights_0[(&kernelContext_25)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 4254
        uint _S225 = (&_S224)->kind_0;

#line 4263
        bool _S226 = ((&_S224)->kind_0) == 0U;

#line 4263
        float3 to_light_7;

#line 4263
        float reach_0;

#line 4263
        if(_S226)
        {

#line 4263
            to_light_7 = normalize((float4((&_S224)->direction_0) ).xyz);

#line 4263
            reach_0 = 1.0f;

#line 4263
        }
        else
        {


            if(_S225 == 3U)
            {

#line 4268
                float4 _S227 = float4((&_S224)->position_0) ;

#line 4276
                float3 offset_0 = _S227.xyz - _S201.world_position_10;
                float distance_3 = length(offset_0);

                float _S228 = range_window_0(distance_3, _S227.w);

#line 4279
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 4279
                reach_0 = _S228;

#line 4268
            }
            else
            {

#line 4268
                float4 _S229 = float4((&_S224)->position_0) ;

#line 4283
                float3 offset_1 = _S229.xyz - _S201.world_position_10;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_1 = punctual_falloff_0(distance_4, _S229.w);
                if(_S225 == 2U)
                {

#line 4287
                    float4 _S230 = float4((&_S224)->direction_0) ;

#line 4287
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S230.xyz, _S230.w, (&_S224)->cos_inner_0);

#line 4287
                }
                else
                {

#line 4287
                    reach_0 = reach_1;

#line 4287
                }

#line 4287
                to_light_7 = to_light_8;

#line 4268
            }

#line 4263
        }

#line 4296
        float n_dot_l_5 = dot(_S206, to_light_7);

#line 4296
        float3 specular_0;

#line 4296
        float diffuse_0;


        if(_S225 == 3U)
        {

#line 4309
            thread array<float3, int(4)> corners_2;

#line 4309
            rect_corners_0(&_S224, _S201.world_position_10, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S206, to_eye_1, _S213);

#line 4311
            thread array<float3, int(4)> _S231 = corners_2;

#line 4311
            float _S232 = ltc_irradiance_0(to_local_0, &_S231);

#line 4311
            thread TableTap_0 _S233 = _S217;

#line 4311
            float4 _S234 = ltc_at_0(&_S233, &kernelContext_25);

            matrix<float,int(3),int(3)>  _S235 = (((to_local_0) * (ltc_transform_0(_S234))));

#line 4313
            thread array<float3, int(4)> _S236 = corners_2;

#line 4313
            float _S237 = ltc_irradiance_0(_S235, &_S236);
            float3 _S238 = float3(_S237)  * _S222;

#line 4314
            diffuse_0 = _S232;

#line 4314
            specular_0 = _S238;

#line 4299
        }
        else
        {

#line 4319
            float _S239 = max(n_dot_l_5, 0.0f);

#line 4326
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 4334
            float3 specular_1 = ggx_lobe_0(_S211, f0_2, _S239, _S213, max(dot(_S206, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S239) ;

#line 4334
            diffuse_0 = _S239;

#line 4334
            specular_0 = specular_1;

#line 4299
        }

#line 4299
        float3 specular_2;

#line 4342
        if((((&_S224)->flags_3) & 1U) != 0U)
        {

#line 4342
            specular_2 = _S223;

#line 4342
        }
        else
        {

#line 4342
            specular_2 = specular_0;

#line 4342
        }

#line 4342
        float reach_2;

#line 4360
        if(_S226)
        {

#line 4360
            float _S240 = sun_visibility_0(_S201.world_position_10, to_light_7, n_dot_l_5, _S208, _S214, &kernelContext_25);

#line 4360
            reach_2 = _S240;

#line 4360
        }
        else
        {


            if(_S225 == 1U)
            {

#line 4365
                uint _S241 = (&_S224)->shadow_tile_0;

#line 4377
                if(((&_S224)->shadow_tile_0) <= 8U)
                {

#line 4377
                    float _S242 = point_visibility_0(&_S224, _S241, _S201.world_position_10, to_light_7, n_dot_l_5, _S208, _S214, &kernelContext_25);

#line 4377
                    reach_2 = reach_0 * _S242;

#line 4377
                }
                else
                {

#line 4377
                    reach_2 = reach_0;

#line 4377
                }

#line 4365
            }
            else
            {

#line 4365
                uint _S243 = (&_S224)->shadow_tile_0;

#line 4383
                if(((&_S224)->shadow_tile_0) < 14U)
                {

#line 4383
                    float _S244 = spot_visibility_0(&_S224, _S243, _S201.world_position_10, to_light_7, n_dot_l_5, _S208, _S214, &kernelContext_25);

#line 4383
                    reach_2 = reach_0 * _S244;

#line 4383
                }
                else
                {

#line 4383
                    reach_2 = reach_0;

#line 4383
                }

#line 4365
            }

#line 4360
        }

#line 4391
        float3 _S245 = (float4((&_S224)->color_0) ).xyz;

#line 4391
        float3 direct_1 = direct_0 + _S245 * float3((diffuse_0 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S245 * (specular_2 * float3(reach_2) );

#line 4254
        slot_0 = slot_0 + 1U;

#line 4254
        direct_0 = direct_1;

#line 4254
        gloss_0 = gloss_1;

#line 4254
    }

#line 4406
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S220 + _S221);

#line 4406
    float _S246 = occlusion_at_0(_S214, &kernelContext_25);

#line 4442
    float3 _S247 = frame_4->ambient_0.xyz;

#line 4442
    float3 _S248 = sky_irradiance_0(_S206, &kernelContext_25);

#line 4442
    float3 _S249 = _S247 + _S248;

#line 4442
    float3 _S250 = probe_irradiance_0(_S201.world_position_10, _S206, &kernelContext_25);

#line 4463
    float3 lit_1 = diffuse_albedo_0 * ((_S249 + _S250) * float3(_S246)  + direct_0) + gloss_2;

#line 4463
    float3 _S251 = emissive_of_0(&_S203);

#line 4499
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_25)->frame_0->fog_params_0.x, (&kernelContext_25)->frame_0->fog_params_0.y, (&kernelContext_25)->frame_0->camera_position_0.y - (&kernelContext_25)->frame_0->fog_params_0.z, _S201.world_position_10.y - (&kernelContext_25)->frame_0->fog_params_0.z, length((&kernelContext_25)->frame_0->camera_position_0.xyz - _S201.world_position_10)));


    thread FragmentOutput_0 output_1;



    (&output_1)->lit_0 = float4((lit_1 + _S251) * float3(fog_survives_0)  + (&kernelContext_25)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_1)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_1)->motion_0 = motion_1;
    return output_1;
}


#line 4512
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
    uint frame_5 [[user(TEXCOORD_5)]];
};


#line 4512
[[vertex]] vertexMain_Result_0 vertexMain(uint index_4 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_6 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]])
{

#line 4512
    thread KernelContext_0 kernelContext_26;

#line 4512
    (&kernelContext_26)->draw_0 = draw_3;

#line 4512
    (&kernelContext_26)->visible_instances_0 = visible_instances_3;

#line 4512
    (&kernelContext_26)->instances_0 = instances_3;

#line 4512
    (&kernelContext_26)->meshes_0 = meshes_3;

#line 4512
    (&kernelContext_26)->frame_0 = frame_6;

#line 4512
    (&kernelContext_26)->vertices_0 = vertices_3;

#line 4512
    (&kernelContext_26)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4512
    (&kernelContext_26)->materials_0 = materials_3;

#line 4512
    (&kernelContext_26)->normal_textures_0 = normal_textures_3;

#line 4512
    (&kernelContext_26)->base_color_sampler_0 = base_color_sampler_3;

#line 4512
    (&kernelContext_26)->base_color_textures_0 = base_color_textures_3;

#line 4512
    (&kernelContext_26)->cluster_lights_0 = cluster_lights_3;

#line 4512
    (&kernelContext_26)->specular_dfg_0 = specular_dfg_3;

#line 4512
    (&kernelContext_26)->lights_0 = lights_3;

#line 4512
    (&kernelContext_26)->ltc_matrix_0 = ltc_matrix_3;

#line 4512
    (&kernelContext_26)->shadow_atlas_0 = shadow_atlas_3;

#line 4512
    (&kernelContext_26)->shadow_sampler_0 = shadow_sampler_3;

#line 4512
    (&kernelContext_26)->probes_0 = probes_3;

#line 4512
    GpuInstance_natural_0 device* _S252 = instances_3+visible_instances_3[draw_3->base_0 + instance_id_1];

#line 1626
    GpuMesh_0 mesh_3 = meshes_3[draw_3->mesh_0];

#line 1634
    bool _S253 = ((_S252->flags_0) & 2U) != 0U;

#line 1634
    uint base_vertex_3;
    if(_S253)
    {

#line 1635
        base_vertex_3 = _S252->base_vertex_0;

#line 1635
    }
    else
    {

#line 1635
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1635
    }

#line 1635
    MeshVertex_0 _S254 = load_vertex_0(index_4 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_26);

#line 1635
    uint previous_base_0;

#line 1648
    if(_S253)
    {

#line 1648
        previous_base_0 = _S252->previous_base_vertex_0;

#line 1648
    }
    else
    {

#line 1648
        previous_base_0 = base_vertex_3;

#line 1648
    }

#line 1648
    float3 _S255 = load_position_0(index_4 + previous_base_0, &kernelContext_26);

#line 1648
    matrix<float,int(4),int(4)>  _S256 = matrix<float,int(4),int(4)> (_S252->transform_0.data_0[int(0)][int(0)], _S252->transform_0.data_0[int(1)][int(0)], _S252->transform_0.data_0[int(2)][int(0)], _S252->transform_0.data_0[int(3)][int(0)], _S252->transform_0.data_0[int(0)][int(1)], _S252->transform_0.data_0[int(1)][int(1)], _S252->transform_0.data_0[int(2)][int(1)], _S252->transform_0.data_0[int(3)][int(1)], _S252->transform_0.data_0[int(0)][int(2)], _S252->transform_0.data_0[int(1)][int(2)], _S252->transform_0.data_0[int(2)][int(2)], _S252->transform_0.data_0[int(3)][int(2)], _S252->transform_0.data_0[int(0)][int(3)], _S252->transform_0.data_0[int(1)][int(3)], _S252->transform_0.data_0[int(2)][int(3)], _S252->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S254.position_1, 1.0f)) * (_S256)));

    thread VertexOutput_0 output_2;
    (&output_2)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_26)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_2)->world_position_1 = world_0.xyz;

#line 1662
    matrix<float,int(3),int(3)>  _S257 = matrix<float,int(3),int(3)> (_S256[int(0)].xyz, _S256[int(1)].xyz, _S256[int(2)].xyz);

#line 1662
    (&output_2)->world_normal_0 = (((_S254.basis_1.normal_0) * (normal_basis_0(_S257))));

#line 1668
    (&output_2)->world_tangent_0 = (((_S254.basis_1.tangent_1) * (_S257)));

#line 1668
    thread TangentFrame_0 _S258 = _S254.basis_1;

#line 1668
    uint _S259 = frame_word_0(mesh_3.flags_1, &_S258);
    (&output_2)->frame_2 = _S259;

#line 1669
    float4 _S260;

#line 1676
    if(((&kernelContext_26)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1676
        _S260 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1676
    }
    else
    {

#line 1676
        _S260 = _S254.color_1;

#line 1676
    }

#line 1675
    (&output_2)->color_2 = _S260;

#line 1682
    (&output_2)->material_2 = _S252->material_0;
    (&output_2)->uv_0 = _S254.uv0_0;

#line 1689
    (&output_2)->clip_position_0 = (&output_2)->position_3;
    (&output_2)->previous_clip_position_0 = ((((((float4(_S255, 1.0f)) * (matrix<float,int(4),int(4)> (_S252->previous_transform_0.data_0[int(0)][int(0)], _S252->previous_transform_0.data_0[int(1)][int(0)], _S252->previous_transform_0.data_0[int(2)][int(0)], _S252->previous_transform_0.data_0[int(3)][int(0)], _S252->previous_transform_0.data_0[int(0)][int(1)], _S252->previous_transform_0.data_0[int(1)][int(1)], _S252->previous_transform_0.data_0[int(2)][int(1)], _S252->previous_transform_0.data_0[int(3)][int(1)], _S252->previous_transform_0.data_0[int(0)][int(2)], _S252->previous_transform_0.data_0[int(1)][int(2)], _S252->previous_transform_0.data_0[int(2)][int(2)], _S252->previous_transform_0.data_0[int(3)][int(2)], _S252->previous_transform_0.data_0[int(0)][int(3)], _S252->previous_transform_0.data_0[int(1)][int(3)], _S252->previous_transform_0.data_0[int(2)][int(3)], _S252->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_26)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S261 = output_2;

#line 1693
    thread vertexMain_Result_0 _S262;

#line 1693
    (&_S262)->position_5 = _S261.position_3;

#line 1693
    (&_S262)->world_position_11 = _S261.world_position_1;

#line 1693
    (&_S262)->world_normal_2 = _S261.world_normal_0;

#line 1693
    (&_S262)->color_4 = _S261.color_2;

#line 1693
    (&_S262)->material_6 = _S261.material_2;

#line 1693
    (&_S262)->uv_4 = _S261.uv_0;

#line 1693
    (&_S262)->clip_position_2 = _S261.clip_position_0;

#line 1693
    (&_S262)->previous_clip_position_2 = _S261.previous_clip_position_0;

#line 1693
    (&_S262)->world_tangent_2 = _S261.world_tangent_0;

#line 1693
    (&_S262)->frame_5 = _S261.frame_2;

#line 1693
    return _S262;
}

