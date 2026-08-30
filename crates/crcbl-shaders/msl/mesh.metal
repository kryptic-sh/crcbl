#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 2234 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 2229
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2501
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2561
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2713
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2576
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2604
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


#line 3859
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S7 = previous_0.w;

#line 3861
    if(_S7 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S7) ) * float2(0.5f, -0.5f);
}


#line 3827
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_2)
{

#line 3827
    texture2d<float, access::sample> _S8 = kernelContext_2->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S8).get_width(0)),(*((&height_0)) = (_S8).get_height(0));

    int3 _S9 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 3833
    return ((kernelContext_2->ambient_occlusion_0).read(vec<uint,2>(((_S9)).xy), uint(((_S9)).z)).x);
}


#line 3577
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S10 = axis_0.x;

#line 3581
    float _S11 = axis_0.y;

#line 3581
    bool _S12;

#line 3581
    if(_S10 >= _S11)
    {

#line 3581
        _S12 = _S10 >= (axis_0.z);

#line 3581
    }
    else
    {

#line 3581
        _S12 = false;

#line 3581
    }

#line 3581
    float2 planar_0;

#line 3581
    if(_S12)
    {

#line 3581
        planar_0 = world_position_0.zy;

#line 3581
    }
    else
    {

        if(_S11 >= (axis_0.z))
        {

#line 3585
            planar_0 = world_position_0.xz;

#line 3585
        }
        else
        {

#line 3585
            planar_0 = world_position_0.xy;

#line 3585
        }

#line 3581
    }

#line 3593
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 922
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 3614
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S13 = normal_2.z;

#line 3616
    float sign_z_0;

#line 3616
    if(_S13 >= 0.0f)
    {

#line 3616
        sign_z_0 = 1.0f;

#line 3616
    }
    else
    {

#line 3616
        sign_z_0 = -1.0f;

#line 3616
    }
    float a_0 = -1.0f / (sign_z_0 + _S13);
    float _S14 = normal_2.x;

#line 3618
    float _S15 = sign_z_0 * _S14;

#line 3618
    return float3(1.0f + _S15 * _S14 * a_0, _S15 * normal_2.y * a_0, - sign_z_0 * _S14);
}


#line 3668
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S16 = duvdy_0.y;

#line 3670
    float _S17 = duvdx_0.y;

#line 3670
    float winding_0;
    if((duvdx_0.x * _S16 - duvdy_0.x * _S17) < 0.0f)
    {

#line 3671
        winding_0 = -1.0f;

#line 3671
    }
    else
    {

#line 3671
        winding_0 = 1.0f;

#line 3671
    }
    float3 tangent_2 = (float3(_S16)  * dpdx_0 - float3(_S17)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 3680
    float3 tangent_3 = tangent_2 - normal_3 * float3(dot(normal_3, tangent_2)) ;
    float length_squared_0 = dot(tangent_3, tangent_3);

#line 3681
    float3 _S18;

#line 3690
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 3690
        _S18 = tangent_3 * float3(rsqrt(length_squared_0)) ;

#line 3690
    }
    else
    {

#line 3690
        _S18 = orthonormal_tangent_0(normal_3);

#line 3690
    }

#line 3690
    (&basis_4)->tangent_1 = _S18;

    (&basis_4)->bitangent_0 = cross(normal_3, _S18);
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
    [[flat]] uint frame_1;
};


#line 3750
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_3)
{

#line 3762
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 3772
    uint _S19 = input_0->frame_1;
    if(((input_0->frame_1) & 1U) != 0U)
    {

#line 3781
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_4 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_4, tangent_4);

#line 3783
        float3 _S20;

#line 3788
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 3788
            _S20 = tangent_4 * float3(rsqrt(length_squared_1)) ;

#line 3788
        }
        else
        {

#line 3788
            _S20 = orthonormal_tangent_0(normal_4);

#line 3788
        }

#line 3788
        (&basis_5)->tangent_1 = _S20;

#line 3794
        float3 _S21 = cross((&basis_5)->normal_0, _S20);

#line 3794
        float _S22;
        if((_S19 & 2U) != 0U)
        {

#line 3795
            _S22 = -1.0f;

#line 3795
        }
        else
        {

#line 3795
            _S22 = 1.0f;

#line 3795
        }

#line 3794
        (&basis_5)->bitangent_0 = _S21 * float3(_S22) ;

#line 3773
    }
    else
    {

#line 3799
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 3773
    }

#line 3803
    float3 _S23 = float3(uv_1, float(layer_0));
    float3 _S24 = ((kernelContext_3->normal_textures_0).sample((kernelContext_3->base_color_sampler_0), ((_S23)).xy, uint(((_S23)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 3804
    thread float3 tangent_space_0 = _S24;
    tangent_space_0.xy = _S24.xy * float2(normal_scale_1) ;

#line 3810
    float3 _S25 = normalize(tangent_space_0);

#line 3810
    tangent_space_0 = _S25;
    return normalize(float3(_S25.x)  * (&basis_5)->tangent_1 + float3(_S25.y)  * (&basis_5)->bitangent_0 + float3(_S25.z)  * (&basis_5)->normal_0);
}


#line 2369
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 2380
    float3 _S26;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 2381
        _S26 = - facet_1;

#line 2381
    }
    else
    {

#line 2381
        _S26 = facet_1;

#line 2381
    }

#line 2381
    return _S26;
}


#line 907
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 3375
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_4)
{
    uint _S27 = max(kernelContext_4->frame_0->cluster_grid_0.x, 1U);
    uint _S28 = max(kernelContext_4->frame_0->cluster_grid_0.y, 1U);
    uint _S29 = max(kernelContext_4->frame_0->cluster_grid_0.z, 1U);
    uint _S30 = max(kernelContext_4->frame_0->cluster_grid_0.w, 1U);

#line 3385
    uint _S31 = uint(pixel_0.x) / _S30;

#line 3385
    uint _S32 = min(_S31, _S27 - 1U);
    uint _S33 = uint(pixel_0.y) / _S30;

    float scale_0 = 24.0f / log2(10000.0f);

#line 3396
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S29 - 1U))) * _S28 + min(_S33, _S28 - 1U)) * _S27 + _S32;
}


#line 1801
struct TableTap_0
{
    int2 lo_0;
    int2 hi_0;
    float2 weight_0;
};


#line 1822
TableTap_0 table_tap_0(float n_dot_v_0, float roughness_1, KernelContext_0 thread* kernelContext_5)
{

#line 1822
    texture2d<float, access::sample> _S34 = kernelContext_5->specular_dfg_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S34).get_width(0)),(*((&height_1)) = (_S34).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1828
    float2 _S35 = float2(1.0f) ;
    float2 _S36 = extent_1 - _S35;

#line 1829
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S36);
    float2 high_1 = min(low_1 + _S35, _S36);

    thread TableTap_0 tap_0;
    (&tap_0)->lo_0 = int2(low_1);
    (&tap_0)->hi_0 = int2(high_1);
    (&tap_0)->weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );
    return tap_0;
}


#line 1847
float2 decode_dfg_pair_0(float4 texel_0)
{
    return float2(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / float2(65535.0f) ;
}


#line 1859
float2 dfg_at_0(const TableTap_0 thread* tap_1, KernelContext_0 thread* kernelContext_6)
{
    int _S37 = tap_1->lo_0.x;

#line 1861
    int _S38 = tap_1->lo_0.y;

#line 1861
    int3 _S39 = int3(_S37, _S38, int(0));
    int _S40 = tap_1->hi_0.x;

#line 1862
    int3 _S41 = int3(_S40, _S38, int(0));
    float2 _S42 = float2(tap_1->weight_0.x) ;
    int _S43 = tap_1->hi_0.y;

#line 1864
    int3 _S44 = int3(_S37, _S43, int(0));
    int3 _S45 = int3(_S40, _S43, int(0));

    return mix(mix(decode_dfg_pair_0(((kernelContext_6->specular_dfg_0).read(vec<uint,2>(((_S39)).xy), uint(((_S39)).z)))), decode_dfg_pair_0(((kernelContext_6->specular_dfg_0).read(vec<uint,2>(((_S41)).xy), uint(((_S41)).z)))), _S42), mix(decode_dfg_pair_0(((kernelContext_6->specular_dfg_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)))), decode_dfg_pair_0(((kernelContext_6->specular_dfg_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z)))), _S42), float2(tap_1->weight_0.y) );
}


#line 3326
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}


#line 3342
float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 3354
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 3361
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 2188
void rect_corners_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, array<float3, int(4)> thread* corners_0)
{

#line 2188
    float4 _S46 = float4(light_0->tangent_0) ;

    float3 _S47 = _S46.xyz;

#line 2190
    float3 across_0 = _S47 * float3(_S46.w) ;

#line 2190
    float4 _S48 = float4(light_0->direction_0) ;
    float3 down_0 = cross(_S47, _S48.xyz) * float3(_S48.w) ;
    float3 centre_0 = (float4(light_0->position_0) ).xyz - world_position_3;
    float3 _S49 = centre_0 - across_0;

#line 2193
    (*corners_0)[int(0)] = _S49 - down_0;
    float3 _S50 = centre_0 + across_0;

#line 2194
    (*corners_0)[int(1)] = _S50 - down_0;
    (*corners_0)[int(2)] = _S50 + down_0;
    (*corners_0)[int(3)] = _S49 + down_0;
    return;
}


#line 1946
matrix<float,int(3),int(3)>  ltc_shading_frame_0(float3 normal_5, float3 to_eye_0, float n_dot_v_1)
{
    float3 across_1 = to_eye_0 - normal_5 * float3(n_dot_v_1) ;
    float span_0 = length(across_1);

#line 1949
    float3 seed_0;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {

#line 1950
        seed_0 = float3(0.0f, 0.0f, 1.0f);

#line 1950
    }
    else
    {

#line 1950
        seed_0 = float3(1.0f, 0.0f, 0.0f);

#line 1950
    }

#line 1950
    float3 tangent_5;
    if(span_0 > 0.00009999999747379f)
    {

#line 1951
        tangent_5 = across_1 / float3(span_0) ;

#line 1951
    }
    else
    {

#line 1951
        tangent_5 = normalize(cross(seed_0, normal_5));

#line 1951
    }

    return matrix<float,int(3),int(3)> (tangent_5, cross(normal_5, tangent_5), normal_5);
}


#line 1927
struct LtcPolygon_0
{
    array<float3, int(5)> corner_0;
    int count_0;
};


#line 2017
LtcPolygon_0 ltc_clip_0(const LtcPolygon_0 thread* polygon_0)
{

#line 2017
    float3 _S51 = polygon_0->corner_0[int(0)];

#line 2017
    float3 _S52 = polygon_0->corner_0[int(1)];

#line 2017
    float3 _S53 = polygon_0->corner_0[int(2)];

#line 2017
    float3 _S54 = polygon_0->corner_0[int(3)];

#line 2023
    float3 _S55 = float3(0.0f, 0.0f, 0.0f);


    float _S56 = polygon_0->corner_0[int(0)].z;

#line 2026
    int count_1;

#line 2026
    if(_S56 > 0.0f)
    {

#line 2026
        count_1 = int(1);

#line 2026
    }
    else
    {

#line 2026
        count_1 = int(0);

#line 2026
    }
    float _S57 = _S52.z;

#line 2027
    int _S58;

#line 2027
    if(_S57 > 0.0f)
    {

#line 2027
        _S58 = int(2);

#line 2027
    }
    else
    {

#line 2027
        _S58 = int(0);

#line 2027
    }

#line 2027
    int config_0 = count_1 + _S58;
    float _S59 = _S53.z;

#line 2028
    if(_S59 > 0.0f)
    {

#line 2028
        count_1 = int(4);

#line 2028
    }
    else
    {

#line 2028
        count_1 = int(0);

#line 2028
    }

#line 2028
    int config_1 = config_0 + count_1;
    float _S60 = _S54.z;

#line 2029
    if(_S60 > 0.0f)
    {

#line 2029
        count_1 = int(8);

#line 2029
    }
    else
    {

#line 2029
        count_1 = int(0);

#line 2029
    }

#line 2029
    int config_2 = config_1 + count_1;

#line 2029
    float3 l0_0;

#line 2029
    float3 l1_0;

#line 2029
    float3 l2_0;

#line 2029
    float3 l3_0;

#line 2029
    float3 l4_0;


    if(config_2 == int(1))
    {

#line 2032
        float3 _S61 = float3(_S56) ;


        float3 _S62 = float3(- _S57)  * _S51 + _S61 * _S52;
        float3 _S63 = float3(- _S60)  * _S51 + _S61 * _S54;

#line 2036
        count_1 = int(3);

#line 2036
        l0_0 = _S51;

#line 2036
        l1_0 = _S62;

#line 2036
        l2_0 = _S63;

#line 2036
        l3_0 = _S54;

#line 2036
        l4_0 = _S55;

#line 2032
    }
    else
    {



        if(config_2 == int(2))
        {

#line 2038
            float3 _S64 = float3(_S57) ;


            float3 _S65 = float3(- _S56)  * _S52 + _S64 * _S51;
            float3 _S66 = float3(- _S59)  * _S52 + _S64 * _S53;

#line 2042
            count_1 = int(3);

#line 2042
            l0_0 = _S65;

#line 2042
            l1_0 = _S52;

#line 2042
            l2_0 = _S66;

#line 2042
            l3_0 = _S54;

#line 2042
            l4_0 = _S55;

#line 2038
        }
        else
        {



            if(config_2 == int(3))
            {

                float3 _S67 = float3(- _S59)  * _S52 + float3(_S57)  * _S53;
                float3 _S68 = float3(- _S60)  * _S51 + float3(_S56)  * _S54;

#line 2048
                count_1 = int(4);

#line 2048
                l0_0 = _S51;

#line 2048
                l1_0 = _S52;

#line 2048
                l2_0 = _S67;

#line 2048
                l3_0 = _S68;

#line 2048
                l4_0 = _S55;

#line 2044
            }
            else
            {



                if(config_2 == int(4))
                {

#line 2050
                    float3 _S69 = float3(_S59) ;


                    float3 _S70 = float3(- _S60)  * _S53 + _S69 * _S54;
                    float3 _S71 = float3(- _S57)  * _S53 + _S69 * _S52;

#line 2054
                    count_1 = int(3);

#line 2054
                    l0_0 = _S70;

#line 2054
                    l1_0 = _S71;

#line 2054
                    l2_0 = _S53;

#line 2054
                    l3_0 = _S54;

#line 2054
                    l4_0 = _S55;

#line 2050
                }
                else
                {



                    if(config_2 == int(6))
                    {

                        float3 _S72 = float3(- _S56)  * _S52 + float3(_S57)  * _S51;
                        float3 _S73 = float3(- _S60)  * _S53 + float3(_S59)  * _S54;

#line 2060
                        count_1 = int(4);

#line 2060
                        l0_0 = _S72;

#line 2060
                        l1_0 = _S52;

#line 2060
                        l2_0 = _S53;

#line 2060
                        l3_0 = _S73;

#line 2060
                        l4_0 = _S55;

#line 2056
                    }
                    else
                    {



                        if(config_2 == int(7))
                        {

#line 2062
                            float3 _S74 = float3(- _S60) ;


                            float3 _S75 = _S74 * _S51 + float3(_S56)  * _S54;
                            float3 _S76 = _S74 * _S53 + float3(_S59)  * _S54;

#line 2066
                            count_1 = int(5);

#line 2066
                            l0_0 = _S51;

#line 2066
                            l1_0 = _S52;

#line 2066
                            l2_0 = _S53;

#line 2066
                            l3_0 = _S76;

#line 2066
                            l4_0 = _S75;

#line 2062
                        }
                        else
                        {



                            if(config_2 == int(8))
                            {

#line 2068
                                float3 _S77 = float3(_S60) ;


                                float3 _S78 = float3(- _S56)  * _S54 + _S77 * _S51;
                                float3 _S79 = float3(- _S59)  * _S54 + _S77 * _S53;

#line 2072
                                count_1 = int(3);

#line 2072
                                l0_0 = _S78;

#line 2072
                                l1_0 = _S79;

#line 2072
                                l2_0 = _S54;

#line 2072
                                l3_0 = _S54;

#line 2072
                                l4_0 = _S55;

#line 2068
                            }
                            else
                            {

#line 2075
                                if(config_2 == int(9))
                                {

                                    float3 _S80 = float3(- _S57)  * _S51 + float3(_S56)  * _S52;
                                    float3 _S81 = float3(- _S59)  * _S54 + float3(_S60)  * _S53;

#line 2079
                                    count_1 = int(4);

#line 2079
                                    l0_0 = _S51;

#line 2079
                                    l1_0 = _S80;

#line 2079
                                    l2_0 = _S81;

#line 2079
                                    l3_0 = _S54;

#line 2079
                                    l4_0 = _S55;

#line 2075
                                }
                                else
                                {



                                    if(config_2 == int(11))
                                    {


                                        float3 _S82 = float3(- _S60)  * _S53 + float3(_S59)  * _S54;
                                        float3 _S83 = float3(- _S59)  * _S52 + float3(_S57)  * _S53;

#line 2086
                                        count_1 = int(5);

#line 2086
                                        l0_0 = _S51;

#line 2086
                                        l1_0 = _S52;

#line 2086
                                        l2_0 = _S83;

#line 2086
                                        l3_0 = _S82;

#line 2086
                                        l4_0 = _S54;

#line 2081
                                    }
                                    else
                                    {

#line 2088
                                        if(config_2 == int(12))
                                        {

                                            float3 _S84 = float3(- _S57)  * _S53 + float3(_S59)  * _S52;
                                            float3 _S85 = float3(- _S56)  * _S54 + float3(_S60)  * _S51;

#line 2092
                                            count_1 = int(4);

#line 2092
                                            l0_0 = _S85;

#line 2092
                                            l1_0 = _S84;

#line 2092
                                            l2_0 = _S53;

#line 2092
                                            l3_0 = _S54;

#line 2092
                                            l4_0 = _S55;

#line 2088
                                        }
                                        else
                                        {



                                            if(config_2 == int(13))
                                            {



                                                float3 _S86 = float3(- _S59)  * _S52 + float3(_S57)  * _S53;
                                                float3 _S87 = float3(- _S57)  * _S51 + float3(_S56)  * _S52;

#line 2100
                                                count_1 = int(5);

#line 2100
                                                l0_0 = _S51;

#line 2100
                                                l1_0 = _S87;

#line 2100
                                                l2_0 = _S86;

#line 2100
                                                l3_0 = _S53;

#line 2100
                                                l4_0 = _S54;

#line 2094
                                            }
                                            else
                                            {

#line 2102
                                                if(config_2 == int(14))
                                                {

#line 2102
                                                    float3 _S88 = float3(- _S56) ;


                                                    float3 _S89 = _S88 * _S54 + float3(_S60)  * _S51;
                                                    float3 _S90 = _S88 * _S52 + float3(_S57)  * _S51;

#line 2106
                                                    count_1 = int(5);

#line 2106
                                                    l0_0 = _S90;

#line 2106
                                                    l1_0 = _S89;

#line 2102
                                                }
                                                else
                                                {



                                                    if(config_2 == int(15))
                                                    {

#line 2108
                                                        count_1 = int(4);

#line 2108
                                                    }
                                                    else
                                                    {

#line 2108
                                                        count_1 = int(0);

#line 2108
                                                    }

#line 2108
                                                    l0_0 = _S51;

#line 2108
                                                    l1_0 = _S55;

#line 2102
                                                }

#line 2023
                                                float3 _S91 = l1_0;

#line 2023
                                                l1_0 = _S52;

#line 2023
                                                l2_0 = _S53;

#line 2023
                                                l3_0 = _S54;

#line 2023
                                                l4_0 = _S91;

#line 2094
                                            }

#line 2088
                                        }

#line 2081
                                    }

#line 2075
                                }

#line 2068
                            }

#line 2062
                        }

#line 2056
                    }

#line 2050
                }

#line 2044
            }

#line 2038
        }

#line 2032
    }

#line 2116
    if(count_1 <= int(3))
    {

#line 2116
        l3_0 = l0_0;

#line 2116
        l4_0 = l0_0;

#line 2116
    }
    else
    {


        if(count_1 == int(4))
        {

#line 2121
            l4_0 = l0_0;

#line 2121
        }

#line 2116
    }

#line 2126
    thread LtcPolygon_0 clipped_0;
    (&clipped_0)->corner_0[int(0)] = l0_0;
    (&clipped_0)->corner_0[int(1)] = l1_0;
    (&clipped_0)->corner_0[int(2)] = l2_0;
    (&clipped_0)->corner_0[int(3)] = l3_0;
    (&clipped_0)->corner_0[int(4)] = l4_0;
    (&clipped_0)->count_0 = count_1;
    return clipped_0;
}


#line 1989
float ltc_edge_0(float3 first_0, float3 second_0)
{
    float cosine_0 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    float y_0 = abs(cosine_0);


    float fit_0 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);

#line 1995
    float weight_1;

#line 2000
    if(cosine_0 > 0.0f)
    {

#line 2000
        weight_1 = fit_0;

#line 2000
    }
    else
    {

#line 2000
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;

#line 2000
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}


#line 2146
float ltc_irradiance_0(matrix<float,int(3),int(3)>  transform_1, const array<float3, int(4)> thread* corners_1)
{
    thread LtcPolygon_0 polygon_1;

#line 2148
    int corner_1 = int(0);
    for(;;)
    {

#line 2149
        if(corner_1 < int(4))
        {
        }
        else
        {

#line 2149
            break;
        }
        (&polygon_1)->corner_0[corner_1] = ((((*corners_1)[corner_1]) * (transform_1)));

#line 2149
        corner_1 = corner_1 + int(1);

#line 2149
    }



    (&polygon_1)->corner_0[int(4)] = float3(0.0f, 0.0f, 0.0f);
    (&polygon_1)->count_0 = int(4);

#line 2154
    thread LtcPolygon_0 _S92 = polygon_1;

#line 2154
    LtcPolygon_0 _S93 = ltc_clip_0(&_S92);
    polygon_1 = _S93;
    if(((&polygon_1)->count_0) == int(0))
    {
        return 0.0f;
    }

#line 2158
    int at_2 = int(0);

    for(;;)
    {

#line 2160
        if(at_2 < int(5))
        {
        }
        else
        {

#line 2160
            break;
        }
        (&polygon_1)->corner_0[at_2] = normalize((&polygon_1)->corner_0[at_2]);

#line 2160
        at_2 = at_2 + int(1);

#line 2160
    }

#line 2167
    float sum_0 = ltc_edge_0((&polygon_1)->corner_0[int(0)], (&polygon_1)->corner_0[int(1)]) + ltc_edge_0((&polygon_1)->corner_0[int(1)], (&polygon_1)->corner_0[int(2)]) + ltc_edge_0((&polygon_1)->corner_0[int(2)], (&polygon_1)->corner_0[int(3)]);

#line 2167
    float sum_1;
    if(((&polygon_1)->count_0) >= int(4))
    {

#line 2168
        sum_1 = sum_0 + ltc_edge_0((&polygon_1)->corner_0[int(3)], (&polygon_1)->corner_0[int(4)]);

#line 2168
    }
    else
    {

#line 2168
        sum_1 = sum_0;

#line 2168
    }



    if(((&polygon_1)->count_0) == int(5))
    {

#line 2172
        sum_1 = sum_1 + ltc_edge_0((&polygon_1)->corner_0[int(4)], (&polygon_1)->corner_0[int(0)]);

#line 2172
    }

#line 2179
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}


#line 1875
float4 ltc_at_0(const TableTap_0 thread* tap_2, KernelContext_0 thread* kernelContext_7)
{
    int _S94 = tap_2->lo_0.x;

#line 1877
    int _S95 = tap_2->lo_0.y;

#line 1877
    int3 _S96 = int3(_S94, _S95, int(0));
    int _S97 = tap_2->hi_0.x;

#line 1878
    int3 _S98 = int3(_S97, _S95, int(0));
    float4 _S99 = float4(tap_2->weight_0.x) ;
    int _S100 = tap_2->hi_0.y;

#line 1880
    int3 _S101 = int3(_S94, _S100, int(0));
    int3 _S102 = int3(_S97, _S100, int(0));

    return mix(mix(((kernelContext_7->ltc_matrix_0).read(vec<uint,2>(((_S96)).xy), uint(((_S96)).z))), ((kernelContext_7->ltc_matrix_0).read(vec<uint,2>(((_S98)).xy), uint(((_S98)).z))), _S99), mix(((kernelContext_7->ltc_matrix_0).read(vec<uint,2>(((_S101)).xy), uint(((_S101)).z))), ((kernelContext_7->ltc_matrix_0).read(vec<uint,2>(((_S102)).xy), uint(((_S102)).z))), _S99), float4(tap_2->weight_0.y) );
}


#line 1962
matrix<float,int(3),int(3)>  ltc_transform_0(float4 entry_0)
{
    return matrix<float,int(3),int(3)> (entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}


#line 1757
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_2, float n_dot_h_0, float v_dot_h_0)
{

#line 1764
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1771
    float _S103 = 1.0f - alpha2_0;

#line 1776
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S103 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S103 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2420
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_1 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}


#line 2735
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


#line 2851
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_8)
{
    float2 texel_1 = kernelContext_8->frame_0->shadow_params_0.xy;
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S104 = float2(0.5f, 0.5f) * texel_1 * grid_0;


    float2 _S105 = float2(1.0f, 1.0f);

#line 2858
    float2 _S106 = _S105 / texel_1;

#line 2858
    uint index_0 = 0U;

#line 2858
    float sum_2 = 0.0f;

#line 2858
    float found_0 = 0.0f;



    for(;;)
    {

#line 2862
        if(index_0 < 16U)
        {
        }
        else
        {

#line 2862
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_0] * float2(8.0f) ;
        float _S107 = spoke_0.x;

#line 2865
        float _S108 = rotation_0.x;

#line 2865
        float _S109 = spoke_0.y;

#line 2865
        float _S110 = rotation_0.y;

#line 2874
        int3 _S111 = int3(int2(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + float2(_S107 * _S108 - _S109 * _S110, _S107 * _S110 + _S109 * _S108) * texel_1 * grid_0, _S104, float2(1.0f)  - _S104)) * _S106, _S106 - _S105)), int(0));

#line 2874
        float depth_1 = ((kernelContext_8->shadow_atlas_0).read(vec<uint,2>(((_S111)).xy), uint(((_S111)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 2878
            sum_2 = sum_2 + depth_1;

#line 2878
            found_0 = found_1;

#line 2875
        }

#line 2862
        index_0 = index_0 + 1U;

#line 2862
    }

#line 2882
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 2893
    float _S112 = 2.0f * kernelContext_8->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_2 / found_0 - reference_0) * (_S112 + 40.0f) * 0.01999999955296516f / (_S112 / 768.0f), 2.0f, 8.0f);
}


#line 2753
float tile_tap_0(uint tile_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_9)
{
    float2 texel_2 = kernelContext_9->frame_0->shadow_params_0.xy;

#line 2760
    float2 grid_1 = float2(4.0f, 4.0f);
    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_2 * grid_1;

    float _S113 = spoke_1.x;

#line 2763
    float _S114 = rotation_1.x;

#line 2763
    float _S115 = spoke_1.y;

#line 2763
    float _S116 = rotation_1.y;


    float _S117 = ((kernelContext_9->shadow_atlas_0).sample_compare((kernelContext_9->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + float2(_S113 * _S114 - _S115 * _S116, _S113 * _S116 + _S115 * _S114) * texel_2 * grid_1, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2766
    return _S117;
}


#line 2788
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_2, KernelContext_0 thread* kernelContext_10)
{
    float2 _S118 = shadow_rotation_0(pixel_2);

#line 2790
    uint spot_0 = 0U;

#line 2790
    float probe_0 = 0.0f;


    for(;;)
    {

#line 2793
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2793
            break;
        }

#line 2793
        float _S119 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S118, reference_2, kernelContext_10);

        float probe_1 = probe_0 + _S119;

#line 2793
        spot_0 = spot_0 + 1U;

#line 2793
        probe_0 = probe_1;

#line 2793
    }

#line 2802
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2808
    uint index_1 = 0U;

#line 2808
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2812
        if(index_1 < 32U)
        {
        }
        else
        {

#line 2812
            break;
        }

#line 2812
        float _S120 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[index_1] * float2(radius_2) , _S118, reference_2, kernelContext_10);

        float visibility_1 = visibility_0 + _S120;

#line 2812
        index_1 = index_1 + 1U;

#line 2812
        visibility_0 = visibility_1;

#line 2812
    }



    return visibility_0 / 32.0f;
}


#line 2947
float cascade_visibility_0(uint cascade_1, float3 world_position_4, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_11)
{

#line 2978
    float texel_world_0 = 2.0f * kernelContext_11->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 2985
    float4 clip_0 = (((float4(world_position_4 + geometric_normal_1 * float3((texel_world_0 * kernelContext_11->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_11->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_11->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 2989
    bool _S121;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 2990
        _S121 = true;

#line 2990
    }
    else
    {

#line 2990
        _S121 = (ndc_0.z) <= 0.0f;

#line 2990
    }

#line 2990
    if(_S121)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 3017
    float _S122 = ndc_0.z;

#line 3017
    float _S123 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S122, shadow_rotation_0(pixel_3), kernelContext_11);

#line 3017
    float _S124 = tile_pcf_0(cascade_1, tile_uv_4, _S122, pixel_3, _S123, kernelContext_11);
    return _S124;
}


#line 3034
float sun_visibility_0(float3 world_position_5, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_12)
{

#line 3035
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 3047
    float eye_distance_0 = length(world_position_5 - kernelContext_12->frame_0->camera_position_0.xyz);

#line 3047
    uint index_2 = 0U;

    for(;;)
    {

#line 3049
        if(index_2 < 2U)
        {
        }
        else
        {

#line 3049
            cascade_2 = 1U;

#line 3049
            break;
        }
        if(eye_distance_0 < kernelContext_12->frame_0->cascade_far_0[index_2])
        {

#line 3051
            cascade_2 = index_2;


            break;
        }

#line 3049
        index_2 = index_2 + 1U;

#line 3049
    }

#line 3049
    float _S125 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_12);

#line 3060
    uint _S126 = cascade_2 + 1U;

#line 3060
    if(_S126 >= 2U)
    {



        return _S125;
    }

#line 3073
    float band_0 = kernelContext_12->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_12->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S125;
    }

#line 3077
    float _S127 = cascade_visibility_0(_S126, world_position_5, to_light_3, geometric_normal_2, pixel_4, kernelContext_12);

#line 3088
    return mix(_S125, _S127, blend_0);
}


#line 3278
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S128 = axis_2.x;

#line 3281
    float _S129 = axis_2.y;

#line 3281
    bool _S130;

#line 3281
    if(_S128 >= _S129)
    {

#line 3281
        _S130 = _S128 >= (axis_2.z);

#line 3281
    }
    else
    {

#line 3281
        _S130 = false;

#line 3281
    }

#line 3281
    uint _S131;

#line 3281
    if(_S130)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 3283
            _S131 = 0U;

#line 3283
        }
        else
        {

#line 3283
            _S131 = 1U;

#line 3283
        }

#line 3283
        return _S131;
    }
    if(_S129 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 3287
            _S131 = 2U;

#line 3287
        }
        else
        {

#line 3287
            _S131 = 3U;

#line 3287
        }

#line 3287
        return _S131;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 3289
        _S131 = 4U;

#line 3289
    }
    else
    {

#line 3289
        _S131 = 5U;

#line 3289
    }

#line 3289
    return _S131;
}


#line 308
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 3191
float punctual_visibility_0(uint tile_4, float3 world_position_6, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_13)
{

#line 3203
    float4 clip_1 = (((float4(world_position_6 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_13->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 3210
    float _S132 = clip_1.w;

#line 3210
    if(_S132 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S132) ;

#line 3214
    bool _S133;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 3215
        _S133 = true;

#line 3215
    }
    else
    {

#line 3215
        _S133 = (ndc_1.z) <= 0.0f;

#line 3215
    }

#line 3215
    if(_S133)
    {

#line 3215
        _S133 = true;

#line 3215
    }
    else
    {

#line 3215
        _S133 = (ndc_1.z) > 1.0f;

#line 3215
    }

#line 3215
    if(_S133)
    {

#line 3222
        return 1.0f;
    }

#line 3222
    float _S134 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_13);

#line 3232
    return _S134;
}


#line 3297
float point_visibility_0(const GpuLight_natural_0 thread* light_1, uint base_1, float3 world_position_7, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_14)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_7 - (float4(light_1->position_0) ).xyz;

#line 3305
    float _S135 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_14);

#line 3311
    return _S135;
}


#line 3239
float spot_visibility_0(const GpuLight_natural_0 thread* light_2, uint tile_5, float3 world_position_8, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_15)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 3246
    float4 _S136 = float4(light_2->direction_0) ;

#line 3253
    float cos_outer_1 = _S136.w;

#line 3253
    float _S137 = punctual_visibility_0(tile_5, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (float4(light_2->position_0) ).xyz, normalize(_S136.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_15);

#line 3260
    return _S137;
}


#line 1903
float3 specular_compensation_0(float3 f0_1, float directional_albedo_0)
{


    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 3504
float3 sky_irradiance_0(float3 normal_6, KernelContext_0 thread* kernelContext_16)
{
    float4 basis_6 = float4(normal_6, 1.0f);
    return max(float3(dot(kernelContext_16->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_16->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_16->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 959
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 3406
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_17)
{

    GpuProbe_natural_0 _S138 = kernelContext_17->probes_0[min((cell_1.z * kernelContext_17->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_17->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_17->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 3409
    GpuProbe_0 _S139 = { float4(_S138.sh_r_0) , float4(_S138.sh_g_0) , float4(_S138.sh_b_0)  };

#line 3409
    return _S139;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_1, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S140 = float4(t_1) ;

#line 3417
    (&blended_0)->sh_r_0 = mix(a_1->sh_r_0, b_0->sh_r_0, _S140);
    (&blended_0)->sh_g_0 = mix(a_1->sh_g_0, b_0->sh_g_0, _S140);
    (&blended_0)->sh_b_0 = mix(a_1->sh_b_0, b_0->sh_b_0, _S140);
    return blended_0;
}


#line 3457
float3 probe_irradiance_0(float3 world_position_9, float3 normal_7, KernelContext_0 thread* kernelContext_18)
{

#line 3457
    float3 _S141 = float3(1.0f) ;

#line 3462
    float3 _S142 = float3(0.0f, 0.0f, 0.0f);

#line 3462
    float3 last_0 = max(float3(kernelContext_18->frame_0->probe_counts_0.xyz) - _S141, _S142);
    float3 grid_2 = clamp((world_position_9 - kernelContext_18->frame_0->probe_origin_0.xyz) * kernelContext_18->frame_0->probe_inv_spacing_0.xyz, _S142, last_0);

    float3 base_2 = floor(grid_2);
    float3 f_0 = grid_2 - base_2;

    uint3 _S143 = uint3(base_2);



    uint3 _S144 = uint3(min(base_2 + _S141, last_0));

#line 3479
    uint _S145 = _S143.x;

#line 3479
    uint _S146 = _S143.y;

#line 3479
    uint _S147 = _S143.z;

#line 3479
    GpuProbe_0 _S148 = probe_at_0(uint3(_S145, _S146, _S147), kernelContext_18);

#line 3479
    uint _S149 = _S144.x;

#line 3479
    GpuProbe_0 _S150 = probe_at_0(uint3(_S149, _S146, _S147), kernelContext_18);

#line 3479
    float _S151 = f_0.x;

#line 3479
    thread GpuProbe_0 _S152 = _S148;

#line 3479
    thread GpuProbe_0 _S153 = _S150;

#line 3479
    GpuProbe_0 _S154 = lerp_probe_0(&_S152, &_S153, _S151);
    uint _S155 = _S144.y;

#line 3480
    GpuProbe_0 _S156 = probe_at_0(uint3(_S145, _S155, _S147), kernelContext_18);

#line 3480
    GpuProbe_0 _S157 = probe_at_0(uint3(_S149, _S155, _S147), kernelContext_18);

#line 3480
    thread GpuProbe_0 _S158 = _S156;

#line 3480
    thread GpuProbe_0 _S159 = _S157;

#line 3480
    GpuProbe_0 _S160 = lerp_probe_0(&_S158, &_S159, _S151);
    uint _S161 = _S144.z;

#line 3481
    GpuProbe_0 _S162 = probe_at_0(uint3(_S145, _S146, _S161), kernelContext_18);

#line 3481
    GpuProbe_0 _S163 = probe_at_0(uint3(_S149, _S146, _S161), kernelContext_18);

#line 3481
    thread GpuProbe_0 _S164 = _S162;

#line 3481
    thread GpuProbe_0 _S165 = _S163;

#line 3481
    GpuProbe_0 _S166 = lerp_probe_0(&_S164, &_S165, _S151);

#line 3481
    GpuProbe_0 _S167 = probe_at_0(uint3(_S145, _S155, _S161), kernelContext_18);

#line 3481
    GpuProbe_0 _S168 = probe_at_0(uint3(_S149, _S155, _S161), kernelContext_18);

#line 3481
    thread GpuProbe_0 _S169 = _S167;

#line 3481
    thread GpuProbe_0 _S170 = _S168;

#line 3481
    GpuProbe_0 _S171 = lerp_probe_0(&_S169, &_S170, _S151);

    float _S172 = f_0.y;

#line 3483
    thread GpuProbe_0 _S173 = _S154;

#line 3483
    thread GpuProbe_0 _S174 = _S160;

#line 3483
    GpuProbe_0 _S175 = lerp_probe_0(&_S173, &_S174, _S172);

#line 3483
    thread GpuProbe_0 _S176 = _S166;

#line 3483
    thread GpuProbe_0 _S177 = _S171;

#line 3483
    GpuProbe_0 _S178 = lerp_probe_0(&_S176, &_S177, _S172);

    float _S179 = f_0.z;

#line 3485
    thread GpuProbe_0 _S180 = _S175;

#line 3485
    thread GpuProbe_0 _S181 = _S178;

#line 3485
    GpuProbe_0 _S182 = lerp_probe_0(&_S180, &_S181, _S179);

    float4 basis_7 = float4(normal_7, 1.0f);
    return max(float3(dot(_S182.sh_r_0, basis_7), dot(_S182.sh_g_0, basis_7), dot(_S182.sh_b_0, basis_7)), _S142);
}


#line 932
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 2254
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S183 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 2262
    float kernel_0 = 0.0001984127011383f;

#line 2262
    int term_0 = int(6);

    for(;;)
    {

#line 2264
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 2264
            break;
        }
        float _S184 = kernel_0 * _S183 + FOG_KERNEL_0[term_0];

#line 2264
        int term_1 = term_0 - int(1);

#line 2264
        kernel_0 = _S184;

#line 2264
        term_0 = term_1;

#line 2264
    }

#line 2271
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 2281
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S185 = - d_0;

#line 2285
        float series_0 = 0.00833333376795053f;

#line 2285
        int term_2 = int(3);

        for(;;)
        {

#line 2287
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 2287
                break;
            }
            float _S186 = series_0 * _S185 + FOG_RATIO_KERNEL_0[term_2];

#line 2287
            int term_3 = term_2 - int(1);

#line 2287
            series_0 = _S186;

#line 2287
            term_2 = term_3;

#line 2287
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 2315
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_2)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }

#line 2326
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 2334
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 3530
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 3530
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
    [[flat]] uint frame_2 [[user(TEXCOORD_5)]];
};


#line 3869
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S187 [[stage_in]], float4 position_4 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_1 [[texture(3)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_1 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 3869
    thread KernelContext_0 kernelContext_19;

#line 3869
    (&kernelContext_19)->draw_0 = draw_1;

#line 3869
    (&kernelContext_19)->visible_instances_0 = visible_instances_1;

#line 3869
    (&kernelContext_19)->instances_0 = instances_1;

#line 3869
    (&kernelContext_19)->meshes_0 = meshes_1;

#line 3869
    (&kernelContext_19)->frame_0 = frame_3;

#line 3869
    (&kernelContext_19)->vertices_0 = vertices_1;

#line 3869
    (&kernelContext_19)->ambient_occlusion_0 = ambient_occlusion_1;

#line 3869
    (&kernelContext_19)->materials_0 = materials_1;

#line 3869
    (&kernelContext_19)->normal_textures_0 = normal_textures_1;

#line 3869
    (&kernelContext_19)->base_color_sampler_0 = base_color_sampler_1;

#line 3869
    (&kernelContext_19)->base_color_textures_0 = base_color_textures_1;

#line 3869
    (&kernelContext_19)->cluster_lights_0 = cluster_lights_1;

#line 3869
    (&kernelContext_19)->specular_dfg_0 = specular_dfg_1;

#line 3869
    (&kernelContext_19)->lights_0 = lights_1;

#line 3869
    (&kernelContext_19)->ltc_matrix_0 = ltc_matrix_1;

#line 3869
    (&kernelContext_19)->shadow_atlas_0 = shadow_atlas_1;

#line 3869
    (&kernelContext_19)->shadow_sampler_0 = shadow_sampler_1;

#line 3869
    (&kernelContext_19)->probes_0 = probes_1;

#line 3881
    float3 vertex_normal_0 = normalize(_S187.world_normal_1);

#line 3886
    float2 motion_1 = motion_vector_0(_S187.clip_position_1, _S187.previous_clip_position_1);

#line 3895
    if((frame_3->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 3937
    if((frame_3->ambient_0.w) >= 3.5f)
    {

#line 3937
        float _S188 = occlusion_at_0(position_4.xy, &kernelContext_19);

        thread FragmentOutput_0 occlusion_0;

#line 3948
        (&occlusion_0)->lit_0 = float4(_S188, _S188, _S188, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_3->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S187.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 3965
    thread GpuMaterial_natural_0 _S189 = (&kernelContext_19)->materials_0[_S187.material_5];

#line 3965
    float2 uv_3;

#line 3990
    if(((&_S189)->tiling_0) == 1U)
    {

#line 3990
        uv_3 = physical_tile_uv_0(_S187.world_position_10, vertex_normal_0, (&_S189)->tile_metres_0);

#line 3990
    }
    else
    {

#line 3990
        uv_3 = _S187.uv_2;

#line 3990
    }

#line 3990
    uint _S190 = normal_layer_0(&_S189);

#line 3990
    thread VertexOutput_0 _S191;

#line 3990
    (&_S191)->position_3 = position_4;

#line 3990
    (&_S191)->world_position_1 = _S187.world_position_10;

#line 3990
    (&_S191)->world_normal_0 = _S187.world_normal_1;

#line 3990
    (&_S191)->color_2 = _S187.color_3;

#line 3990
    (&_S191)->material_2 = _S187.material_5;

#line 3990
    (&_S191)->uv_0 = _S187.uv_2;

#line 3990
    (&_S191)->clip_position_0 = _S187.clip_position_1;

#line 3990
    (&_S191)->previous_clip_position_0 = _S187.previous_clip_position_1;

#line 3990
    (&_S191)->world_tangent_0 = _S187.world_tangent_1;

#line 3990
    (&_S191)->frame_1 = _S187.frame_2;

#line 3990
    float3 _S192 = shading_normal_of_0(_S190, (&_S189)->normal_scale_0, &_S191, vertex_normal_0, uv_3, &kernelContext_19);

#line 3997
    if((frame_3->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 3999
        float3 _S193 = float3(0.5f) ;

#line 4011
        (&normals_0)->lit_0 = float4(_S192 * _S193 + _S193, 1.0f);

#line 4017
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_1 = normalize((&kernelContext_19)->frame_0->camera_position_0.xyz - _S187.world_position_10);



    float3 _S194 = geometric_normal_of_0(_S187.world_position_10, vertex_normal_0);

#line 4026
    uint _S195 = base_color_layer_0(&_S189);

#line 4041
    float3 _S196 = float3(uv_3, float(_S195));
    float4 albedo_0 = _S187.color_3 * float4((&_S189)->base_color_0)  * (((&kernelContext_19)->base_color_textures_0).sample(((&kernelContext_19)->base_color_sampler_0), ((_S196)).xy, uint(((_S196)).z)));

#line 4048
    float metallic_1 = saturate((&_S189)->metallic_0);
    float roughness_2 = clamp((&_S189)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_2 * roughness_2;
    float _S197 = alpha_0 * alpha_0;

#line 4057
    float3 _S198 = albedo_0.xyz;

#line 4057
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S198, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S198 * float3((1.0f - metallic_1)) ;

#line 4064
    float _S199 = max(dot(_S192, to_eye_1), 0.00009999999747379f);

#line 4074
    float2 _S200 = position_4.xy;

#line 4074
    uint _S201 = froxel_of_0(_S200, (((float4(_S187.world_position_10, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_19);

#line 4074
    uint base_3 = _S201 * 17U;

#line 4079
    uint _S202 = min((&kernelContext_19)->cluster_lights_0[base_3], 16U);

#line 4079
    TableTap_0 _S203 = table_tap_0(_S199, roughness_2, &kernelContext_19);

#line 4079
    thread TableTap_0 _S204 = _S203;

#line 4079
    float2 _S205 = dfg_at_0(&_S204, &kernelContext_19);

#line 4088
    float _S206 = _S205.x;

#line 4088
    float _S207 = _S205.y;

#line 4088
    float3 _S208 = f0_2 * float3(_S206)  + float3(_S207) ;

#line 4094
    float3 _S209 = float3(0.0f, 0.0f, 0.0f);

#line 4094
    uint slot_0 = 0U;

#line 4094
    float3 direct_0 = _S209;

#line 4094
    float3 gloss_0 = _S209;

    for(;;)
    {

#line 4096
        if(slot_0 < _S202)
        {
        }
        else
        {

#line 4096
            break;
        }

#line 4096
        thread GpuLight_natural_0 _S210 = (&kernelContext_19)->lights_0[(&kernelContext_19)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 4096
        uint _S211 = (&_S210)->kind_0;

#line 4105
        bool _S212 = ((&_S210)->kind_0) == 0U;

#line 4105
        float3 to_light_7;

#line 4105
        float reach_0;

#line 4105
        if(_S212)
        {

#line 4105
            to_light_7 = normalize((float4((&_S210)->direction_0) ).xyz);

#line 4105
            reach_0 = 1.0f;

#line 4105
        }
        else
        {


            if(_S211 == 3U)
            {

#line 4110
                float4 _S213 = float4((&_S210)->position_0) ;

#line 4118
                float3 offset_0 = _S213.xyz - _S187.world_position_10;
                float distance_3 = length(offset_0);

                float _S214 = range_window_0(distance_3, _S213.w);

#line 4121
                to_light_7 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;

#line 4121
                reach_0 = _S214;

#line 4110
            }
            else
            {

#line 4110
                float4 _S215 = float4((&_S210)->position_0) ;

#line 4125
                float3 offset_1 = _S215.xyz - _S187.world_position_10;
                float distance_4 = length(offset_1);
                float3 to_light_8 = offset_1 / float3(max(distance_4, 9.99999997475242708e-07f)) ;
                float reach_1 = punctual_falloff_0(distance_4, _S215.w);
                if(_S211 == 2U)
                {

#line 4129
                    float4 _S216 = float4((&_S210)->direction_0) ;

#line 4129
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S216.xyz, _S216.w, (&_S210)->cos_inner_0);

#line 4129
                }
                else
                {

#line 4129
                    reach_0 = reach_1;

#line 4129
                }

#line 4129
                to_light_7 = to_light_8;

#line 4110
            }

#line 4105
        }

#line 4138
        float n_dot_l_5 = dot(_S192, to_light_7);

#line 4138
        float3 specular_0;

#line 4138
        float diffuse_0;


        if(_S211 == 3U)
        {

#line 4151
            thread array<float3, int(4)> corners_2;

#line 4151
            rect_corners_0(&_S210, _S187.world_position_10, &corners_2);

            matrix<float,int(3),int(3)>  to_local_0 = ltc_shading_frame_0(_S192, to_eye_1, _S199);

#line 4153
            thread array<float3, int(4)> _S217 = corners_2;

#line 4153
            float _S218 = ltc_irradiance_0(to_local_0, &_S217);

#line 4153
            thread TableTap_0 _S219 = _S203;

#line 4153
            float4 _S220 = ltc_at_0(&_S219, &kernelContext_19);

            matrix<float,int(3),int(3)>  _S221 = (((to_local_0) * (ltc_transform_0(_S220))));

#line 4155
            thread array<float3, int(4)> _S222 = corners_2;

#line 4155
            float _S223 = ltc_irradiance_0(_S221, &_S222);
            float3 _S224 = float3(_S223)  * _S208;

#line 4156
            diffuse_0 = _S218;

#line 4156
            specular_0 = _S224;

#line 4141
        }
        else
        {

#line 4161
            float _S225 = max(n_dot_l_5, 0.0f);

#line 4168
            float3 half_vector_0 = normalize(to_light_7 + to_eye_1);

#line 4176
            float3 specular_1 = ggx_lobe_0(_S197, f0_2, _S225, _S199, max(dot(_S192, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * float3(_S225) ;

#line 4176
            diffuse_0 = _S225;

#line 4176
            specular_0 = specular_1;

#line 4141
        }

#line 4141
        float3 specular_2;

#line 4184
        if((((&_S210)->flags_3) & 1U) != 0U)
        {

#line 4184
            specular_2 = _S209;

#line 4184
        }
        else
        {

#line 4184
            specular_2 = specular_0;

#line 4184
        }

#line 4184
        float reach_2;

#line 4202
        if(_S212)
        {

#line 4202
            float _S226 = sun_visibility_0(_S187.world_position_10, to_light_7, n_dot_l_5, _S194, _S200, &kernelContext_19);

#line 4202
            reach_2 = _S226;

#line 4202
        }
        else
        {


            if(_S211 == 1U)
            {

#line 4207
                uint _S227 = (&_S210)->shadow_tile_0;

#line 4219
                if(((&_S210)->shadow_tile_0) <= 8U)
                {

#line 4219
                    float _S228 = point_visibility_0(&_S210, _S227, _S187.world_position_10, to_light_7, n_dot_l_5, _S194, _S200, &kernelContext_19);

#line 4219
                    reach_2 = reach_0 * _S228;

#line 4219
                }
                else
                {

#line 4219
                    reach_2 = reach_0;

#line 4219
                }

#line 4207
            }
            else
            {

#line 4207
                uint _S229 = (&_S210)->shadow_tile_0;

#line 4225
                if(((&_S210)->shadow_tile_0) < 14U)
                {

#line 4225
                    float _S230 = spot_visibility_0(&_S210, _S229, _S187.world_position_10, to_light_7, n_dot_l_5, _S194, _S200, &kernelContext_19);

#line 4225
                    reach_2 = reach_0 * _S230;

#line 4225
                }
                else
                {

#line 4225
                    reach_2 = reach_0;

#line 4225
                }

#line 4207
            }

#line 4202
        }

#line 4233
        float3 _S231 = (float4((&_S210)->color_0) ).xyz;

#line 4233
        float3 direct_1 = direct_0 + _S231 * float3((diffuse_0 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S231 * (specular_2 * float3(reach_2) );

#line 4096
        slot_0 = slot_0 + 1U;

#line 4096
        direct_0 = direct_1;

#line 4096
        gloss_0 = gloss_1;

#line 4096
    }

#line 4248
    float3 gloss_2 = gloss_0 * specular_compensation_0(f0_2, _S206 + _S207);

#line 4248
    float _S232 = occlusion_at_0(_S200, &kernelContext_19);

#line 4284
    float3 _S233 = frame_3->ambient_0.xyz;

#line 4284
    float3 _S234 = sky_irradiance_0(_S192, &kernelContext_19);

#line 4284
    float3 _S235 = _S233 + _S234;

#line 4284
    float3 _S236 = probe_irradiance_0(_S187.world_position_10, _S192, &kernelContext_19);

#line 4305
    float3 lit_1 = diffuse_albedo_0 * ((_S235 + _S236) * float3(_S232)  + direct_0) + gloss_2;

#line 4305
    float3 _S237 = emissive_of_0(&_S189);

#line 4341
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_19)->frame_0->fog_params_0.x, (&kernelContext_19)->frame_0->fog_params_0.y, (&kernelContext_19)->frame_0->camera_position_0.y - (&kernelContext_19)->frame_0->fog_params_0.z, _S187.world_position_10.y - (&kernelContext_19)->frame_0->fog_params_0.z, length((&kernelContext_19)->frame_0->camera_position_0.xyz - _S187.world_position_10)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S237) * float3(fog_survives_0)  + (&kernelContext_19)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_0)->reflectivity_0 = float4(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);

    (&output_0)->motion_0 = motion_1;
    return output_0;
}


#line 4354
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
    uint frame_4 [[user(TEXCOORD_5)]];
};


#line 4354
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], texture2d<float, access::sample> specular_dfg_2 [[texture(3)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], texture2d<float, access::sample> ltc_matrix_2 [[texture(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 4354
    thread KernelContext_0 kernelContext_20;

#line 4354
    (&kernelContext_20)->draw_0 = draw_2;

#line 4354
    (&kernelContext_20)->visible_instances_0 = visible_instances_2;

#line 4354
    (&kernelContext_20)->instances_0 = instances_2;

#line 4354
    (&kernelContext_20)->meshes_0 = meshes_2;

#line 4354
    (&kernelContext_20)->frame_0 = frame_5;

#line 4354
    (&kernelContext_20)->vertices_0 = vertices_2;

#line 4354
    (&kernelContext_20)->ambient_occlusion_0 = ambient_occlusion_2;

#line 4354
    (&kernelContext_20)->materials_0 = materials_2;

#line 4354
    (&kernelContext_20)->normal_textures_0 = normal_textures_2;

#line 4354
    (&kernelContext_20)->base_color_sampler_0 = base_color_sampler_2;

#line 4354
    (&kernelContext_20)->base_color_textures_0 = base_color_textures_2;

#line 4354
    (&kernelContext_20)->cluster_lights_0 = cluster_lights_2;

#line 4354
    (&kernelContext_20)->specular_dfg_0 = specular_dfg_2;

#line 4354
    (&kernelContext_20)->lights_0 = lights_2;

#line 4354
    (&kernelContext_20)->ltc_matrix_0 = ltc_matrix_2;

#line 4354
    (&kernelContext_20)->shadow_atlas_0 = shadow_atlas_2;

#line 4354
    (&kernelContext_20)->shadow_sampler_0 = shadow_sampler_2;

#line 4354
    (&kernelContext_20)->probes_0 = probes_2;

#line 4354
    GpuInstance_natural_0 device* _S238 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1617
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1625
    bool _S239 = ((_S238->flags_0) & 2U) != 0U;

#line 1625
    uint base_vertex_2;
    if(_S239)
    {

#line 1626
        base_vertex_2 = _S238->base_vertex_0;

#line 1626
    }
    else
    {

#line 1626
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1626
    }

#line 1626
    MeshVertex_0 _S240 = load_vertex_0(index_3 + base_vertex_2, float4(mesh_2.uv_scale_u_0, mesh_2.uv_scale_v_0, mesh_2.uv_offset_u_0, mesh_2.uv_offset_v_0), &kernelContext_20);

#line 1626
    uint previous_base_0;

#line 1639
    if(_S239)
    {

#line 1639
        previous_base_0 = _S238->previous_base_vertex_0;

#line 1639
    }
    else
    {

#line 1639
        previous_base_0 = base_vertex_2;

#line 1639
    }

#line 1639
    float3 _S241 = load_position_0(index_3 + previous_base_0, &kernelContext_20);

#line 1639
    matrix<float,int(4),int(4)>  _S242 = matrix<float,int(4),int(4)> (_S238->transform_0.data_0[int(0)][int(0)], _S238->transform_0.data_0[int(1)][int(0)], _S238->transform_0.data_0[int(2)][int(0)], _S238->transform_0.data_0[int(3)][int(0)], _S238->transform_0.data_0[int(0)][int(1)], _S238->transform_0.data_0[int(1)][int(1)], _S238->transform_0.data_0[int(2)][int(1)], _S238->transform_0.data_0[int(3)][int(1)], _S238->transform_0.data_0[int(0)][int(2)], _S238->transform_0.data_0[int(1)][int(2)], _S238->transform_0.data_0[int(2)][int(2)], _S238->transform_0.data_0[int(3)][int(2)], _S238->transform_0.data_0[int(0)][int(3)], _S238->transform_0.data_0[int(1)][int(3)], _S238->transform_0.data_0[int(2)][int(3)], _S238->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S240.position_1, 1.0f)) * (_S242)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_20)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_1 = world_0.xyz;

#line 1653
    matrix<float,int(3),int(3)>  _S243 = matrix<float,int(3),int(3)> (_S242[int(0)].xyz, _S242[int(1)].xyz, _S242[int(2)].xyz);

#line 1653
    (&output_1)->world_normal_0 = (((_S240.basis_1.normal_0) * (normal_basis_0(_S243))));

#line 1659
    (&output_1)->world_tangent_0 = (((_S240.basis_1.tangent_1) * (_S243)));

#line 1659
    thread TangentFrame_0 _S244 = _S240.basis_1;

#line 1659
    uint _S245 = frame_word_0(mesh_2.flags_1, &_S244);
    (&output_1)->frame_1 = _S245;

#line 1660
    float4 _S246;

#line 1667
    if(((&kernelContext_20)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1667
        _S246 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1667
    }
    else
    {

#line 1667
        _S246 = _S240.color_1;

#line 1667
    }

#line 1666
    (&output_1)->color_2 = _S246;

#line 1673
    (&output_1)->material_2 = _S238->material_0;
    (&output_1)->uv_0 = _S240.uv0_0;

#line 1680
    (&output_1)->clip_position_0 = (&output_1)->position_3;
    (&output_1)->previous_clip_position_0 = ((((((float4(_S241, 1.0f)) * (matrix<float,int(4),int(4)> (_S238->previous_transform_0.data_0[int(0)][int(0)], _S238->previous_transform_0.data_0[int(1)][int(0)], _S238->previous_transform_0.data_0[int(2)][int(0)], _S238->previous_transform_0.data_0[int(3)][int(0)], _S238->previous_transform_0.data_0[int(0)][int(1)], _S238->previous_transform_0.data_0[int(1)][int(1)], _S238->previous_transform_0.data_0[int(2)][int(1)], _S238->previous_transform_0.data_0[int(3)][int(1)], _S238->previous_transform_0.data_0[int(0)][int(2)], _S238->previous_transform_0.data_0[int(1)][int(2)], _S238->previous_transform_0.data_0[int(2)][int(2)], _S238->previous_transform_0.data_0[int(3)][int(2)], _S238->previous_transform_0.data_0[int(0)][int(3)], _S238->previous_transform_0.data_0[int(1)][int(3)], _S238->previous_transform_0.data_0[int(2)][int(3)], _S238->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_20)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S247 = output_1;

#line 1684
    thread vertexMain_Result_0 _S248;

#line 1684
    (&_S248)->position_5 = _S247.position_3;

#line 1684
    (&_S248)->world_position_11 = _S247.world_position_1;

#line 1684
    (&_S248)->world_normal_2 = _S247.world_normal_0;

#line 1684
    (&_S248)->color_4 = _S247.color_2;

#line 1684
    (&_S248)->material_6 = _S247.material_2;

#line 1684
    (&_S248)->uv_4 = _S247.uv_0;

#line 1684
    (&_S248)->clip_position_2 = _S247.clip_position_0;

#line 1684
    (&_S248)->previous_clip_position_2 = _S247.previous_clip_position_0;

#line 1684
    (&_S248)->world_tangent_2 = _S247.world_tangent_0;

#line 1684
    (&_S248)->frame_4 = _S247.frame_1;

#line 1684
    return _S248;
}

