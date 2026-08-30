#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2280 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2275
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2547
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2607
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2759
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2622
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2650
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1068
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1611
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1611
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


#line 737
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


#line 1617
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1617
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 3332
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
};


#line 3332
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


#line 3332
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


#line 3332
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 3332
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


#line 1111 "shaders/mesh.slang"
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


#line 1122
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1125
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1475
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1598
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1598
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1600
        word_4 = 1U;

#line 1600
    }
    else
    {

#line 1600
        word_4 = 0U;

#line 1600
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1604
        word_4 = word_4 | 2U;

#line 1604
    }

#line 1603
    return word_4;
}


#line 1603
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1718
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 1718
    thread KernelContext_0 kernelContext_2;

#line 1718
    (&kernelContext_2)->draw_0 = draw_1;

#line 1718
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1718
    (&kernelContext_2)->instances_0 = instances_1;

#line 1718
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1718
    (&kernelContext_2)->frame_0 = frame_1;

#line 1718
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1718
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1718
    (&kernelContext_2)->materials_0 = materials_1;

#line 1718
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1718
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1718
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1718
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1718
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1718
    (&kernelContext_2)->lights_0 = lights_1;

#line 1718
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1718
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1718
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1718
    (&kernelContext_2)->probes_0 = probes_1;

#line 1718
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1721
    uint base_vertex_2;

#line 1727
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1727
        base_vertex_2 = _S7->base_vertex_0;

#line 1727
    }
    else
    {

#line 1727
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1727
    }

#line 1727
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1727
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1727
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 3905
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S11 = previous_0.w;

#line 3907
    if(_S11 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S11) ) * float2(0.5f, -0.5f);
}


#line 3873
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_3)
{

#line 3873
    texture2d<float, access::sample> _S12 = kernelContext_3->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S12).get_width(0)),(*((&height_0)) = (_S12).get_height(0));

    int3 _S13 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 3879
    return ((kernelContext_3->ambient_occlusion_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z)).x);
}


#line 3623
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S14 = axis_0.x;

#line 3627
    float _S15 = axis_0.y;

#line 3627
    bool _S16;

#line 3627
    if(_S14 >= _S15)
    {

#line 3627
        _S16 = _S14 >= (axis_0.z);

#line 3627
    }
    else
    {

#line 3627
        _S16 = false;

#line 3627
    }

#line 3627
    float2 planar_0;

#line 3627
    if(_S16)
    {

#line 3627
        planar_0 = world_position_0.zy;

#line 3627
    }
    else
    {

        if(_S15 >= (axis_0.z))
        {

#line 3631
            planar_0 = world_position_0.xz;

#line 3631
        }
        else
        {

#line 3631
            planar_0 = world_position_0.xy;

#line 3631
        }

#line 3627
    }

#line 3639
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 922
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 3660
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S17 = normal_2.z;

#line 3662
    float sign_z_0;

#line 3662
    if(_S17 >= 0.0f)
    {

#line 3662
        sign_z_0 = 1.0f;

#line 3662
    }
    else
    {

#line 3662
        sign_z_0 = -1.0f;

#line 3662
    }
    float a_0 = -1.0f / (sign_z_0 + _S17);
    float _S18 = normal_2.x;

#line 3664
    float _S19 = sign_z_0 * _S18;

#line 3664
    return float3(1.0f + _S19 * _S18 * a_0, _S19 * normal_2.y * a_0, - sign_z_0 * _S18);
}


#line 3714
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S20 = duvdy_0.y;

#line 3716
    float _S21 = duvdx_0.y;

#line 3716
    float winding_0;
    if((duvdx_0.x * _S20 - duvdy_0.x * _S21) < 0.0f)
    {

#line 3717
        winding_0 = -1.0f;

#line 3717
    }
    else
    {

#line 3717
        winding_0 = 1.0f;

#line 3717
    }
    float3 tangent_2 = (float3(_S20)  * dpdx_0 - float3(_S21)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 3726
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 3727
    float3 _S22;

#line 3736
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 3736
        _S22 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 3736
    }
    else
    {

#line 3736
        _S22 = orthonormal_tangent_0(normal_3);

#line 3736
    }

#line 3736
    (&basis_4)->tangent_1 = _S22;

    (&basis_4)->bitangent_0 = cross(normal_3, _S22);
    return basis_4;
}


#line 1482
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


#line 3796
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_4)
{

#line 3808
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 3818
    uint _S23 = input_0->frame_2;
    if(((input_0->frame_2) & 1U) != 0U)
    {

#line 3827
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 3829
        float3 _S24;

#line 3834
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 3834
            _S24 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 3834
        }
        else
        {

#line 3834
            _S24 = orthonormal_tangent_0(normal_4);

#line 3834
        }

#line 3834
        (&basis_5)->tangent_1 = _S24;

#line 3840
        float3 _S25 = cross((&basis_5)->normal_0, _S24);

#line 3840
        float _S26;
        if((_S23 & 2U) != 0U)
        {

#line 3841
            _S26 = -1.0f;

#line 3841
        }
        else
        {

#line 3841
            _S26 = 1.0f;

#line 3841
        }

#line 3840
        (&basis_5)->bitangent_0 = _S25 * float3(_S26) ;

#line 3819
    }
    else
    {

#line 3845
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 3819
    }

#line 3849
    float3 _S27 = float3(uv_1, float(layer_0));
    float3 _S28 = ((kernelContext_4->normal_textures_0).sample((kernelContext_4->base_color_sampler_0), ((_S27)).xy, uint(((_S27)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 3850
    thread float3 tangent_space_0 = _S28;
    tangent_space_0.xy = _S28.xy * float2(normal_scale_1) ;

#line 3856
    float3 _S29 = normalize(tangent_space_0);

#line 3856
    tangent_space_0 = _S29;
    return normalize(float3(_S29.x)  * (&basis_5)->tangent_1 + float3(_S29.y)  * (&basis_5)->bitangent_0 + float3(_S29.z)  * (&basis_5)->normal_0);
}


#line 2415
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2426
    float3 _S30;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2427
        _S30 = - facet_1;

#line 2427
    }
    else
    {

#line 2427
        _S30 = facet_1;

#line 2427
    }

#line 2427
    return _S30;
}


#line 907
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3421
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_5)
{
    uint _S31 = max(kernelContext_5->frame_0->cluster_grid_0.x, 1U);
    uint _S32 = max(kernelContext_5->frame_0->cluster_grid_0.y, 1U);
    uint _S33 = max(kernelContext_5->frame_0->cluster_grid_0.z, 1U);
    uint _S34 = max(kernelContext_5->frame_0->cluster_grid_0.w, 1U);

#line 3431
    uint _S35 = uint(pixel_0.x) / _S34;

#line 3431
    uint _S36 = min(_S35, _S31 - 1U);
    uint _S37 = uint(pixel_0.y) / _S34;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3442
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S33 - 1U))) * _S32 + min(_S37, _S32 - 1U)) * _S31 + _S36;
}


#line 1847
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 1868
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_6)
{

#line 1868
    texture2d<float, access::sample> _S38 = kernelContext_6->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S38).get_width(0)),(*((&height_1)) = (_S38).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1874
    float2 _S39 = float2(1.0f) ;
    float2 _S40 = extent_1 - _S39;

#line 1875
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S40);
    float2 high_1 = min(low_1 + _S39, _S40);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 1893
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 1905
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_7)
{
    int _S41 = tap_1->lo_0.x;

#line 1907
    int _S42 = tap_1->lo_0.y;

#line 1907
    int3 _S43 = int3(_S41, _S42, int(0));
    int _S44 = tap_1->hi_0.x;

#line 1908
    int3 _S45 = int3(_S44, _S42, int(0));
    float2 _S46 = float2(tap_1->weight_0.x) ;
    int _S47 = tap_1->hi_0.y;

#line 1910
    int3 _S48 = int3(_S41, _S47, int(0));
    int3 _S49 = int3(_S44, _S47, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z)))), decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z)))), _S46), mix(decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)))), decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), _S46), float2(tap_1->weight_0.y) );
}


#line 3372
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3388
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3400
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3407
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2234
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2234
    float4 _S50 = float4(light_0->tangent_0) ;

    float3 _S51 = _S50.xyz;

#line 2236
    float3 across_0 = _S51 * float3(_S50.w) ;

#line 2236
    float4 _S52 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S51, _S52.xyz) * float3(_S52.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S53 = centre_0 - across_0;

#line 2239
    (*corners_0)[int(0)] = _S53 - down_0;
    float3 _S54 = centre_0 + across_0;

#line 2240
    (*corners_0)[int(1)] = _S54 - down_0;
    (*corners_0)[int(2)] = _S54 + down_0;
    (*corners_0)[int(3)] = _S53 + down_0;
    return;
}


#line 1992
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 1995
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 1996
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 1996
    }
    else
    {

#line 1996
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 1996
    }

#line 1996
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 1997
        tangent_5 = across_1 / float3(span_0) ;

#line 1997
    }
    else
    {

#line 1997
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 1997
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 1973
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2063
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2063
    float3 _S55 = polygon_0->corner_0[int(0)];

#line 2063
    float3 _S56 = polygon_0->corner_0[int(1)];

#line 2063
    float3 _S57 = polygon_0->corner_0[int(2)];

#line 2063
    float3 _S58 = polygon_0->corner_0[int(3)];

#line 2069
    float3 _S59 = float3(0.0f, 0.0f, 0.0f);


    float _S60 = polygon_0->corner_0[int(0)].z;

#line 2072
    int count_1;

#line 2072
    if(_S60 > 0.0f)
    {

#line 2072
        count_1 = int(1);

#line 2072
    }
    else
    {

#line 2072
        count_1 = int(0);

#line 2072
    }
    float _S61 = _S56.z;

#line 2073
    int _S62;

#line 2073
    if(_S61 > 0.0f)
    {

#line 2073
        _S62 = int(2);

#line 2073
    }
    else
    {

#line 2073
        _S62 = int(0);

#line 2073
    }

#line 2073
    int config_0 = count_1 + _S62;
    float _S63 = _S57.z;

#line 2074
    if(_S63 > 0.0f)
    {

#line 2074
        count_1 = int(4);

#line 2074
    }
    else
    {

#line 2074
        count_1 = int(0);

#line 2074
    }

#line 2074
    int config_1 = config_0 + count_1;
    float _S64 = _S58.z;

#line 2075
    if(_S64 > 0.0f)
    {

#line 2075
        count_1 = int(8);

#line 2075
    }
    else
    {

#line 2075
        count_1 = int(0);

#line 2075
    }

#line 2075
    int config_2 = config_1 + count_1;

#line 2075
    float3 l0_0;

#line 2075
    float3 l1_0;

#line 2075
    float3 l2_0;

#line 2075
    float3 l3_0;

#line 2075
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2078
        float3 _S65 = float3(_S60) ;


        float3 _S66 = float3(- _S61)  * _S55 + _S65 * _S56;
        float3 _S67 = float3(- _S64)  * _S55 + _S65 * _S58;

#line 2082
        count_1 = int(3);

#line 2082
        l0_0 = _S55;

#line 2082
        l1_0 = _S66;

#line 2082
        l2_0 = _S67;

#line 2082
        l3_0 = _S58;

#line 2082
        l4_0 = _S59;

#line 2078
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2084
            float3 _S68 = float3(_S61) ;


            float3 _S69 = float3(- _S60)  * _S56 + _S68 * _S55;
            float3 _S70 = float3(- _S63)  * _S56 + _S68 * _S57;

#line 2088
            count_1 = int(3);

#line 2088
            l0_0 = _S69;

#line 2088
            l1_0 = _S56;

#line 2088
            l2_0 = _S70;

#line 2088
            l3_0 = _S58;

#line 2088
            l4_0 = _S59;

#line 2084
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S71 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;
                float3 _S72 = float3(- _S64)  * _S55 + float3(_S60)  * _S58;

#line 2094
                count_1 = int(4);

#line 2094
                l0_0 = _S55;

#line 2094
                l1_0 = _S56;

#line 2094
                l2_0 = _S71;

#line 2094
                l3_0 = _S72;

#line 2094
                l4_0 = _S59;

#line 2090
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2096
                    float3 _S73 = float3(_S63) ;


                    float3 _S74 = float3(- _S64)  * _S57 + _S73 * _S58;
                    float3 _S75 = float3(- _S61)  * _S57 + _S73 * _S56;

#line 2100
                    count_1 = int(3);

#line 2100
                    l0_0 = _S74;

#line 2100
                    l1_0 = _S75;

#line 2100
                    l2_0 = _S57;

#line 2100
                    l3_0 = _S58;

#line 2100
                    l4_0 = _S59;

#line 2096
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S76 = float3(- _S60)  * _S56 + float3(_S61)  * _S55;
                        float3 _S77 = float3(- _S64)  * _S57 + float3(_S63)  * _S58;

#line 2106
                        count_1 = int(4);

#line 2106
                        l0_0 = _S76;

#line 2106
                        l1_0 = _S56;

#line 2106
                        l2_0 = _S57;

#line 2106
                        l3_0 = _S77;

#line 2106
                        l4_0 = _S59;

#line 2102
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2108
                            float3 _S78 = float3(- _S64) ;


                            float3 _S79 = _S78 * _S55 + float3(_S60)  * _S58;
                            float3 _S80 = _S78 * _S57 + float3(_S63)  * _S58;

#line 2112
                            count_1 = int(5);

#line 2112
                            l0_0 = _S55;

#line 2112
                            l1_0 = _S56;

#line 2112
                            l2_0 = _S57;

#line 2112
                            l3_0 = _S80;

#line 2112
                            l4_0 = _S79;

#line 2108
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2114
                                float3 _S81 = float3(_S64) ;


                                float3 _S82 = float3(- _S60)  * _S58 + _S81 * _S55;
                                float3 _S83 = float3(- _S63)  * _S58 + _S81 * _S57;

#line 2118
                                count_1 = int(3);

#line 2118
                                l0_0 = _S82;

#line 2118
                                l1_0 = _S83;

#line 2118
                                l2_0 = _S58;

#line 2118
                                l3_0 = _S58;

#line 2118
                                l4_0 = _S59;

#line 2114
                            }
                            else
                            {

#line 2121
                                if(config_2 == int(9))
                                {

                                    float3 _S84 = float3(- _S61)  * _S55 + float3(_S60)  * _S56;
                                    float3 _S85 = float3(- _S63)  * _S58 + float3(_S64)  * _S57;

#line 2125
                                    count_1 = int(4);

#line 2125
                                    l0_0 = _S55;

#line 2125
                                    l1_0 = _S84;

#line 2125
                                    l2_0 = _S85;

#line 2125
                                    l3_0 = _S58;

#line 2125
                                    l4_0 = _S59;

#line 2121
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S86 = float3(- _S64)  * _S57 + float3(_S63)  * _S58;
                                        float3 _S87 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;

#line 2132
                                        count_1 = int(5);

#line 2132
                                        l0_0 = _S55;

#line 2132
                                        l1_0 = _S56;

#line 2132
                                        l2_0 = _S87;

#line 2132
                                        l3_0 = _S86;

#line 2132
                                        l4_0 = _S58;

#line 2127
                                    }
                                    else
                                    {

#line 2134
                                        if(config_2 == int(12))
                                        {

                                            float3 _S88 = float3(- _S61)  * _S57 + float3(_S63)  * _S56;
                                            float3 _S89 = float3(- _S60)  * _S58 + float3(_S64)  * _S55;

#line 2138
                                            count_1 = int(4);

#line 2138
                                            l0_0 = _S89;

#line 2138
                                            l1_0 = _S88;

#line 2138
                                            l2_0 = _S57;

#line 2138
                                            l3_0 = _S58;

#line 2138
                                            l4_0 = _S59;

#line 2134
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S90 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;
                                                float3 _S91 = float3(- _S61)  * _S55 + float3(_S60)  * _S56;

#line 2146
                                                count_1 = int(5);

#line 2146
                                                l0_0 = _S55;

#line 2146
                                                l1_0 = _S91;

#line 2146
                                                l2_0 = _S90;

#line 2146
                                                l3_0 = _S57;

#line 2146
                                                l4_0 = _S58;

#line 2140
                                            }
                                            else
                                            {

#line 2148
                                                if(config_2 == int(14))
                                                {

#line 2148
                                                    float3 _S92 = float3(- _S60) ;


                                                    float3 _S93 = _S92 * _S58 + float3(_S64)  * _S55;
                                                    float3 _S94 = _S92 * _S56 + float3(_S61)  * _S55;

#line 2152
                                                    count_1 = int(5);

#line 2152
                                                    l0_0 = _S94;

#line 2152
                                                    l1_0 = _S93;

#line 2148
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2154
                                                        count_1 = int(4);

#line 2154
                                                    }
                                                    else
                                                    {

#line 2154
                                                        count_1 = int(0);

#line 2154
                                                    }

#line 2154
                                                    l0_0 = _S55;

#line 2154
                                                    l1_0 = _S59;

#line 2148
                                                }

#line 2069
                                                float3 _S95 = l1_0;

#line 2069
                                                l1_0 = _S56;

#line 2069
                                                l2_0 = _S57;

#line 2069
                                                l3_0 = _S58;

#line 2069
                                                l4_0 = _S95;

#line 2140
                                            }

#line 2134
                                        }

#line 2127
                                    }

#line 2121
                                }

#line 2114
                            }

#line 2108
                        }

#line 2102
                    }

#line 2096
                }

#line 2090
            }

#line 2084
        }

#line 2078
    }

#line 2162
    if(count_1 <= int(3))
    {

#line 2162
        l3_0 = l0_0;

#line 2162
        l4_0 = l0_0;

#line 2162
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2167
            l4_0 = l0_0;

#line 2167
        }

#line 2162
    }

#line 2172
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2035
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2041
    float weight_1;

#line 2046
    if(cosine_0 > 0.0f)
    {

#line 2046
        weight_1 = fit_0;

#line 2046
    }
    else
    {

#line 2046
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2046
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2192
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2194
    int corner_1 = int(0);
    for(;;)
    {

#line 2195
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2195
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2195
        corner_1 = corner_1 + int(1);

#line 2195
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2200
    thread LtcPolygon_0 _S96 = polygon_1;

#line 2200
    LtcPolygon_0 _S97 = ltc_clip_0(&_S96);
    polygon_1 = _S97;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2204
    int at_2 = int(0);

    for(;;)
    {

#line 2206
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2206
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2206
        at_2 = at_2 + int(1);

#line 2206
    }

#line 2213
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2213
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2214
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2214
    }
    else
    {

#line 2214
        sum_1 = sum_0;

#line 2214
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2218
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2218
    }

#line 2225
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 1921
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_8)
{
    int _S98 = tap_2->lo_0.x;

#line 1923
    int _S99 = tap_2->lo_0.y;

#line 1923
    int3 _S100 = int3(_S98, _S99, int(0));
    int _S101 = tap_2->hi_0.x;

#line 1924
    int3 _S102 = int3(_S101, _S99, int(0));
    float4 _S103 = float4(tap_2->weight_0.x) ;
    int _S104 = tap_2->hi_0.y;

#line 1926
    int3 _S105 = int3(_S98, _S104, int(0));
    int3 _S106 = int3(_S101, _S104, int(0));

    return mix(mix(((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S100)).xy), uint(((_S100)).z))), ((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S102)).xy), uint(((_S102)).z))), _S103), mix(((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S105)).xy), uint(((_S105)).z))), ((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), _S103), float4(tap_2->weight_0.y) );
}


#line 2008
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 1803
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 1810
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1817
    float _S107 = 1.0f - alpha2_0;

#line 1822
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S107 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S107 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2466
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 2781
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 320
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 2897
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_9)
{
    float2 texel_1 = kernelContext_9->frame_0->shadow_params_0.xy;
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S108 = float2(0.5f, 0.5f) * texel_1 * grid_0;


    float2 _S109 = float2(1.0f, 1.0f);

#line 2904
    float2 _S110 = _S109 / texel_1;

#line 2904
    uint index_1 = 0U;

#line 2904
    float sum_2 = 0.0f;

#line 2904
    float found_0 = 0.0f;



    for(;;)
    {

#line 2908
        if(index_1 < 16U)
        {
        }
        else
        {

#line 2908
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_1] * float2(8.0f) ;
        float _S111 = spoke_0.x;

#line 2911
        float _S112 = rotation_0.x;

#line 2911
        float _S113 = spoke_0.y;

#line 2911
        float _S114 = rotation_0.y;

#line 2920
        int3 _S115 = int3(int2(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + float2(_S111 * _S112 - _S113 * _S114, _S111 * _S114 + _S113 * _S112) * texel_1 * grid_0, _S108, float2(1.0f)  - _S108)) * _S110, _S110 - _S109)), int(0));

#line 2920
        float depth_1 = ((kernelContext_9->shadow_atlas_0).read(vec<uint,2>(((_S115)).xy), uint(((_S115)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 2924
            sum_2 = sum_2 + depth_1;

#line 2924
            found_0 = found_1;

#line 2921
        }

#line 2908
        index_1 = index_1 + 1U;

#line 2908
    }

#line 2928
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 2939
    float _S116 = 2.0f * kernelContext_9->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_2 / found_0 - reference_0) * (_S116 + 40.0f) * 0.01999999955296516f / (_S116 / 768.0f), 2.0f, 8.0f);
}


#line 2799
float tile_tap_0(uint tile_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_10)
{
    float2 texel_2 = kernelContext_10->frame_0->shadow_params_0.xy;

#line 2806
    float2 grid_1 = float2(4.0f, 4.0f);
    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_2 * grid_1;

    float _S117 = spoke_1.x;

#line 2809
    float _S118 = rotation_1.x;

#line 2809
    float _S119 = spoke_1.y;

#line 2809
    float _S120 = rotation_1.y;


    float _S121 = ((kernelContext_10->shadow_atlas_0).sample_compare((kernelContext_10->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + float2(_S117 * _S118 - _S119 * _S120, _S117 * _S120 + _S119 * _S118) * texel_2 * grid_1, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2812
    return _S121;
}


#line 2834
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_2, KernelContext_0 thread* kernelContext_11)
{
    float2 _S122 = shadow_rotation_0(pixel_2);

#line 2836
    uint spot_0 = 0U;

#line 2836
    float probe_0 = 0.0f;


    for(;;)
    {

#line 2839
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2839
            break;
        }

#line 2839
        float _S123 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S122, reference_2, kernelContext_11);

        float probe_1 = probe_0 + _S123;

#line 2839
        spot_0 = spot_0 + 1U;

#line 2839
        probe_0 = probe_1;

#line 2839
    }

#line 2848
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2854
    uint index_2 = 0U;

#line 2854
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2858
        if(index_2 < 32U)
        {
        }
        else
        {

#line 2858
            break;
        }

#line 2858
        float _S124 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S122, reference_2, kernelContext_11);

        float visibility_1 = visibility_0 + _S124;

#line 2858
        index_2 = index_2 + 1U;

#line 2858
        visibility_0 = visibility_1;

#line 2858
    }



    return visibility_0 / 32.0f;
}


#line 2993
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_12)
{

#line 3024
    float texel_world_0 = 2.0f * kernelContext_12->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 3031
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_12->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_12->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_12->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3035
    bool _S125;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3036
        _S125 = true;

#line 3036
    }
    else
    {

#line 3036
        _S125 = (ndc_0.z) <= 0.0f;

#line 3036
    }

#line 3036
    if(_S125)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3063
    float _S126 = ndc_0.z;

#line 3063
    float _S127 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S126, shadow_rotation_0(pixel_3), kernelContext_12);

#line 3063
    float _S128 = tile_pcf_0(cascade_1, tile_uv_4, _S126, pixel_3, _S127, kernelContext_12);
    return _S128;
}


#line 3080
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_13)
{

#line 3081
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3093
    float eye_distance_0 = length(world_position_5 - kernelContext_13->frame_0->camera_position_0.xyz);

#line 3093
    uint index_3 = 0U;

    for(;;)
    {

#line 3095
        if(index_3 < 2U)
        {
        }
        else
        {

#line 3095
            cascade_2 = 1U;

#line 3095
            break;
        }
        if(eye_distance_0 < kernelContext_13->frame_0->cascade_far_0[index_3])
        {

#line 3097
            cascade_2 = index_3;


            break;
        }

#line 3095
        index_3 = index_3 + 1U;

#line 3095
    }

#line 3095
    float _S129 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_13);

#line 3106
    uint _S130 = cascade_2 + 1U;

#line 3106
    if(_S130 >= 2U)
    {



        return _S129;
    }

#line 3119
    float band_0 = kernelContext_13->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_13->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S129;
    }

#line 3123
    float _S131 = cascade_visibility_0(_S130, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_13);

#line 3134
    return mix(_S129, _S131, blend_0);
}


#line 3324
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S132 = axis_2.x;

#line 3327
    float _S133 = axis_2.y;

#line 3327
    bool _S134;

#line 3327
    if(_S132 >= _S133)
    {

#line 3327
        _S134 = _S132 >= (axis_2.z);

#line 3327
    }
    else
    {

#line 3327
        _S134 = false;

#line 3327
    }

#line 3327
    uint _S135;

#line 3327
    if(_S134)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3329
            _S135 = 0U;

#line 3329
        }
        else
        {

#line 3329
            _S135 = 1U;

#line 3329
        }

#line 3329
        return _S135;
    }
    if(_S133 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3333
            _S135 = 2U;

#line 3333
        }
        else
        {

#line 3333
            _S135 = 3U;

#line 3333
        }

#line 3333
        return _S135;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3335
        _S135 = 4U;

#line 3335
    }
    else
    {

#line 3335
        _S135 = 5U;

#line 3335
    }

#line 3335
    return _S135;
}


#line 308
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 3237
float punctual_visibility_0(uint tile_4, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_14)
{

#line 3249
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_14->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 3256
    float _S136 = clip_1.w;

#line 3256
    if(_S136 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S136) ;

#line 3260
    bool _S137;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3261
        _S137 = true;

#line 3261
    }
    else
    {

#line 3261
        _S137 = (ndc_1.z) <= 0.0f;

#line 3261
    }

#line 3261
    if(_S137)
    {

#line 3261
        _S137 = true;

#line 3261
    }
    else
    {

#line 3261
        _S137 = (ndc_1.z) > 1.0f;

#line 3261
    }

#line 3261
    if(_S137)
    {

#line 3268
        return 1.0f;
    }

#line 3268
    float _S138 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_14);

#line 3278
    return _S138;
}


#line 3343
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_15)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3351
    float _S139 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_15);

#line 3357
    return _S139;
}


#line 3285
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_5, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_16)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3292
    float4 _S140 = float4(light_2->direction_0) ;

#line 3299
    float cos_outer_1 = _S140.w;

#line 3299
    float _S141 = punctual_visibility_0(tile_5, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S140.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_16);

#line 3306
    return _S141;
}


#line 1949
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 3550
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_17)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_17->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_17->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_17->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 959
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 3452
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_18)
{

    GpuProbe_natural_0 _S142 = kernelContext_18->probes_0[min((cell_1.z * kernelContext_18->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_18->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_18->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 3455
    GpuProbe_0 _S143 = { float4(_S142.sh_r_0) , float4(_S142.sh_g_0) , float4(_S142.sh_b_0)  };

#line 3455
    return _S143;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_1, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S144 = float4(t_1) ;

#line 3463
    (&blended_0)->sh_r_0 = mix(a_1->sh_r_0, b_0->sh_r_0, _S144);
    (&blended_0)->sh_g_0 = mix(a_1->sh_g_0, b_0->sh_g_0, _S144);
    (&blended_0)->sh_b_0 = mix(a_1->sh_b_0, b_0->sh_b_0, _S144);
    return blended_0;
}


#line 3503
float3 probe_irradiance_0(float3 world_position_9, float3 normal_7, KernelContext_0 thread* kernelContext_19)
{

#line 3503
    float3 _S145 = float3(1.0f) ;

#line 3508
    float3 _S146 = float3(0.0f, 0.0f, 0.0f);

#line 3508
    float3 last_0 = max(float3(kernelContext_19->frame_0->probe_counts_0.xyz) - _S145, _S146);
    float3 grid_2 = clamp((world_position_9 - kernelContext_19->frame_0->probe_origin_0.xyz) * kernelContext_19->frame_0->probe_inv_spacing_0.xyz, _S146, last_0);

    float3 base_2 = floor(grid_2);
    float3 f_0 = grid_2 - base_2;

    uint3 _S147 = uint3(base_2);



    uint3 _S148 = uint3(min(base_2 + _S145, last_0));

#line 3525
    uint _S149 = _S147.x;

#line 3525
    uint _S150 = _S147.y;

#line 3525
    uint _S151 = _S147.z;

#line 3525
    GpuProbe_0 _S152 = probe_at_0(uint3(_S149, _S150, _S151), kernelContext_19);

#line 3525
    uint _S153 = _S148.x;

#line 3525
    GpuProbe_0 _S154 = probe_at_0(uint3(_S153, _S150, _S151), kernelContext_19);

#line 3525
    float _S155 = f_0.x;

#line 3525
    thread GpuProbe_0 _S156 = _S152;

#line 3525
    thread GpuProbe_0 _S157 = _S154;

#line 3525
    GpuProbe_0 _S158 = lerp_probe_0(&_S156, &_S157, _S155);
    uint _S159 = _S148.y;

#line 3526
    GpuProbe_0 _S160 = probe_at_0(uint3(_S149, _S159, _S151), kernelContext_19);

#line 3526
    GpuProbe_0 _S161 = probe_at_0(uint3(_S153, _S159, _S151), kernelContext_19);

#line 3526
    thread GpuProbe_0 _S162 = _S160;

#line 3526
    thread GpuProbe_0 _S163 = _S161;

#line 3526
    GpuProbe_0 _S164 = lerp_probe_0(&_S162, &_S163, _S155);
    uint _S165 = _S148.z;

#line 3527
    GpuProbe_0 _S166 = probe_at_0(uint3(_S149, _S150, _S165), kernelContext_19);

#line 3527
    GpuProbe_0 _S167 = probe_at_0(uint3(_S153, _S150, _S165), kernelContext_19);

#line 3527
    thread GpuProbe_0 _S168 = _S166;

#line 3527
    thread GpuProbe_0 _S169 = _S167;

#line 3527
    GpuProbe_0 _S170 = lerp_probe_0(&_S168, &_S169, _S155);

#line 3527
    GpuProbe_0 _S171 = probe_at_0(uint3(_S149, _S159, _S165), kernelContext_19);

#line 3527
    GpuProbe_0 _S172 = probe_at_0(uint3(_S153, _S159, _S165), kernelContext_19);

#line 3527
    thread GpuProbe_0 _S173 = _S171;

#line 3527
    thread GpuProbe_0 _S174 = _S172;

#line 3527
    GpuProbe_0 _S175 = lerp_probe_0(&_S173, &_S174, _S155);

    float _S176 = f_0.y;

#line 3529
    thread GpuProbe_0 _S177 = _S158;

#line 3529
    thread GpuProbe_0 _S178 = _S164;

#line 3529
    GpuProbe_0 _S179 = lerp_probe_0(&_S177, &_S178, _S176);

#line 3529
    thread GpuProbe_0 _S180 = _S170;

#line 3529
    thread GpuProbe_0 _S181 = _S175;

#line 3529
    GpuProbe_0 _S182 = lerp_probe_0(&_S180, &_S181, _S176);

    float _S183 = f_0.z;

#line 3531
    thread GpuProbe_0 _S184 = _S179;

#line 3531
    thread GpuProbe_0 _S185 = _S182;

#line 3531
    GpuProbe_0 _S186 = lerp_probe_0(&_S184, &_S185, _S183);

    float4 basis_7 = float4(normal_7, 1.0f);
    return max(float3(dot(_S186.sh_r_0, basis_7), dot(_S186.sh_g_0, basis_7), dot(_S186.sh_b_0, basis_7)), _S146);
}


#line 932
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2300
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S187 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2308
    float kernel_0 = 0.0001984127011383f;

#line 2308
    int term_0 = int(6);

    for(;;)
    {

#line 2310
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2310
            break;
        }
        float _S188 = kernel_0 * _S187 + FOG_KERNEL_0[term_0];

#line 2310
        int term_1 = term_0 - int(1);

#line 2310
        kernel_0 = _S188;

#line 2310
        term_0 = term_1;

#line 2310
    }

#line 2317
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2327
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S189 = - d_0;

#line 2331
        float series_0 = 0.00833333376795053f;

#line 2331
        int term_2 = int(3);

        for(;;)
        {

#line 2333
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2333
                break;
            }
            float _S190 = series_0 * _S189 + FOG_RATIO_KERNEL_0[term_2];

#line 2333
            int term_3 = term_2 - int(1);

#line 2333
            series_0 = _S190;

#line 2333
            term_2 = term_3;

#line 2333
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2361
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2372
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2380
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 3576
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 3576
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


#line 3915
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S191 [[stage_in]], float4 position_4 [[position]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 3915
    thread KernelContext_0 kernelContext_20;

#line 3915
    (&kernelContext_20)->draw_0 = draw_2;

#line 3915
    (&kernelContext_20)->visible_instances_0 = visible_instances_2;

#line 3915
    (&kernelContext_20)->instances_0 = instances_2;

#line 3915
    (&kernelContext_20)->meshes_0 = meshes_2;

#line 3915
    (&kernelContext_20)->frame_0 = frame_4;

#line 3915
    (&kernelContext_20)->vertices_0 = vertices_2;

#line 3915
    (&kernelContext_20)->ambient_occlusion_0 = ambient_occlusion_2;

#line 3915
    (&kernelContext_20)->materials_0 = materials_2;

#line 3915
    (&kernelContext_20)->normal_textures_0 = normal_textures_2;

#line 3915
    (&kernelContext_20)->base_color_sampler_0 = base_color_sampler_2;

#line 3915
    (&kernelContext_20)->base_color_textures_0 = base_color_textures_2;

#line 3915
    (&kernelContext_20)->cluster_lights_0 = cluster_lights_2;

#line 3915
    (&kernelContext_20)->specular_dfg_0 = specular_dfg_2;

#line 3915
    (&kernelContext_20)->lights_0 = lights_2;

#line 3915
    (&kernelContext_20)->ltc_matrix_0 = ltc_matrix_2;

#line 3915
    (&kernelContext_20)->shadow_atlas_0 = shadow_atlas_2;

#line 3915
    (&kernelContext_20)->shadow_sampler_0 = shadow_sampler_2;

#line 3915
    (&kernelContext_20)->probes_0 = probes_2;

#line 3927
    float3 vertex_normal_0 = normalize(_S191.world_normal_1);

#line 3932
    float2 motion_1 = motion_vector_0(_S191.clip_position_1, _S191.previous_clip_position_1);

#line 3941
    if((frame_4->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 3983
    if((frame_4->ambient_0.w) >= 3.5f)
    {

#line 3983
        float _S192 = occlusion_at_0(position_4.xy, &kernelContext_20);

        thread FragmentOutput_0 occlusion_0;

#line 3994
        (&occlusion_0)->lit_0 = float4(_S192, _S192, _S192, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_4->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S191.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 4011
    thread GpuMaterial_natural_0 _S193 = (&kernelContext_20)->materials_0[_S191.material_5];

#line 4011
    float2 uv_3;

#line 4036
    if(((&_S193)->tiling_0) == 1U)
    {

#line 4036
        uv_3 = physical_tile_uv_0(_S191.world_position_10, vertex_normal_0, (&_S193)->tile_metres_0);

#line 4036
    }
    else
    {

#line 4036
        uv_3 = _S191.uv_2;

#line 4036
    }

#line 4036
    uint _S194 = normal_layer_0(&_S193);

#line 4036
    thread VertexOutput_0 _S195;

#line 4036
    (&_S195)->position_3 = position_4;

#line 4036
    (&_S195)->world_position_1 = _S191.world_position_10;

#line 4036
    (&_S195)->world_normal_0 = _S191.world_normal_1;

#line 4036
    (&_S195)->color_2 = _S191.color_3;

#line 4036
    (&_S195)->material_2 = _S191.material_5;

#line 4036
    (&_S195)->uv_0 = _S191.uv_2;

#line 4036
    (&_S195)->clip_position_0 = _S191.clip_position_1;

#line 4036
    (&_S195)->previous_clip_position_0 = _S191.previous_clip_position_1;

#line 4036
    (&_S195)->world_tangent_0 = _S191.world_tangent_1;

#line 4036
    (&_S195)->frame_2 = _S191.frame_3;

#line 4036
    float3 _S196 = shading_normal_of_0(_S194, (&_S193)->normal_scale_0, &_S195, vertex_normal_0, uv_3, &kernelContext_20);

#line 4043
    if((frame_4->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 4045
        float3 _S197 = float3(0.5f) ;

#line 4057
        (&normals_0)->lit_0 = float4(_S196 * _S197 + _S197, 1.0f);

#line 4063
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_20)->frame_0->camera_position_0.xyz - _S191.world_position_10);



    float3 _S198 = geometric_normal_of_0(_S191.world_position_10, vertex_normal_0);

#line 4072
    uint _S199 = base_color_layer_0(&_S193);

#line 4087
    float3 _S200 = float3(uv_3, float(_S199));
    float4 albedo_0 = _S191.color_3 * float4((&_S193)->base_color_0)  * (((&kernelContext_20)->base_color_textures_0).sample(((&kernelContext_20)->base_color_sampler_0), ((_S200)).xy, uint(((_S200)).z)));

#line 4094
    float metallic_1 = saturate((&_S193)->metallic_0);
    float roughness_2 = clamp((&_S193)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S201 = alpha_0 * alpha_0;

#line 4103
    float3 _S202 = albedo_0.xyz;

#line 4103
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S202, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S202 * float3((1.0f - metallic_1)) ;

#line 4110
    float _S203 = max(dot(_S196, to_eye_1), 0.00009999999747379f);

#line 4120
    float2 _S204 = position_4.xy;

#line 4120
    uint _S205 = froxel_of_0(_S204, (((float4(_S191.world_position_10, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_20);

#line 4120
    uint base_3 = _S205 * 17U;

#line 4125
    uint _S206 = min((&kernelContext_20)->cluster_lights_0[base_3], 16U);

#line 4125
    TableTap_0 _S207 = table_tap_0(_S203, roughness_2, &kernelContext_20);

#line 4125
    thread TableTap_0 _S208 = _S207;

#line 4125
    float2 _S209 = dfg_at_0(&_S208, &kernelContext_20);

#line 4134
    float _S210 = _S209.x;

#line 4134
    float _S211 = _S209.y;

#line 4134
    float3 _S212 = f0_2 * float3(_S210)  + float3(_S211) ;

#line 4140
    float3 _S213 = float3(0.0f, 0.0f, 0.0f);

#line 4140
    uint slot_0 = 0U;

#line 4140
    float3 direct_0 = _S213;

#line 4140
    float3 gloss_0 = _S213;

    for(;;)
    {

#line 4142
        if(slot_0 < _S206)
        {
        }
        else
        {

#line 4142
            break;
        }

#line 4142
        thread GpuLight_natural_0 _S214 = (&kernelContext_20)->lights_0[(&kernelContext_20)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 4142
        uint _S215 = (&_S214)->kind_0;

#line 4151
        bool _S216 = ((&_S214)->kind_0) == 0U;

#line 4151
        float3 to_light_7;

#line 4151
        float reach_0;

#line 4151
        if(_S216)
        {

#line 4151
            to_light_7 = normalize((float4((&_S214)->direction_0) ).xyz);

#line 4151
            reach_0 = 1.0f;

#line 4151
        }
        else
        {


            if(_S215 == 3U)
            {

#line 4156
                float4 _S217 = float4((&_S214)->position_0) ;

#line 4164
                float3 offset_0 = _S217.xyz - _S191.world_position_10;
                float distance_3 = length(offset_0);

                float _S218 = range_window_0(distance_3, _S217.w);

#line 4167
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 4167
                reach_0 = _S218;

#line 4156
            }
            else
            {

#line 4156
                float4 _S219 = float4((&_S214)->position_0) ;

#line 4171
                float3 offset_1 = _S219.xyz - _S191.world_position_10;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_1 = punctual_falloff_0(distance_4, _S219.w);
                if(_S215 == 2U)
                {

#line 4175
                    float4 _S220 = float4((&_S214)->direction_0) ;

#line 4175
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S220.xyz, _S220.w, (&_S214)->cos_inner_0);

#line 4175
                }
                else
                {

#line 4175
                    reach_0 = reach_1;

#line 4175
                }

#line 4175
                to_light_7 = to_light_8;

#line 4156
            }

#line 4151
        }

#line 4184
        float n_dot_l_5 = dot(_S196, to_light_7);

#line 4184
        float3 specular_0;

#line 4184
        float diffuse_0;


        if(_S215 == 3U)
        {

#line 4197
            thread array<float3, int(4)> corners_2;

#line 4197
            rect_corners_0(&_S214, _S191.world_position_10, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S196, to_eye_1, _S203);

#line 4199
            thread array<float3, int(4)> _S221 = corners_2;

#line 4199
            float _S222 = ltc_irradiance_0(to_local_0, &_S221);

#line 4199
            thread TableTap_0 _S223 = _S207;

#line 4199
            float4 _S224 = ltc_at_0(&_S223, &kernelContext_20);

            matrix<float,int(3),int(3)>  _S225 = (((to_local_0) * (ltc_transform_0(_S224))));

#line 4201
            thread array<float3, int(4)> _S226 = corners_2;

#line 4201
            float _S227 = ltc_irradiance_0(_S225, &_S226);
            float3 _S228 = float3(_S227)  * _S212;

#line 4202
            diffuse_0 = _S222;

#line 4202
            specular_0 = _S228;

#line 4187
        }
        else
        {

#line 4207
            float _S229 = max(n_dot_l_5, 0.0f);

#line 4214
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 4222
            float3 specular_1 = ggx_lobe_0(_S201, f0_2, _S229, _S203, max(dot(_S196, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S229) ;

#line 4222
            diffuse_0 = _S229;

#line 4222
            specular_0 = specular_1;

#line 4187
        }

#line 4187
        float3 specular_2;

#line 4230
        if((((&_S214)->flags_3) & 1U) != 0U)
        {

#line 4230
            specular_2 = _S213;

#line 4230
        }
        else
        {

#line 4230
            specular_2 = specular_0;

#line 4230
        }

#line 4230
        float reach_2;

#line 4248
        if(_S216)
        {

#line 4248
            float _S230 = sun_visibility_0(_S191.world_position_10, to_light_7, n_dot_l_5, _S198, _S204, &kernelContext_20);

#line 4248
            reach_2 = _S230;

#line 4248
        }
        else
        {


            if(_S215 == 1U)
            {

#line 4253
                uint _S231 = (&_S214)->shadow_tile_0;

#line 4265
                if(((&_S214)->shadow_tile_0) <= 8U)
                {

#line 4265
                    float _S232 = point_visibility_0(&_S214, _S231, _S191.world_position_10, to_light_7, n_dot_l_5, _S198, _S204, &kernelContext_20);

#line 4265
                    reach_2 = reach_0 * _S232;

#line 4265
                }
                else
                {

#line 4265
                    reach_2 = reach_0;

#line 4265
                }

#line 4253
            }
            else
            {

#line 4253
                uint _S233 = (&_S214)->shadow_tile_0;

#line 4271
                if(((&_S214)->shadow_tile_0) < 14U)
                {

#line 4271
                    float _S234 = spot_visibility_0(&_S214, _S233, _S191.world_position_10, to_light_7, n_dot_l_5, _S198, _S204, &kernelContext_20);

#line 4271
                    reach_2 = reach_0 * _S234;

#line 4271
                }
                else
                {

#line 4271
                    reach_2 = reach_0;

#line 4271
                }

#line 4253
            }

#line 4248
        }

#line 4279
        float3 _S235 = (float4((&_S214)->color_0) ).xyz;

#line 4279
        float3 direct_1 = direct_0 + _S235 * float3((diffuse_0 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S235 * (specular_2 * float3(reach_2) );

#line 4142
        slot_0 = slot_0 + 1U;

#line 4142
        direct_0 = direct_1;

#line 4142
        gloss_0 = gloss_1;

#line 4142
    }

#line 4294
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S210 + _S211);

#line 4294
    float _S236 = occlusion_at_0(_S204, &kernelContext_20);

#line 4330
    float3 _S237 = frame_4->ambient_0.xyz;

#line 4330
    float3 _S238 = sky_irradiance_0(_S196, &kernelContext_20);

#line 4330
    float3 _S239 = _S237 + _S238;

#line 4330
    float3 _S240 = probe_irradiance_0(_S191.world_position_10, _S196, &kernelContext_20);

#line 4351
    float3 lit_1 = diffuse_albedo_0 * ((_S239 + _S240) * float3(_S236)  + direct_0) + gloss_2;

#line 4351
    float3 _S241 = emissive_of_0(&_S193);

#line 4387
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_20)->frame_0->fog_params_0.x, (&kernelContext_20)->frame_0->fog_params_0.y, (&kernelContext_20)->frame_0->camera_position_0.y - (&kernelContext_20)->frame_0->fog_params_0.z, _S191.world_position_10.y - (&kernelContext_20)->frame_0->fog_params_0.z, length((&kernelContext_20)->frame_0->camera_position_0.xyz - _S191.world_position_10)));


    thread FragmentOutput_0 output_1;



    (&output_1)->lit_0 = float4((lit_1 + _S241) * float3(fog_survives_0)  + (&kernelContext_20)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_1)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_1)->motion_0 = motion_1;
    return output_1;
}


#line 4400
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


#line 4400
[[vertex]] vertexMain_Result_0 vertexMain(uint index_4 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_6 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]])
{

#line 4400
    thread KernelContext_0 kernelContext_21;

#line 4400
    (&kernelContext_21)->draw_0 = draw_3;

#line 4400
    (&kernelContext_21)->visible_instances_0 = visible_instances_3;

#line 4400
    (&kernelContext_21)->instances_0 = instances_3;

#line 4400
    (&kernelContext_21)->meshes_0 = meshes_3;

#line 4400
    (&kernelContext_21)->frame_0 = frame_6;

#line 4400
    (&kernelContext_21)->vertices_0 = vertices_3;

#line 4400
    (&kernelContext_21)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4400
    (&kernelContext_21)->materials_0 = materials_3;

#line 4400
    (&kernelContext_21)->normal_textures_0 = normal_textures_3;

#line 4400
    (&kernelContext_21)->base_color_sampler_0 = base_color_sampler_3;

#line 4400
    (&kernelContext_21)->base_color_textures_0 = base_color_textures_3;

#line 4400
    (&kernelContext_21)->cluster_lights_0 = cluster_lights_3;

#line 4400
    (&kernelContext_21)->specular_dfg_0 = specular_dfg_3;

#line 4400
    (&kernelContext_21)->lights_0 = lights_3;

#line 4400
    (&kernelContext_21)->ltc_matrix_0 = ltc_matrix_3;

#line 4400
    (&kernelContext_21)->shadow_atlas_0 = shadow_atlas_3;

#line 4400
    (&kernelContext_21)->shadow_sampler_0 = shadow_sampler_3;

#line 4400
    (&kernelContext_21)->probes_0 = probes_3;

#line 4400
    GpuInstance_natural_0 device* _S242 = instances_3+visible_instances_3[draw_3->base_0 + instance_id_1];

#line 1617
    GpuMesh_0 mesh_3 = meshes_3[draw_3->mesh_0];

#line 1625
    bool _S243 = ((_S242->flags_0) & 2U) != 0U;

#line 1625
    uint base_vertex_3;
    if(_S243)
    {

#line 1626
        base_vertex_3 = _S242->base_vertex_0;

#line 1626
    }
    else
    {

#line 1626
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1626
    }

#line 1626
    MeshVertex_0 _S244 = load_vertex_0(index_4 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_21);

#line 1626
    uint previous_base_0;

#line 1639
    if(_S243)
    {

#line 1639
        previous_base_0 = _S242->previous_base_vertex_0;

#line 1639
    }
    else
    {

#line 1639
        previous_base_0 = base_vertex_3;

#line 1639
    }

#line 1639
    float3 _S245 = load_position_0(index_4 + previous_base_0, &kernelContext_21);

#line 1639
    matrix<float,int(4),int(4)>  _S246 = matrix<float,int(4),int(4)> (_S242->transform_0.data_0[int(0)][int(0)], _S242->transform_0.data_0[int(1)][int(0)], _S242->transform_0.data_0[int(2)][int(0)], _S242->transform_0.data_0[int(3)][int(0)], _S242->transform_0.data_0[int(0)][int(1)], _S242->transform_0.data_0[int(1)][int(1)], _S242->transform_0.data_0[int(2)][int(1)], _S242->transform_0.data_0[int(3)][int(1)], _S242->transform_0.data_0[int(0)][int(2)], _S242->transform_0.data_0[int(1)][int(2)], _S242->transform_0.data_0[int(2)][int(2)], _S242->transform_0.data_0[int(3)][int(2)], _S242->transform_0.data_0[int(0)][int(3)], _S242->transform_0.data_0[int(1)][int(3)], _S242->transform_0.data_0[int(2)][int(3)], _S242->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S244.position_1, 1.0f)) * (_S246)));

    thread VertexOutput_0 output_2;
    (&output_2)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_21)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_21)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_2)->world_position_1 = world_0.xyz;

#line 1653
    matrix<float,int(3),int(3)>  _S247 = matrix<float,int(3),int(3)> (_S246[int(0)].xyz, _S246[int(1)].xyz, _S246[int(2)].xyz);

#line 1653
    (&output_2)->world_normal_0 = (((_S244.basis_1.normal_0) * (normal_basis_0(_S247))));

#line 1659
    (&output_2)->world_tangent_0 = (((_S244.basis_1.tangent_1) * (_S247)));

#line 1659
    thread TangentFrame_0 _S248 = _S244.basis_1;

#line 1659
    uint _S249 = frame_word_0(mesh_3.flags_1, &_S248);
    (&output_2)->frame_2 = _S249;

#line 1660
    float4 _S250;

#line 1667
    if(((&kernelContext_21)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1667
        _S250 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1667
    }
    else
    {

#line 1667
        _S250 = _S244.color_1;

#line 1667
    }

#line 1666
    (&output_2)->color_2 = _S250;

#line 1673
    (&output_2)->material_2 = _S242->material_0;
    (&output_2)->uv_0 = _S244.uv0_0;

#line 1680
    (&output_2)->clip_position_0 = (&output_2)->position_3;
    (&output_2)->previous_clip_position_0 = ((((((float4(_S245, 1.0f)) * (matrix<float,int(4),int(4)> (_S242->previous_transform_0.data_0[int(0)][int(0)], _S242->previous_transform_0.data_0[int(1)][int(0)], _S242->previous_transform_0.data_0[int(2)][int(0)], _S242->previous_transform_0.data_0[int(3)][int(0)], _S242->previous_transform_0.data_0[int(0)][int(1)], _S242->previous_transform_0.data_0[int(1)][int(1)], _S242->previous_transform_0.data_0[int(2)][int(1)], _S242->previous_transform_0.data_0[int(3)][int(1)], _S242->previous_transform_0.data_0[int(0)][int(2)], _S242->previous_transform_0.data_0[int(1)][int(2)], _S242->previous_transform_0.data_0[int(2)][int(2)], _S242->previous_transform_0.data_0[int(3)][int(2)], _S242->previous_transform_0.data_0[int(0)][int(3)], _S242->previous_transform_0.data_0[int(1)][int(3)], _S242->previous_transform_0.data_0[int(2)][int(3)], _S242->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_21)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S251 = output_2;

#line 1684
    thread vertexMain_Result_0 _S252;

#line 1684
    (&_S252)->position_5 = _S251.position_3;

#line 1684
    (&_S252)->world_position_11 = _S251.world_position_1;

#line 1684
    (&_S252)->world_normal_2 = _S251.world_normal_0;

#line 1684
    (&_S252)->color_4 = _S251.color_2;

#line 1684
    (&_S252)->material_6 = _S251.material_2;

#line 1684
    (&_S252)->uv_4 = _S251.uv_0;

#line 1684
    (&_S252)->clip_position_2 = _S251.clip_position_0;

#line 1684
    (&_S252)->previous_clip_position_2 = _S251.previous_clip_position_0;

#line 1684
    (&_S252)->world_tangent_2 = _S251.world_tangent_0;

#line 1684
    (&_S252)->frame_5 = _S251.frame_2;

#line 1684
    return _S252;
}

