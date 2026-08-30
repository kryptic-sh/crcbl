#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2302 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2297
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2569
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2629
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2781
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2644
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2672
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1090
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1633
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1633
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


#line 759
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


#line 1639
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1639
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 338 "shaders/mesh.slang"
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


#line 338
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


#line 338
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


#line 338
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 338
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


#line 1133
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


#line 1144
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1147
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1497
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1620
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1620
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1622
        word_4 = 1U;

#line 1622
    }
    else
    {

#line 1622
        word_4 = 0U;

#line 1622
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_1), basis_3->bitangent_0)) < 0.0f)
    {

#line 1626
        word_4 = word_4 | 2U;

#line 1626
    }

#line 1625
    return word_4;
}


#line 1625
struct vertexOutput_0
{
    float4 output_0 [[position]];
};


#line 1740
[[vertex]] vertexOutput_0 depthVertexMain(uint index_0 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 1740
    thread KernelContext_0 kernelContext_2;

#line 1740
    (&kernelContext_2)->draw_0 = draw_1;

#line 1740
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 1740
    (&kernelContext_2)->instances_0 = instances_1;

#line 1740
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 1740
    (&kernelContext_2)->frame_0 = frame_1;

#line 1740
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 1740
    (&kernelContext_2)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1740
    (&kernelContext_2)->materials_0 = materials_1;

#line 1740
    (&kernelContext_2)->normal_textures_0 = normal_textures_1;

#line 1740
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 1740
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 1740
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 1740
    (&kernelContext_2)->specular_dfg_0 = specular_dfg_1;

#line 1740
    (&kernelContext_2)->lights_0 = lights_1;

#line 1740
    (&kernelContext_2)->ltc_matrix_0 = ltc_matrix_1;

#line 1740
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 1740
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;

#line 1740
    (&kernelContext_2)->probes_0 = probes_1;

#line 1740
    GpuInstance_natural_0 device* _S7 = instances_1+visible_instances_1[draw_1->base_0 + instance_id_0];


    GpuMesh_0 mesh_2 = meshes_1[draw_1->mesh_0];

#line 1743
    uint base_vertex_2;

#line 1749
    if(((_S7->flags_0) & 2U) != 0U)
    {

#line 1749
        base_vertex_2 = _S7->base_vertex_0;

#line 1749
    }
    else
    {

#line 1749
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1749
    }

#line 1749
    matrix<float,int(4),int(4)>  _S8 = matrix<float,int(4),int(4)> (_S7->transform_0.data_0[int(0)][int(0)], _S7->transform_0.data_0[int(1)][int(0)], _S7->transform_0.data_0[int(2)][int(0)], _S7->transform_0.data_0[int(3)][int(0)], _S7->transform_0.data_0[int(0)][int(1)], _S7->transform_0.data_0[int(1)][int(1)], _S7->transform_0.data_0[int(2)][int(1)], _S7->transform_0.data_0[int(3)][int(1)], _S7->transform_0.data_0[int(0)][int(2)], _S7->transform_0.data_0[int(1)][int(2)], _S7->transform_0.data_0[int(2)][int(2)], _S7->transform_0.data_0[int(3)][int(2)], _S7->transform_0.data_0[int(0)][int(3)], _S7->transform_0.data_0[int(1)][int(3)], _S7->transform_0.data_0[int(2)][int(3)], _S7->transform_0.data_0[int(3)][int(3)]);

#line 1749
    float3 _S9 = load_position_0(index_0 + base_vertex_2, &kernelContext_2);

#line 1749
    vertexOutput_0 _S10 = { ((((((float4(_S9, 1.0f)) * (_S8)))) * (matrix<float,int(4),int(4)> ((&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_2)->frame_0->view_proj_0.data_1[int(3)][int(3)])))) };


    return _S10;
}


#line 3954
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S11 = previous_0.w;

#line 3956
    if(_S11 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S11) ) * float2(0.5f, -0.5f);
}


#line 3922
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_3)
{

#line 3922
    texture2d<float, access::sample> _S12 = kernelContext_3->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S12).get_width(0)),(*((&height_0)) = (_S12).get_height(0));

    int3 _S13 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 3928
    return ((kernelContext_3->ambient_occlusion_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z)).x);
}


#line 3672
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S14 = axis_0.x;

#line 3676
    float _S15 = axis_0.y;

#line 3676
    bool _S16;

#line 3676
    if(_S14 >= _S15)
    {

#line 3676
        _S16 = _S14 >= (axis_0.z);

#line 3676
    }
    else
    {

#line 3676
        _S16 = false;

#line 3676
    }

#line 3676
    float2 planar_0;

#line 3676
    if(_S16)
    {

#line 3676
        planar_0 = world_position_0.zy;

#line 3676
    }
    else
    {

        if(_S15 >= (axis_0.z))
        {

#line 3680
            planar_0 = world_position_0.xz;

#line 3680
        }
        else
        {

#line 3680
            planar_0 = world_position_0.xy;

#line 3680
        }

#line 3676
    }

#line 3688
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 944
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 3709
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S17 = normal_2.z;

#line 3711
    float sign_z_0;

#line 3711
    if(_S17 >= 0.0f)
    {

#line 3711
        sign_z_0 = 1.0f;

#line 3711
    }
    else
    {

#line 3711
        sign_z_0 = -1.0f;

#line 3711
    }
    float a_0 = -1.0f / (sign_z_0 + _S17);
    float _S18 = normal_2.x;

#line 3713
    float _S19 = sign_z_0 * _S18;

#line 3713
    return float3(1.0f + _S19 * _S18 * a_0, _S19 * normal_2.y * a_0, - sign_z_0 * _S18);
}


#line 3763
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S20 = duvdy_0.y;

#line 3765
    float _S21 = duvdx_0.y;

#line 3765
    float winding_0;
    if((duvdx_0.x * _S20 - duvdy_0.x * _S21) < 0.0f)
    {

#line 3766
        winding_0 = -1.0f;

#line 3766
    }
    else
    {

#line 3766
        winding_0 = 1.0f;

#line 3766
    }
    float3 tangent_2 = (float3(_S20)  * dpdx_0 - float3(_S21)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 3775
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 3776
    float3 _S22;

#line 3785
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 3785
        _S22 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 3785
    }
    else
    {

#line 3785
        _S22 = orthonormal_tangent_0(normal_3);

#line 3785
    }

#line 3785
    (&basis_4)->tangent_1 = _S22;

    (&basis_4)->bitangent_0 = cross(normal_3, _S22);
    return basis_4;
}


#line 1504
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


#line 3845
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_4)
{

#line 3857
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 3867
    uint _S23 = input_0->frame_2;
    if(((input_0->frame_2) & 1U) != 0U)
    {

#line 3876
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 3878
        float3 _S24;

#line 3883
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 3883
            _S24 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 3883
        }
        else
        {

#line 3883
            _S24 = orthonormal_tangent_0(normal_4);

#line 3883
        }

#line 3883
        (&basis_5)->tangent_1 = _S24;

#line 3889
        float3 _S25 = cross((&basis_5)->normal_0, _S24);

#line 3889
        float _S26;
        if((_S23 & 2U) != 0U)
        {

#line 3890
            _S26 = -1.0f;

#line 3890
        }
        else
        {

#line 3890
            _S26 = 1.0f;

#line 3890
        }

#line 3889
        (&basis_5)->bitangent_0 = _S25 * float3(_S26) ;

#line 3868
    }
    else
    {

#line 3894
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 3868
    }

#line 3898
    float3 _S27 = float3(uv_1, float(layer_0));
    float3 _S28 = ((kernelContext_4->normal_textures_0).sample((kernelContext_4->base_color_sampler_0), ((_S27)).xy, uint(((_S27)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 3899
    thread float3 tangent_space_0 = _S28;
    tangent_space_0.xy = _S28.xy * float2(normal_scale_1) ;

#line 3905
    float3 _S29 = normalize(tangent_space_0);

#line 3905
    tangent_space_0 = _S29;
    return normalize(float3(_S29.x)  * (&basis_5)->tangent_1 + float3(_S29.y)  * (&basis_5)->bitangent_0 + float3(_S29.z)  * (&basis_5)->normal_0);
}


#line 2437
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2448
    float3 _S30;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2449
        _S30 = - facet_1;

#line 2449
    }
    else
    {

#line 2449
        _S30 = facet_1;

#line 2449
    }

#line 2449
    return _S30;
}


#line 929
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3470
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_5)
{
    uint _S31 = max(kernelContext_5->frame_0->cluster_grid_0.x, 1U);
    uint _S32 = max(kernelContext_5->frame_0->cluster_grid_0.y, 1U);
    uint _S33 = max(kernelContext_5->frame_0->cluster_grid_0.z, 1U);
    uint _S34 = max(kernelContext_5->frame_0->cluster_grid_0.w, 1U);

#line 3480
    uint _S35 = uint(pixel_0.x) / _S34;

#line 3480
    uint _S36 = min(_S35, _S31 - 1U);
    uint _S37 = uint(pixel_0.y) / _S34;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3491
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S33 - 1U))) * _S32 + min(_S37, _S32 - 1U)) * _S31 + _S36;
}


#line 1869
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 1890
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_6)
{

#line 1890
    texture2d<float, access::sample> _S38 = kernelContext_6->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S38).get_width(0)),(*((&height_1)) = (_S38).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1896
    float2 _S39 = float2(1.0f) ;
    float2 _S40 = extent_1 - _S39;

#line 1897
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S40);
    float2 high_1 = min(low_1 + _S39, _S40);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 1915
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 1927
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_7)
{
    int _S41 = tap_1->lo_0.x;

#line 1929
    int _S42 = tap_1->lo_0.y;

#line 1929
    int3 _S43 = int3(_S41, _S42, int(0));
    int _S44 = tap_1->hi_0.x;

#line 1930
    int3 _S45 = int3(_S44, _S42, int(0));
    float2 _S46 = float2(tap_1->weight_0.x) ;
    int _S47 = tap_1->hi_0.y;

#line 1932
    int3 _S48 = int3(_S41, _S47, int(0));
    int3 _S49 = int3(_S44, _S47, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z)))), decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z)))), _S46), mix(decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)))), decode_dfg_pair_0(((kernelContext_7->specular_dfg_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)))), _S46), float2(tap_1->weight_0.y) );
}


#line 3421
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3437
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3449
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3456
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2256
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2256
    float4 _S50 = float4(light_0->tangent_0) ;

    float3 _S51 = _S50.xyz;

#line 2258
    float3 across_0 = _S51 * float3(_S50.w) ;

#line 2258
    float4 _S52 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S51, _S52.xyz) * float3(_S52.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S53 = centre_0 - across_0;

#line 2261
    (*corners_0)[int(0)] = _S53 - down_0;
    float3 _S54 = centre_0 + across_0;

#line 2262
    (*corners_0)[int(1)] = _S54 - down_0;
    (*corners_0)[int(2)] = _S54 + down_0;
    (*corners_0)[int(3)] = _S53 + down_0;
    return;
}


#line 2014
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 2017
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 2018
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 2018
    }
    else
    {

#line 2018
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 2018
    }

#line 2018
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 2019
        tangent_5 = across_1 / float3(span_0) ;

#line 2019
    }
    else
    {

#line 2019
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 2019
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 1995
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2085
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2085
    float3 _S55 = polygon_0->corner_0[int(0)];

#line 2085
    float3 _S56 = polygon_0->corner_0[int(1)];

#line 2085
    float3 _S57 = polygon_0->corner_0[int(2)];

#line 2085
    float3 _S58 = polygon_0->corner_0[int(3)];

#line 2091
    float3 _S59 = float3(0.0f, 0.0f, 0.0f);


    float _S60 = polygon_0->corner_0[int(0)].z;

#line 2094
    int count_1;

#line 2094
    if(_S60 > 0.0f)
    {

#line 2094
        count_1 = int(1);

#line 2094
    }
    else
    {

#line 2094
        count_1 = int(0);

#line 2094
    }
    float _S61 = _S56.z;

#line 2095
    int _S62;

#line 2095
    if(_S61 > 0.0f)
    {

#line 2095
        _S62 = int(2);

#line 2095
    }
    else
    {

#line 2095
        _S62 = int(0);

#line 2095
    }

#line 2095
    int config_0 = count_1 + _S62;
    float _S63 = _S57.z;

#line 2096
    if(_S63 > 0.0f)
    {

#line 2096
        count_1 = int(4);

#line 2096
    }
    else
    {

#line 2096
        count_1 = int(0);

#line 2096
    }

#line 2096
    int config_1 = config_0 + count_1;
    float _S64 = _S58.z;

#line 2097
    if(_S64 > 0.0f)
    {

#line 2097
        count_1 = int(8);

#line 2097
    }
    else
    {

#line 2097
        count_1 = int(0);

#line 2097
    }

#line 2097
    int config_2 = config_1 + count_1;

#line 2097
    float3 l0_0;

#line 2097
    float3 l1_0;

#line 2097
    float3 l2_0;

#line 2097
    float3 l3_0;

#line 2097
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2100
        float3 _S65 = float3(_S60) ;


        float3 _S66 = float3(- _S61)  * _S55 + _S65 * _S56;
        float3 _S67 = float3(- _S64)  * _S55 + _S65 * _S58;

#line 2104
        count_1 = int(3);

#line 2104
        l0_0 = _S55;

#line 2104
        l1_0 = _S66;

#line 2104
        l2_0 = _S67;

#line 2104
        l3_0 = _S58;

#line 2104
        l4_0 = _S59;

#line 2100
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2106
            float3 _S68 = float3(_S61) ;


            float3 _S69 = float3(- _S60)  * _S56 + _S68 * _S55;
            float3 _S70 = float3(- _S63)  * _S56 + _S68 * _S57;

#line 2110
            count_1 = int(3);

#line 2110
            l0_0 = _S69;

#line 2110
            l1_0 = _S56;

#line 2110
            l2_0 = _S70;

#line 2110
            l3_0 = _S58;

#line 2110
            l4_0 = _S59;

#line 2106
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S71 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;
                float3 _S72 = float3(- _S64)  * _S55 + float3(_S60)  * _S58;

#line 2116
                count_1 = int(4);

#line 2116
                l0_0 = _S55;

#line 2116
                l1_0 = _S56;

#line 2116
                l2_0 = _S71;

#line 2116
                l3_0 = _S72;

#line 2116
                l4_0 = _S59;

#line 2112
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2118
                    float3 _S73 = float3(_S63) ;


                    float3 _S74 = float3(- _S64)  * _S57 + _S73 * _S58;
                    float3 _S75 = float3(- _S61)  * _S57 + _S73 * _S56;

#line 2122
                    count_1 = int(3);

#line 2122
                    l0_0 = _S74;

#line 2122
                    l1_0 = _S75;

#line 2122
                    l2_0 = _S57;

#line 2122
                    l3_0 = _S58;

#line 2122
                    l4_0 = _S59;

#line 2118
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S76 = float3(- _S60)  * _S56 + float3(_S61)  * _S55;
                        float3 _S77 = float3(- _S64)  * _S57 + float3(_S63)  * _S58;

#line 2128
                        count_1 = int(4);

#line 2128
                        l0_0 = _S76;

#line 2128
                        l1_0 = _S56;

#line 2128
                        l2_0 = _S57;

#line 2128
                        l3_0 = _S77;

#line 2128
                        l4_0 = _S59;

#line 2124
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2130
                            float3 _S78 = float3(- _S64) ;


                            float3 _S79 = _S78 * _S55 + float3(_S60)  * _S58;
                            float3 _S80 = _S78 * _S57 + float3(_S63)  * _S58;

#line 2134
                            count_1 = int(5);

#line 2134
                            l0_0 = _S55;

#line 2134
                            l1_0 = _S56;

#line 2134
                            l2_0 = _S57;

#line 2134
                            l3_0 = _S80;

#line 2134
                            l4_0 = _S79;

#line 2130
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2136
                                float3 _S81 = float3(_S64) ;


                                float3 _S82 = float3(- _S60)  * _S58 + _S81 * _S55;
                                float3 _S83 = float3(- _S63)  * _S58 + _S81 * _S57;

#line 2140
                                count_1 = int(3);

#line 2140
                                l0_0 = _S82;

#line 2140
                                l1_0 = _S83;

#line 2140
                                l2_0 = _S58;

#line 2140
                                l3_0 = _S58;

#line 2140
                                l4_0 = _S59;

#line 2136
                            }
                            else
                            {

#line 2143
                                if(config_2 == int(9))
                                {

                                    float3 _S84 = float3(- _S61)  * _S55 + float3(_S60)  * _S56;
                                    float3 _S85 = float3(- _S63)  * _S58 + float3(_S64)  * _S57;

#line 2147
                                    count_1 = int(4);

#line 2147
                                    l0_0 = _S55;

#line 2147
                                    l1_0 = _S84;

#line 2147
                                    l2_0 = _S85;

#line 2147
                                    l3_0 = _S58;

#line 2147
                                    l4_0 = _S59;

#line 2143
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S86 = float3(- _S64)  * _S57 + float3(_S63)  * _S58;
                                        float3 _S87 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;

#line 2154
                                        count_1 = int(5);

#line 2154
                                        l0_0 = _S55;

#line 2154
                                        l1_0 = _S56;

#line 2154
                                        l2_0 = _S87;

#line 2154
                                        l3_0 = _S86;

#line 2154
                                        l4_0 = _S58;

#line 2149
                                    }
                                    else
                                    {

#line 2156
                                        if(config_2 == int(12))
                                        {

                                            float3 _S88 = float3(- _S61)  * _S57 + float3(_S63)  * _S56;
                                            float3 _S89 = float3(- _S60)  * _S58 + float3(_S64)  * _S55;

#line 2160
                                            count_1 = int(4);

#line 2160
                                            l0_0 = _S89;

#line 2160
                                            l1_0 = _S88;

#line 2160
                                            l2_0 = _S57;

#line 2160
                                            l3_0 = _S58;

#line 2160
                                            l4_0 = _S59;

#line 2156
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S90 = float3(- _S63)  * _S56 + float3(_S61)  * _S57;
                                                float3 _S91 = float3(- _S61)  * _S55 + float3(_S60)  * _S56;

#line 2168
                                                count_1 = int(5);

#line 2168
                                                l0_0 = _S55;

#line 2168
                                                l1_0 = _S91;

#line 2168
                                                l2_0 = _S90;

#line 2168
                                                l3_0 = _S57;

#line 2168
                                                l4_0 = _S58;

#line 2162
                                            }
                                            else
                                            {

#line 2170
                                                if(config_2 == int(14))
                                                {

#line 2170
                                                    float3 _S92 = float3(- _S60) ;


                                                    float3 _S93 = _S92 * _S58 + float3(_S64)  * _S55;
                                                    float3 _S94 = _S92 * _S56 + float3(_S61)  * _S55;

#line 2174
                                                    count_1 = int(5);

#line 2174
                                                    l0_0 = _S94;

#line 2174
                                                    l1_0 = _S93;

#line 2170
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2176
                                                        count_1 = int(4);

#line 2176
                                                    }
                                                    else
                                                    {

#line 2176
                                                        count_1 = int(0);

#line 2176
                                                    }

#line 2176
                                                    l0_0 = _S55;

#line 2176
                                                    l1_0 = _S59;

#line 2170
                                                }

#line 2091
                                                float3 _S95 = l1_0;

#line 2091
                                                l1_0 = _S56;

#line 2091
                                                l2_0 = _S57;

#line 2091
                                                l3_0 = _S58;

#line 2091
                                                l4_0 = _S95;

#line 2162
                                            }

#line 2156
                                        }

#line 2149
                                    }

#line 2143
                                }

#line 2136
                            }

#line 2130
                        }

#line 2124
                    }

#line 2118
                }

#line 2112
            }

#line 2106
        }

#line 2100
    }

#line 2184
    if(count_1 <= int(3))
    {

#line 2184
        l3_0 = l0_0;

#line 2184
        l4_0 = l0_0;

#line 2184
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2189
            l4_0 = l0_0;

#line 2189
        }

#line 2184
    }

#line 2194
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 2057
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 2063
    float weight_1;

#line 2068
    if(cosine_0 > 0.0f)
    {

#line 2068
        weight_1 = fit_0;

#line 2068
    }
    else
    {

#line 2068
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2068
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2214
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2216
    int corner_1 = int(0);
    for(;;)
    {

#line 2217
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2217
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2217
        corner_1 = corner_1 + int(1);

#line 2217
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2222
    thread LtcPolygon_0 _S96 = polygon_1;

#line 2222
    LtcPolygon_0 _S97 = ltc_clip_0(&_S96);
    polygon_1 = _S97;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2226
    int at_2 = int(0);

    for(;;)
    {

#line 2228
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2228
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2228
        at_2 = at_2 + int(1);

#line 2228
    }

#line 2235
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2235
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2236
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2236
    }
    else
    {

#line 2236
        sum_1 = sum_0;

#line 2236
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2240
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2240
    }

#line 2247
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 1943
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_8)
{
    int _S98 = tap_2->lo_0.x;

#line 1945
    int _S99 = tap_2->lo_0.y;

#line 1945
    int3 _S100 = int3(_S98, _S99, int(0));
    int _S101 = tap_2->hi_0.x;

#line 1946
    int3 _S102 = int3(_S101, _S99, int(0));
    float4 _S103 = float4(tap_2->weight_0.x) ;
    int _S104 = tap_2->hi_0.y;

#line 1948
    int3 _S105 = int3(_S98, _S104, int(0));
    int3 _S106 = int3(_S101, _S104, int(0));

    return mix(mix(((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S100)).xy), uint(((_S100)).z))), ((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S102)).xy), uint(((_S102)).z))), _S103), mix(((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S105)).xy), uint(((_S105)).z))), ((kernelContext_8->ltc_matrix_0).read(vec<uint,2>(((_S106)).xy), uint(((_S106)).z))), _S103), float4(tap_2->weight_0.y) );
}


#line 2030
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 1825
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 1832
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1839
    float _S107 = 1.0f - alpha2_0;

#line 1844
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S107 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S107 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2488
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 2803
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 2816
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_9)
{
    return kernelContext_9->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 2816
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 2828
float2 atlas_step_0(float4 rect_0, KernelContext_0 thread* kernelContext_11)
{
    return kernelContext_11->frame_0->shadow_params_0.xy / rect_0.xy;
}


#line 2828
float2 atlas_step_1(float4 rect_1, KernelContext_0 thread* kernelContext_12)
{
    return kernelContext_12->frame_0->shadow_params_0.xy / rect_1.xy;
}


#line 324
float2 atlas_uv_0(float4 rect_2, float2 tile_uv_0)
{
    return rect_2.zw + tile_uv_0 * rect_2.xy;
}


#line 2946
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_13)
{
    float2 texel_1 = kernelContext_13->frame_0->shadow_params_0.xy;

#line 2948
    float4 _S108 = atlas_rect_0(cascade_0, kernelContext_13);

#line 2948
    float2 _S109 = atlas_step_0(_S108, kernelContext_13);


    float2 _S110 = float2(0.5f, 0.5f) * _S109;


    float2 _S111 = float2(1.0f, 1.0f);

#line 2954
    float2 _S112 = _S111 / texel_1;

#line 2954
    uint index_1 = 0U;

#line 2954
    float sum_2 = 0.0f;

#line 2954
    float found_0 = 0.0f;



    for(;;)
    {

#line 2958
        if(index_1 < 16U)
        {
        }
        else
        {

#line 2958
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_1] * float2(8.0f) ;
        float _S113 = spoke_0.x;

#line 2961
        float _S114 = rotation_0.x;

#line 2961
        float _S115 = spoke_0.y;

#line 2961
        float _S116 = rotation_0.y;

#line 2969
        int3 _S117 = int3(int2(min(atlas_uv_0(_S108, clamp(tile_uv_1 + float2(_S113 * _S114 - _S115 * _S116, _S113 * _S116 + _S115 * _S114) * _S109, _S110, float2(1.0f)  - _S110)) * _S112, _S112 - _S111)), int(0));

#line 2969
        float depth_1 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S117)).xy), uint(((_S117)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 2973
            sum_2 = sum_2 + depth_1;

#line 2973
            found_0 = found_1;

#line 2970
        }

#line 2958
        index_1 = index_1 + 1U;

#line 2958
    }

#line 2977
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 2988
    float _S118 = 2.0f * kernelContext_13->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_2 / found_0 - reference_0) * (_S118 + 40.0f) * 0.01999999955296516f / (_S118 / 768.0f), 2.0f, 8.0f);
}


#line 2850
float tile_tap_0(float4 rect_3, float2 texel_step_0, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_14)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S119 = spoke_1.x;

#line 2855
    float _S120 = rotation_1.x;

#line 2855
    float _S121 = spoke_1.y;

#line 2855
    float _S122 = rotation_1.y;


    float _S123 = ((kernelContext_14->shadow_atlas_0).sample_compare((kernelContext_14->shadow_sampler_0), (atlas_uv_0(rect_3, clamp(tile_uv_2 + float2(_S119 * _S120 - _S121 * _S122, _S119 * _S122 + _S121 * _S120) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2858
    return _S123;
}


#line 2880
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_2, KernelContext_0 thread* kernelContext_15)
{
    float2 _S124 = shadow_rotation_0(pixel_2);

#line 2882
    float4 _S125 = atlas_rect_1(tile_2, kernelContext_15);

#line 2882
    float2 _S126 = atlas_step_1(_S125, kernelContext_15);

#line 2882
    uint spot_0 = 0U;

#line 2882
    float probe_0 = 0.0f;

#line 2887
    for(;;)
    {

#line 2887
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2887
            break;
        }

#line 2887
        float _S127 = tile_tap_0(_S125, _S126, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S124, reference_2, kernelContext_15);

        float probe_1 = probe_0 + _S127;

#line 2887
        spot_0 = spot_0 + 1U;

#line 2887
        probe_0 = probe_1;

#line 2887
    }

#line 2896
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2902
    uint index_2 = 0U;

#line 2902
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2906
        if(index_2 < 32U)
        {
        }
        else
        {

#line 2906
            break;
        }

#line 2906
        float _S128 = tile_tap_0(_S125, _S126, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S124, reference_2, kernelContext_15);

        float visibility_1 = visibility_0 + _S128;

#line 2906
        index_2 = index_2 + 1U;

#line 2906
        visibility_0 = visibility_1;

#line 2906
    }

#line 2911
    return visibility_0 / 32.0f;
}


#line 3042
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_16)
{

#line 3073
    float texel_world_0 = 2.0f * kernelContext_16->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 3080
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 3084
    bool _S129;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 3085
        _S129 = true;

#line 3085
    }
    else
    {

#line 3085
        _S129 = (ndc_0.z) <= 0.0f;

#line 3085
    }

#line 3085
    if(_S129)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3112
    float _S130 = ndc_0.z;

#line 3112
    float _S131 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S130, shadow_rotation_0(pixel_3), kernelContext_16);

#line 3112
    float _S132 = tile_pcf_0(cascade_1, tile_uv_4, _S130, pixel_3, _S131, kernelContext_16);
    return _S132;
}


#line 3129
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_17)
{

#line 3130
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3142
    float eye_distance_0 = length(world_position_5 - kernelContext_17->frame_0->camera_position_0.xyz);

#line 3142
    uint index_3 = 0U;

    for(;;)
    {

#line 3144
        if(index_3 < 2U)
        {
        }
        else
        {

#line 3144
            cascade_2 = 1U;

#line 3144
            break;
        }
        if(eye_distance_0 < kernelContext_17->frame_0->cascade_far_0[index_3])
        {

#line 3146
            cascade_2 = index_3;


            break;
        }

#line 3144
        index_3 = index_3 + 1U;

#line 3144
    }

#line 3144
    float _S133 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_17);

#line 3155
    uint _S134 = cascade_2 + 1U;

#line 3155
    if(_S134 >= 2U)
    {



        return _S133;
    }

#line 3168
    float band_0 = kernelContext_17->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_17->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S133;
    }

#line 3172
    float _S135 = cascade_visibility_0(_S134, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_17);

#line 3183
    return mix(_S133, _S135, blend_0);
}


#line 3373
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S136 = axis_2.x;

#line 3376
    float _S137 = axis_2.y;

#line 3376
    bool _S138;

#line 3376
    if(_S136 >= _S137)
    {

#line 3376
        _S138 = _S136 >= (axis_2.z);

#line 3376
    }
    else
    {

#line 3376
        _S138 = false;

#line 3376
    }

#line 3376
    uint _S139;

#line 3376
    if(_S138)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3378
            _S139 = 0U;

#line 3378
        }
        else
        {

#line 3378
            _S139 = 1U;

#line 3378
        }

#line 3378
        return _S139;
    }
    if(_S137 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3382
            _S139 = 2U;

#line 3382
        }
        else
        {

#line 3382
            _S139 = 3U;

#line 3382
        }

#line 3382
        return _S139;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3384
        _S139 = 4U;

#line 3384
    }
    else
    {

#line 3384
        _S139 = 5U;

#line 3384
    }

#line 3384
    return _S139;
}


#line 311
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 3286
float punctual_visibility_0(uint tile_4, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_18)
{

#line 3298
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 3305
    float _S140 = clip_1.w;

#line 3305
    if(_S140 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S140) ;

#line 3309
    bool _S141;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3310
        _S141 = true;

#line 3310
    }
    else
    {

#line 3310
        _S141 = (ndc_1.z) <= 0.0f;

#line 3310
    }

#line 3310
    if(_S141)
    {

#line 3310
        _S141 = true;

#line 3310
    }
    else
    {

#line 3310
        _S141 = (ndc_1.z) > 1.0f;

#line 3310
    }

#line 3310
    if(_S141)
    {

#line 3317
        return 1.0f;
    }

#line 3317
    float _S142 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_18);

#line 3327
    return _S142;
}


#line 3392
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_19)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3400
    float _S143 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_19);

#line 3406
    return _S143;
}


#line 3334
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_5, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_20)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3341
    float4 _S144 = float4(light_2->direction_0) ;

#line 3348
    float cos_outer_1 = _S144.w;

#line 3348
    float _S145 = punctual_visibility_0(tile_5, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S144.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_20);

#line 3355
    return _S145;
}


#line 1971
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 3599
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_21)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_21->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_21->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_21->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 981
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 3501
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_22)
{

    GpuProbe_natural_0 _S146 = kernelContext_22->probes_0[min((cell_1.z * kernelContext_22->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_22->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_22->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 3504
    GpuProbe_0 _S147 = { float4(_S146.sh_r_0) , float4(_S146.sh_g_0) , float4(_S146.sh_b_0)  };

#line 3504
    return _S147;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_1, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S148 = float4(t_1) ;

#line 3512
    (&blended_0)->sh_r_0 = mix(a_1->sh_r_0, b_0->sh_r_0, _S148);
    (&blended_0)->sh_g_0 = mix(a_1->sh_g_0, b_0->sh_g_0, _S148);
    (&blended_0)->sh_b_0 = mix(a_1->sh_b_0, b_0->sh_b_0, _S148);
    return blended_0;
}


#line 3552
float3 probe_irradiance_0(float3 world_position_9, float3 normal_7, KernelContext_0 thread* kernelContext_23)
{

#line 3552
    float3 _S149 = float3(1.0f) ;

#line 3557
    float3 _S150 = float3(0.0f, 0.0f, 0.0f);

#line 3557
    float3 last_0 = max(float3(kernelContext_23->frame_0->probe_counts_0.xyz) - _S149, _S150);
    float3 grid_0 = clamp((world_position_9 - kernelContext_23->frame_0->probe_origin_0.xyz) * kernelContext_23->frame_0->probe_inv_spacing_0.xyz, _S150, last_0);

    float3 base_2 = floor(grid_0);
    float3 f_0 = grid_0 - base_2;

    uint3 _S151 = uint3(base_2);



    uint3 _S152 = uint3(min(base_2 + _S149, last_0));

#line 3574
    uint _S153 = _S151.x;

#line 3574
    uint _S154 = _S151.y;

#line 3574
    uint _S155 = _S151.z;

#line 3574
    GpuProbe_0 _S156 = probe_at_0(uint3(_S153, _S154, _S155), kernelContext_23);

#line 3574
    uint _S157 = _S152.x;

#line 3574
    GpuProbe_0 _S158 = probe_at_0(uint3(_S157, _S154, _S155), kernelContext_23);

#line 3574
    float _S159 = f_0.x;

#line 3574
    thread GpuProbe_0 _S160 = _S156;

#line 3574
    thread GpuProbe_0 _S161 = _S158;

#line 3574
    GpuProbe_0 _S162 = lerp_probe_0(&_S160, &_S161, _S159);
    uint _S163 = _S152.y;

#line 3575
    GpuProbe_0 _S164 = probe_at_0(uint3(_S153, _S163, _S155), kernelContext_23);

#line 3575
    GpuProbe_0 _S165 = probe_at_0(uint3(_S157, _S163, _S155), kernelContext_23);

#line 3575
    thread GpuProbe_0 _S166 = _S164;

#line 3575
    thread GpuProbe_0 _S167 = _S165;

#line 3575
    GpuProbe_0 _S168 = lerp_probe_0(&_S166, &_S167, _S159);
    uint _S169 = _S152.z;

#line 3576
    GpuProbe_0 _S170 = probe_at_0(uint3(_S153, _S154, _S169), kernelContext_23);

#line 3576
    GpuProbe_0 _S171 = probe_at_0(uint3(_S157, _S154, _S169), kernelContext_23);

#line 3576
    thread GpuProbe_0 _S172 = _S170;

#line 3576
    thread GpuProbe_0 _S173 = _S171;

#line 3576
    GpuProbe_0 _S174 = lerp_probe_0(&_S172, &_S173, _S159);

#line 3576
    GpuProbe_0 _S175 = probe_at_0(uint3(_S153, _S163, _S169), kernelContext_23);

#line 3576
    GpuProbe_0 _S176 = probe_at_0(uint3(_S157, _S163, _S169), kernelContext_23);

#line 3576
    thread GpuProbe_0 _S177 = _S175;

#line 3576
    thread GpuProbe_0 _S178 = _S176;

#line 3576
    GpuProbe_0 _S179 = lerp_probe_0(&_S177, &_S178, _S159);

    float _S180 = f_0.y;

#line 3578
    thread GpuProbe_0 _S181 = _S162;

#line 3578
    thread GpuProbe_0 _S182 = _S168;

#line 3578
    GpuProbe_0 _S183 = lerp_probe_0(&_S181, &_S182, _S180);

#line 3578
    thread GpuProbe_0 _S184 = _S174;

#line 3578
    thread GpuProbe_0 _S185 = _S179;

#line 3578
    GpuProbe_0 _S186 = lerp_probe_0(&_S184, &_S185, _S180);

    float _S187 = f_0.z;

#line 3580
    thread GpuProbe_0 _S188 = _S183;

#line 3580
    thread GpuProbe_0 _S189 = _S186;

#line 3580
    GpuProbe_0 _S190 = lerp_probe_0(&_S188, &_S189, _S187);

    float4 basis_7 = float4(normal_7, 1.0f);
    return max(float3(dot(_S190.sh_r_0, basis_7), dot(_S190.sh_g_0, basis_7), dot(_S190.sh_b_0, basis_7)), _S150);
}


#line 954
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2322
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S191 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2330
    float kernel_0 = 0.0001984127011383f;

#line 2330
    int term_0 = int(6);

    for(;;)
    {

#line 2332
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2332
            break;
        }
        float _S192 = kernel_0 * _S191 + FOG_KERNEL_0[term_0];

#line 2332
        int term_1 = term_0 - int(1);

#line 2332
        kernel_0 = _S192;

#line 2332
        term_0 = term_1;

#line 2332
    }

#line 2339
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2349
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S193 = - d_0;

#line 2353
        float series_0 = 0.00833333376795053f;

#line 2353
        int term_2 = int(3);

        for(;;)
        {

#line 2355
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2355
                break;
            }
            float _S194 = series_0 * _S193 + FOG_RATIO_KERNEL_0[term_2];

#line 2355
            int term_3 = term_2 - int(1);

#line 2355
            series_0 = _S194;

#line 2355
            term_2 = term_3;

#line 2355
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2383
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2394
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2402
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 3625
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 3625
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


#line 3964
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S195 [[stage_in]], float4 position_4 [[position]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 3964
    thread KernelContext_0 kernelContext_24;

#line 3964
    (&kernelContext_24)->draw_0 = draw_2;

#line 3964
    (&kernelContext_24)->visible_instances_0 = visible_instances_2;

#line 3964
    (&kernelContext_24)->instances_0 = instances_2;

#line 3964
    (&kernelContext_24)->meshes_0 = meshes_2;

#line 3964
    (&kernelContext_24)->frame_0 = frame_4;

#line 3964
    (&kernelContext_24)->vertices_0 = vertices_2;

#line 3964
    (&kernelContext_24)->ambient_occlusion_0 = ambient_occlusion_2;

#line 3964
    (&kernelContext_24)->materials_0 = materials_2;

#line 3964
    (&kernelContext_24)->normal_textures_0 = normal_textures_2;

#line 3964
    (&kernelContext_24)->base_color_sampler_0 = base_color_sampler_2;

#line 3964
    (&kernelContext_24)->base_color_textures_0 = base_color_textures_2;

#line 3964
    (&kernelContext_24)->cluster_lights_0 = cluster_lights_2;

#line 3964
    (&kernelContext_24)->specular_dfg_0 = specular_dfg_2;

#line 3964
    (&kernelContext_24)->lights_0 = lights_2;

#line 3964
    (&kernelContext_24)->ltc_matrix_0 = ltc_matrix_2;

#line 3964
    (&kernelContext_24)->shadow_atlas_0 = shadow_atlas_2;

#line 3964
    (&kernelContext_24)->shadow_sampler_0 = shadow_sampler_2;

#line 3964
    (&kernelContext_24)->probes_0 = probes_2;

#line 3976
    float3 vertex_normal_0 = normalize(_S195.world_normal_1);

#line 3981
    float2 motion_1 = motion_vector_0(_S195.clip_position_1, _S195.previous_clip_position_1);

#line 3990
    if((frame_4->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 4032
    if((frame_4->ambient_0.w) >= 3.5f)
    {

#line 4032
        float _S196 = occlusion_at_0(position_4.xy, &kernelContext_24);

        thread FragmentOutput_0 occlusion_0;

#line 4043
        (&occlusion_0)->lit_0 = float4(_S196, _S196, _S196, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_4->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S195.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 4060
    thread GpuMaterial_natural_0 _S197 = (&kernelContext_24)->materials_0[_S195.material_5];

#line 4060
    float2 uv_3;

#line 4085
    if(((&_S197)->tiling_0) == 1U)
    {

#line 4085
        uv_3 = physical_tile_uv_0(_S195.world_position_10, vertex_normal_0, (&_S197)->tile_metres_0);

#line 4085
    }
    else
    {

#line 4085
        uv_3 = _S195.uv_2;

#line 4085
    }

#line 4085
    uint _S198 = normal_layer_0(&_S197);

#line 4085
    thread VertexOutput_0 _S199;

#line 4085
    (&_S199)->position_3 = position_4;

#line 4085
    (&_S199)->world_position_1 = _S195.world_position_10;

#line 4085
    (&_S199)->world_normal_0 = _S195.world_normal_1;

#line 4085
    (&_S199)->color_2 = _S195.color_3;

#line 4085
    (&_S199)->material_2 = _S195.material_5;

#line 4085
    (&_S199)->uv_0 = _S195.uv_2;

#line 4085
    (&_S199)->clip_position_0 = _S195.clip_position_1;

#line 4085
    (&_S199)->previous_clip_position_0 = _S195.previous_clip_position_1;

#line 4085
    (&_S199)->world_tangent_0 = _S195.world_tangent_1;

#line 4085
    (&_S199)->frame_2 = _S195.frame_3;

#line 4085
    float3 _S200 = shading_normal_of_0(_S198, (&_S197)->normal_scale_0, &_S199, vertex_normal_0, uv_3, &kernelContext_24);

#line 4092
    if((frame_4->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 4094
        float3 _S201 = float3(0.5f) ;

#line 4106
        (&normals_0)->lit_0 = float4(_S200 * _S201 + _S201, 1.0f);

#line 4112
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_24)->frame_0->camera_position_0.xyz - _S195.world_position_10);



    float3 _S202 = geometric_normal_of_0(_S195.world_position_10, vertex_normal_0);

#line 4121
    uint _S203 = base_color_layer_0(&_S197);

#line 4136
    float3 _S204 = float3(uv_3, float(_S203));
    float4 albedo_0 = _S195.color_3 * float4((&_S197)->base_color_0)  * (((&kernelContext_24)->base_color_textures_0).sample(((&kernelContext_24)->base_color_sampler_0), ((_S204)).xy, uint(((_S204)).z)));

#line 4143
    float metallic_1 = saturate((&_S197)->metallic_0);
    float roughness_2 = clamp((&_S197)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S205 = alpha_0 * alpha_0;

#line 4152
    float3 _S206 = albedo_0.xyz;

#line 4152
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S206, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S206 * float3((1.0f - metallic_1)) ;

#line 4159
    float _S207 = max(dot(_S200, to_eye_1), 0.00009999999747379f);

#line 4169
    float2 _S208 = position_4.xy;

#line 4169
    uint _S209 = froxel_of_0(_S208, (((float4(_S195.world_position_10, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_24)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_24)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_24);

#line 4169
    uint base_3 = _S209 * 17U;

#line 4174
    uint _S210 = min((&kernelContext_24)->cluster_lights_0[base_3], 16U);

#line 4174
    TableTap_0 _S211 = table_tap_0(_S207, roughness_2, &kernelContext_24);

#line 4174
    thread TableTap_0 _S212 = _S211;

#line 4174
    float2 _S213 = dfg_at_0(&_S212, &kernelContext_24);

#line 4183
    float _S214 = _S213.x;

#line 4183
    float _S215 = _S213.y;

#line 4183
    float3 _S216 = f0_2 * float3(_S214)  + float3(_S215) ;

#line 4189
    float3 _S217 = float3(0.0f, 0.0f, 0.0f);

#line 4189
    uint slot_0 = 0U;

#line 4189
    float3 direct_0 = _S217;

#line 4189
    float3 gloss_0 = _S217;

    for(;;)
    {

#line 4191
        if(slot_0 < _S210)
        {
        }
        else
        {

#line 4191
            break;
        }

#line 4191
        thread GpuLight_natural_0 _S218 = (&kernelContext_24)->lights_0[(&kernelContext_24)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 4191
        uint _S219 = (&_S218)->kind_0;

#line 4200
        bool _S220 = ((&_S218)->kind_0) == 0U;

#line 4200
        float3 to_light_7;

#line 4200
        float reach_0;

#line 4200
        if(_S220)
        {

#line 4200
            to_light_7 = normalize((float4((&_S218)->direction_0) ).xyz);

#line 4200
            reach_0 = 1.0f;

#line 4200
        }
        else
        {


            if(_S219 == 3U)
            {

#line 4205
                float4 _S221 = float4((&_S218)->position_0) ;

#line 4213
                float3 offset_0 = _S221.xyz - _S195.world_position_10;
                float distance_3 = length(offset_0);

                float _S222 = range_window_0(distance_3, _S221.w);

#line 4216
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 4216
                reach_0 = _S222;

#line 4205
            }
            else
            {

#line 4205
                float4 _S223 = float4((&_S218)->position_0) ;

#line 4220
                float3 offset_1 = _S223.xyz - _S195.world_position_10;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_1 = punctual_falloff_0(distance_4, _S223.w);
                if(_S219 == 2U)
                {

#line 4224
                    float4 _S224 = float4((&_S218)->direction_0) ;

#line 4224
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S224.xyz, _S224.w, (&_S218)->cos_inner_0);

#line 4224
                }
                else
                {

#line 4224
                    reach_0 = reach_1;

#line 4224
                }

#line 4224
                to_light_7 = to_light_8;

#line 4205
            }

#line 4200
        }

#line 4233
        float n_dot_l_5 = dot(_S200, to_light_7);

#line 4233
        float3 specular_0;

#line 4233
        float diffuse_0;


        if(_S219 == 3U)
        {

#line 4246
            thread array<float3, int(4)> corners_2;

#line 4246
            rect_corners_0(&_S218, _S195.world_position_10, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S200, to_eye_1, _S207);

#line 4248
            thread array<float3, int(4)> _S225 = corners_2;

#line 4248
            float _S226 = ltc_irradiance_0(to_local_0, &_S225);

#line 4248
            thread TableTap_0 _S227 = _S211;

#line 4248
            float4 _S228 = ltc_at_0(&_S227, &kernelContext_24);

            matrix<float,int(3),int(3)>  _S229 = (((to_local_0) * (ltc_transform_0(_S228))));

#line 4250
            thread array<float3, int(4)> _S230 = corners_2;

#line 4250
            float _S231 = ltc_irradiance_0(_S229, &_S230);
            float3 _S232 = float3(_S231)  * _S216;

#line 4251
            diffuse_0 = _S226;

#line 4251
            specular_0 = _S232;

#line 4236
        }
        else
        {

#line 4256
            float _S233 = max(n_dot_l_5, 0.0f);

#line 4263
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 4271
            float3 specular_1 = ggx_lobe_0(_S205, f0_2, _S233, _S207, max(dot(_S200, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S233) ;

#line 4271
            diffuse_0 = _S233;

#line 4271
            specular_0 = specular_1;

#line 4236
        }

#line 4236
        float3 specular_2;

#line 4279
        if((((&_S218)->flags_3) & 1U) != 0U)
        {

#line 4279
            specular_2 = _S217;

#line 4279
        }
        else
        {

#line 4279
            specular_2 = specular_0;

#line 4279
        }

#line 4279
        float reach_2;

#line 4297
        if(_S220)
        {

#line 4297
            float _S234 = sun_visibility_0(_S195.world_position_10, to_light_7, n_dot_l_5, _S202, _S208, &kernelContext_24);

#line 4297
            reach_2 = _S234;

#line 4297
        }
        else
        {


            if(_S219 == 1U)
            {

#line 4302
                uint _S235 = (&_S218)->shadow_tile_0;

#line 4314
                if(((&_S218)->shadow_tile_0) <= 8U)
                {

#line 4314
                    float _S236 = point_visibility_0(&_S218, _S235, _S195.world_position_10, to_light_7, n_dot_l_5, _S202, _S208, &kernelContext_24);

#line 4314
                    reach_2 = reach_0 * _S236;

#line 4314
                }
                else
                {

#line 4314
                    reach_2 = reach_0;

#line 4314
                }

#line 4302
            }
            else
            {

#line 4302
                uint _S237 = (&_S218)->shadow_tile_0;

#line 4320
                if(((&_S218)->shadow_tile_0) < 14U)
                {

#line 4320
                    float _S238 = spot_visibility_0(&_S218, _S237, _S195.world_position_10, to_light_7, n_dot_l_5, _S202, _S208, &kernelContext_24);

#line 4320
                    reach_2 = reach_0 * _S238;

#line 4320
                }
                else
                {

#line 4320
                    reach_2 = reach_0;

#line 4320
                }

#line 4302
            }

#line 4297
        }

#line 4328
        float3 _S239 = (float4((&_S218)->color_0) ).xyz;

#line 4328
        float3 direct_1 = direct_0 + _S239 * float3((diffuse_0 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S239 * (specular_2 * float3(reach_2) );

#line 4191
        slot_0 = slot_0 + 1U;

#line 4191
        direct_0 = direct_1;

#line 4191
        gloss_0 = gloss_1;

#line 4191
    }

#line 4343
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S214 + _S215);

#line 4343
    float _S240 = occlusion_at_0(_S208, &kernelContext_24);

#line 4379
    float3 _S241 = frame_4->ambient_0.xyz;

#line 4379
    float3 _S242 = sky_irradiance_0(_S200, &kernelContext_24);

#line 4379
    float3 _S243 = _S241 + _S242;

#line 4379
    float3 _S244 = probe_irradiance_0(_S195.world_position_10, _S200, &kernelContext_24);

#line 4400
    float3 lit_1 = diffuse_albedo_0 * ((_S243 + _S244) * float3(_S240)  + direct_0) + gloss_2;

#line 4400
    float3 _S245 = emissive_of_0(&_S197);

#line 4436
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_24)->frame_0->fog_params_0.x, (&kernelContext_24)->frame_0->fog_params_0.y, (&kernelContext_24)->frame_0->camera_position_0.y - (&kernelContext_24)->frame_0->fog_params_0.z, _S195.world_position_10.y - (&kernelContext_24)->frame_0->fog_params_0.z, length((&kernelContext_24)->frame_0->camera_position_0.xyz - _S195.world_position_10)));


    thread FragmentOutput_0 output_1;



    (&output_1)->lit_0 = float4((lit_1 + _S245) * float3(fog_survives_0)  + (&kernelContext_24)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_1)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_1)->motion_0 = motion_1;
    return output_1;
}


#line 4449
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


#line 4449
[[vertex]] vertexMain_Result_0 vertexMain(uint index_4 [[vertex_id]], uint instance_id_1 [[instance_id]], DrawConstants_0 constant* draw_3 [[buffer(3)]], uint device* visible_instances_3 [[buffer(5)]], GpuInstance_natural_0 device* instances_3 [[buffer(2)]], GpuMesh_0 device* meshes_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_6 [[buffer(0)]], uint device* vertices_3 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_3 [[texture(2)]], GpuMaterial_natural_0 device* materials_3 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_3 [[texture(4)]], sampler base_color_sampler_3 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_3 [[texture(0)]], uint device* cluster_lights_3 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_3 [[texture(3)]], GpuLight_natural_0 device* lights_3 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_3 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], GpuProbe_natural_0 device* probes_3 [[buffer(9)]])
{

#line 4449
    thread KernelContext_0 kernelContext_25;

#line 4449
    (&kernelContext_25)->draw_0 = draw_3;

#line 4449
    (&kernelContext_25)->visible_instances_0 = visible_instances_3;

#line 4449
    (&kernelContext_25)->instances_0 = instances_3;

#line 4449
    (&kernelContext_25)->meshes_0 = meshes_3;

#line 4449
    (&kernelContext_25)->frame_0 = frame_6;

#line 4449
    (&kernelContext_25)->vertices_0 = vertices_3;

#line 4449
    (&kernelContext_25)->ambient_occlusion_0 = ambient_occlusion_3;

#line 4449
    (&kernelContext_25)->materials_0 = materials_3;

#line 4449
    (&kernelContext_25)->normal_textures_0 = normal_textures_3;

#line 4449
    (&kernelContext_25)->base_color_sampler_0 = base_color_sampler_3;

#line 4449
    (&kernelContext_25)->base_color_textures_0 = base_color_textures_3;

#line 4449
    (&kernelContext_25)->cluster_lights_0 = cluster_lights_3;

#line 4449
    (&kernelContext_25)->specular_dfg_0 = specular_dfg_3;

#line 4449
    (&kernelContext_25)->lights_0 = lights_3;

#line 4449
    (&kernelContext_25)->ltc_matrix_0 = ltc_matrix_3;

#line 4449
    (&kernelContext_25)->shadow_atlas_0 = shadow_atlas_3;

#line 4449
    (&kernelContext_25)->shadow_sampler_0 = shadow_sampler_3;

#line 4449
    (&kernelContext_25)->probes_0 = probes_3;

#line 4449
    GpuInstance_natural_0 device* _S246 = instances_3+visible_instances_3[draw_3->base_0 + instance_id_1];

#line 1639
    GpuMesh_0 mesh_3 = meshes_3[draw_3->mesh_0];

#line 1647
    bool _S247 = ((_S246->flags_0) & 2U) != 0U;

#line 1647
    uint base_vertex_3;
    if(_S247)
    {

#line 1648
        base_vertex_3 = _S246->base_vertex_0;

#line 1648
    }
    else
    {

#line 1648
        base_vertex_3 = mesh_3.base_vertex_1;

#line 1648
    }

#line 1648
    MeshVertex_0 _S248 = load_vertex_0(index_4 + base_vertex_3, float4(mesh_3.uv_scale_u_0, mesh_3.uv_scale_v_0, mesh_3.uv_offset_u_0, mesh_3.uv_offset_v_0), &kernelContext_25);

#line 1648
    uint previous_base_0;

#line 1661
    if(_S247)
    {

#line 1661
        previous_base_0 = _S246->previous_base_vertex_0;

#line 1661
    }
    else
    {

#line 1661
        previous_base_0 = base_vertex_3;

#line 1661
    }

#line 1661
    float3 _S249 = load_position_0(index_4 + previous_base_0, &kernelContext_25);

#line 1661
    matrix<float,int(4),int(4)>  _S250 = matrix<float,int(4),int(4)> (_S246->transform_0.data_0[int(0)][int(0)], _S246->transform_0.data_0[int(1)][int(0)], _S246->transform_0.data_0[int(2)][int(0)], _S246->transform_0.data_0[int(3)][int(0)], _S246->transform_0.data_0[int(0)][int(1)], _S246->transform_0.data_0[int(1)][int(1)], _S246->transform_0.data_0[int(2)][int(1)], _S246->transform_0.data_0[int(3)][int(1)], _S246->transform_0.data_0[int(0)][int(2)], _S246->transform_0.data_0[int(1)][int(2)], _S246->transform_0.data_0[int(2)][int(2)], _S246->transform_0.data_0[int(3)][int(2)], _S246->transform_0.data_0[int(0)][int(3)], _S246->transform_0.data_0[int(1)][int(3)], _S246->transform_0.data_0[int(2)][int(3)], _S246->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S248.position_1, 1.0f)) * (_S250)));

    thread VertexOutput_0 output_2;
    (&output_2)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_25)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_2)->world_position_1 = world_0.xyz;

#line 1675
    matrix<float,int(3),int(3)>  _S251 = matrix<float,int(3),int(3)> (_S250[int(0)].xyz, _S250[int(1)].xyz, _S250[int(2)].xyz);

#line 1675
    (&output_2)->world_normal_0 = (((_S248.basis_1.normal_0) * (normal_basis_0(_S251))));

#line 1681
    (&output_2)->world_tangent_0 = (((_S248.basis_1.tangent_1) * (_S251)));

#line 1681
    thread TangentFrame_0 _S252 = _S248.basis_1;

#line 1681
    uint _S253 = frame_word_0(mesh_3.flags_1, &_S252);
    (&output_2)->frame_2 = _S253;

#line 1682
    float4 _S254;

#line 1689
    if(((&kernelContext_25)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1689
        _S254 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1689
    }
    else
    {

#line 1689
        _S254 = _S248.color_1;

#line 1689
    }

#line 1688
    (&output_2)->color_2 = _S254;

#line 1695
    (&output_2)->material_2 = _S246->material_0;
    (&output_2)->uv_0 = _S248.uv0_0;

#line 1702
    (&output_2)->clip_position_0 = (&output_2)->position_3;
    (&output_2)->previous_clip_position_0 = ((((((float4(_S249, 1.0f)) * (matrix<float,int(4),int(4)> (_S246->previous_transform_0.data_0[int(0)][int(0)], _S246->previous_transform_0.data_0[int(1)][int(0)], _S246->previous_transform_0.data_0[int(2)][int(0)], _S246->previous_transform_0.data_0[int(3)][int(0)], _S246->previous_transform_0.data_0[int(0)][int(1)], _S246->previous_transform_0.data_0[int(1)][int(1)], _S246->previous_transform_0.data_0[int(2)][int(1)], _S246->previous_transform_0.data_0[int(3)][int(1)], _S246->previous_transform_0.data_0[int(0)][int(2)], _S246->previous_transform_0.data_0[int(1)][int(2)], _S246->previous_transform_0.data_0[int(2)][int(2)], _S246->previous_transform_0.data_0[int(3)][int(2)], _S246->previous_transform_0.data_0[int(0)][int(3)], _S246->previous_transform_0.data_0[int(1)][int(3)], _S246->previous_transform_0.data_0[int(2)][int(3)], _S246->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_25)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S255 = output_2;

#line 1706
    thread vertexMain_Result_0 _S256;

#line 1706
    (&_S256)->position_5 = _S255.position_3;

#line 1706
    (&_S256)->world_position_11 = _S255.world_position_1;

#line 1706
    (&_S256)->world_normal_2 = _S255.world_normal_0;

#line 1706
    (&_S256)->color_4 = _S255.color_2;

#line 1706
    (&_S256)->material_6 = _S255.material_2;

#line 1706
    (&_S256)->uv_4 = _S255.uv_0;

#line 1706
    (&_S256)->clip_position_2 = _S255.clip_position_0;

#line 1706
    (&_S256)->previous_clip_position_2 = _S255.previous_clip_position_0;

#line 1706
    (&_S256)->world_tangent_2 = _S255.world_tangent_0;

#line 1706
    (&_S256)->frame_5 = _S255.frame_2;

#line 1706
    return _S256;
}

