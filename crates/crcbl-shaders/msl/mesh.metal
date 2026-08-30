#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 1837 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 1832
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 2104
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 2164
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2316
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 2179
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2207
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1048
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1550
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1550
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


#line 1556
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1556
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
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint pad1_2;
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
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> specular_albedo_0;
    GpuProbe_natural_0 device* probes_0;
};


#line 1091 "shaders/mesh.slang"
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
    float3 tangent_0;
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
    (&basis_0)->tangent_0 = _S2;
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


#line 1102
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1105
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1414
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 1537
uint frame_word_0(uint mesh_flags_0, const TangentFrame_0 thread* basis_3)
{

#line 1537
    uint word_4;

    if((mesh_flags_0 & 1U) != 0U)
    {

#line 1539
        word_4 = 1U;

#line 1539
    }
    else
    {

#line 1539
        word_4 = 0U;

#line 1539
    }



    if((dot(cross(basis_3->normal_0, basis_3->tangent_0), basis_3->bitangent_0)) < 0.0f)
    {

#line 1543
        word_4 = word_4 | 2U;

#line 1543
    }

#line 1542
    return word_4;
}


#line 3449
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S7 = previous_0.w;

#line 3451
    if(_S7 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S7) ) * float2(0.5f, -0.5f);
}


#line 3417
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_2)
{

#line 3417
    texture2d<float, access::sample> _S8 = kernelContext_2->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S8).get_width(0)),(*((&height_0)) = (_S8).get_height(0));

    int3 _S9 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 3423
    return ((kernelContext_2->ambient_occlusion_0).read(vec<uint,2>(((_S9)).xy), uint(((_S9)).z)).x);
}


#line 3167
float2 physical_tile_uv_0(float3 world_position_0, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S10 = axis_0.x;

#line 3171
    float _S11 = axis_0.y;

#line 3171
    bool _S12;

#line 3171
    if(_S10 >= _S11)
    {

#line 3171
        _S12 = _S10 >= (axis_0.z);

#line 3171
    }
    else
    {

#line 3171
        _S12 = false;

#line 3171
    }

#line 3171
    float2 planar_0;

#line 3171
    if(_S12)
    {

#line 3171
        planar_0 = world_position_0.zy;

#line 3171
    }
    else
    {

        if(_S11 >= (axis_0.z))
        {

#line 3175
            planar_0 = world_position_0.xz;

#line 3175
        }
        else
        {

#line 3175
            planar_0 = world_position_0.xy;

#line 3175
        }

#line 3171
    }

#line 3183
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 922
uint normal_layer_0(const GpuMaterial_natural_0 thread* material_1)
{
    return (material_1->color_normal_pages_0) >> 16U;
}


#line 3204
float3 orthonormal_tangent_0(float3 normal_2)
{
    float _S13 = normal_2.z;

#line 3206
    float sign_z_0;

#line 3206
    if(_S13 >= 0.0f)
    {

#line 3206
        sign_z_0 = 1.0f;

#line 3206
    }
    else
    {

#line 3206
        sign_z_0 = -1.0f;

#line 3206
    }
    float a_0 = -1.0f / (sign_z_0 + _S13);
    float _S14 = normal_2.x;

#line 3208
    float _S15 = sign_z_0 * _S14;

#line 3208
    return float3(1.0f + _S15 * _S14 * a_0, _S15 * normal_2.y * a_0, - sign_z_0 * _S14);
}


#line 3258
TangentFrame_0 derivative_frame_0(float3 dpdx_0, float3 dpdy_0, float2 duvdx_0, float2 duvdy_0, float3 normal_3)
{
    float _S16 = duvdy_0.y;

#line 3260
    float _S17 = duvdx_0.y;

#line 3260
    float winding_0;
    if((duvdx_0.x * _S16 - duvdy_0.x * _S17) < 0.0f)
    {

#line 3261
        winding_0 = -1.0f;

#line 3261
    }
    else
    {

#line 3261
        winding_0 = 1.0f;

#line 3261
    }
    float3 tangent_1 = (float3(_S16)  * dpdx_0 - float3(_S17)  * dpdy_0) * float3(winding_0) ;

    thread TangentFrame_0 basis_4;
    (&basis_4)->normal_0 = normal_3;

#line 3270
    float3 tangent_2 = tangent_1 - normal_3 * float3(dot(normal_3, tangent_1)) ;
    float length_squared_0 = dot(tangent_2, tangent_2);

#line 3271
    float3 _S18;

#line 3280
    if(length_squared_0 > 1.00000001686238353e-16f)
    {

#line 3280
        _S18 = tangent_2 * float3(rsqrt(length_squared_0)) ;

#line 3280
    }
    else
    {

#line 3280
        _S18 = orthonormal_tangent_0(normal_3);

#line 3280
    }

#line 3280
    (&basis_4)->tangent_0 = _S18;

    (&basis_4)->bitangent_0 = cross(normal_3, _S18);
    return basis_4;
}


#line 1421
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


#line 3340
float3 shading_normal_of_0(uint layer_0, float normal_scale_1, const VertexOutput_0 thread* input_0, float3 normal_4, float2 uv_1, KernelContext_0 thread* kernelContext_3)
{

#line 3352
    float3 dpdx_1 = dfdx(input_0->world_position_1);
    float3 dpdy_1 = dfdy(input_0->world_position_1);
    float2 duvdx_1 = dfdx(uv_1);
    float2 duvdy_1 = dfdy(uv_1);

    if(layer_0 == 0U)
    {
        return normal_4;
    }

    thread TangentFrame_0 basis_5;

#line 3362
    uint _S19 = input_0->frame_1;
    if(((input_0->frame_1) & 1U) != 0U)
    {

#line 3371
        (&basis_5)->normal_0 = normal_4;
        float3 tangent_3 = input_0->world_tangent_0 - normal_4 * float3(dot(normal_4, input_0->world_tangent_0)) ;
        float length_squared_1 = dot(tangent_3, tangent_3);

#line 3373
        float3 _S20;

#line 3378
        if(length_squared_1 > 1.00000001686238353e-16f)
        {

#line 3378
            _S20 = tangent_3 * float3(rsqrt(length_squared_1)) ;

#line 3378
        }
        else
        {

#line 3378
            _S20 = orthonormal_tangent_0(normal_4);

#line 3378
        }

#line 3378
        (&basis_5)->tangent_0 = _S20;

#line 3384
        float3 _S21 = cross((&basis_5)->normal_0, _S20);

#line 3384
        float _S22;
        if((_S19 & 2U) != 0U)
        {

#line 3385
            _S22 = -1.0f;

#line 3385
        }
        else
        {

#line 3385
            _S22 = 1.0f;

#line 3385
        }

#line 3384
        (&basis_5)->bitangent_0 = _S21 * float3(_S22) ;

#line 3363
    }
    else
    {

#line 3389
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);

#line 3363
    }

#line 3393
    float3 _S23 = float3(uv_1, float(layer_0));
    float3 _S24 = ((kernelContext_3->normal_textures_0).sample((kernelContext_3->base_color_sampler_0), ((_S23)).xy, uint(((_S23)).z), gradient2d((duvdx_1), (duvdy_1)))).xyz * float3(2.0f)  - float3(1.0f) ;

#line 3394
    thread float3 tangent_space_0 = _S24;
    tangent_space_0.xy = _S24.xy * float2(normal_scale_1) ;

#line 3400
    float3 _S25 = normalize(tangent_space_0);

#line 3400
    tangent_space_0 = _S25;
    return normalize(float3(_S25.x)  * (&basis_5)->tangent_0 + float3(_S25.y)  * (&basis_5)->bitangent_0 + float3(_S25.z)  * (&basis_5)->normal_0);
}


#line 1972
float3 geometric_normal_of_0(float3 world_position_2, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_2), dfdy(world_position_2));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1983
    float3 _S26;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1984
        _S26 = - facet_1;

#line 1984
    }
    else
    {

#line 1984
        _S26 = facet_1;

#line 1984
    }

#line 1984
    return _S26;
}


#line 907
uint base_color_layer_0(const GpuMaterial_natural_0 thread* material_3)
{
    return (material_3->color_normal_pages_0) & 65535U;
}


#line 2965
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_4)
{
    uint _S27 = max(kernelContext_4->frame_0->cluster_grid_0.x, 1U);
    uint _S28 = max(kernelContext_4->frame_0->cluster_grid_0.y, 1U);
    uint _S29 = max(kernelContext_4->frame_0->cluster_grid_0.z, 1U);
    uint _S30 = max(kernelContext_4->frame_0->cluster_grid_0.w, 1U);

#line 2975
    uint _S31 = uint(pixel_0.x) / _S30;

#line 2975
    uint _S32 = min(_S31, _S27 - 1U);
    uint _S33 = uint(pixel_0.y) / _S30;

    float scale_0 = 24.0f / log2(10000.0f);

#line 2986
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S29 - 1U))) * _S28 + min(_S33, _S28 - 1U)) * _S27 + _S32;
}


#line 2930
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 2944
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 2951
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 1696
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 1703
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1710
    float _S34 = 1.0f - alpha2_0;

#line 1715
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S34 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S34 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 2023
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 2338
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


#line 2454
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_5)
{
    float2 texel_0 = kernelContext_5->frame_0->shadow_params_0.xy;
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S35 = float2(0.5f, 0.5f) * texel_0 * grid_0;


    float2 _S36 = float2(1.0f, 1.0f);

#line 2461
    float2 _S37 = _S36 / texel_0;

#line 2461
    uint index_0 = 0U;

#line 2461
    float sum_0 = 0.0f;

#line 2461
    float found_0 = 0.0f;



    for(;;)
    {

#line 2465
        if(index_0 < 16U)
        {
        }
        else
        {

#line 2465
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_0] * float2(8.0f) ;
        float _S38 = spoke_0.x;

#line 2468
        float _S39 = rotation_0.x;

#line 2468
        float _S40 = spoke_0.y;

#line 2468
        float _S41 = rotation_0.y;

#line 2477
        int3 _S42 = int3(int2(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + float2(_S38 * _S39 - _S40 * _S41, _S38 * _S41 + _S40 * _S39) * texel_0 * grid_0, _S35, float2(1.0f)  - _S35)) * _S37, _S37 - _S36)), int(0));

#line 2477
        float depth_1 = ((kernelContext_5->shadow_atlas_0).read(vec<uint,2>(((_S42)).xy), uint(((_S42)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 2481
            sum_0 = sum_0 + depth_1;

#line 2481
            found_0 = found_1;

#line 2478
        }

#line 2465
        index_0 = index_0 + 1U;

#line 2465
    }

#line 2485
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 2496
    float _S43 = 2.0f * kernelContext_5->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_0 / found_0 - reference_0) * (_S43 + 40.0f) * 0.01999999955296516f / (_S43 / 768.0f), 2.0f, 8.0f);
}


#line 2356
float tile_tap_0(uint tile_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_6)
{
    float2 texel_1 = kernelContext_6->frame_0->shadow_params_0.xy;

#line 2363
    float2 grid_1 = float2(4.0f, 4.0f);
    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_1 * grid_1;

    float _S44 = spoke_1.x;

#line 2366
    float _S45 = rotation_1.x;

#line 2366
    float _S46 = spoke_1.y;

#line 2366
    float _S47 = rotation_1.y;


    float _S48 = ((kernelContext_6->shadow_atlas_0).sample_compare((kernelContext_6->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + float2(_S44 * _S45 - _S46 * _S47, _S44 * _S47 + _S46 * _S45) * texel_1 * grid_1, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2369
    return _S48;
}


#line 2391
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_1, KernelContext_0 thread* kernelContext_7)
{
    float2 _S49 = shadow_rotation_0(pixel_2);

#line 2393
    uint spot_0 = 0U;

#line 2393
    float probe_0 = 0.0f;


    for(;;)
    {

#line 2396
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2396
            break;
        }

#line 2396
        float _S50 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_1) , _S49, reference_2, kernelContext_7);

        float probe_1 = probe_0 + _S50;

#line 2396
        spot_0 = spot_0 + 1U;

#line 2396
        probe_0 = probe_1;

#line 2396
    }

#line 2405
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2411
    uint index_1 = 0U;

#line 2411
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2415
        if(index_1 < 32U)
        {
        }
        else
        {

#line 2415
            break;
        }

#line 2415
        float _S51 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[index_1] * float2(radius_1) , _S49, reference_2, kernelContext_7);

        float visibility_1 = visibility_0 + _S51;

#line 2415
        index_1 = index_1 + 1U;

#line 2415
        visibility_0 = visibility_1;

#line 2415
    }



    return visibility_0 / 32.0f;
}


#line 2550
float cascade_visibility_0(uint cascade_1, float3 world_position_3, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_8)
{

#line 2581
    float texel_world_0 = 2.0f * kernelContext_8->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 2588
    float4 clip_0 = (((float4(world_position_3 + geometric_normal_1 * float3((texel_world_0 * kernelContext_8->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_8->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_8->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 2592
    bool _S52;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 2593
        _S52 = true;

#line 2593
    }
    else
    {

#line 2593
        _S52 = (ndc_0.z) <= 0.0f;

#line 2593
    }

#line 2593
    if(_S52)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 2620
    float _S53 = ndc_0.z;

#line 2620
    float _S54 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S53, shadow_rotation_0(pixel_3), kernelContext_8);

#line 2620
    float _S55 = tile_pcf_0(cascade_1, tile_uv_4, _S53, pixel_3, _S54, kernelContext_8);
    return _S55;
}


#line 2637
float sun_visibility_0(float3 world_position_4, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_9)
{

#line 2638
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 2650
    float eye_distance_0 = length(world_position_4 - kernelContext_9->frame_0->camera_position_0.xyz);

#line 2650
    uint index_2 = 0U;

    for(;;)
    {

#line 2652
        if(index_2 < 2U)
        {
        }
        else
        {

#line 2652
            cascade_2 = 1U;

#line 2652
            break;
        }
        if(eye_distance_0 < kernelContext_9->frame_0->cascade_far_0[index_2])
        {

#line 2654
            cascade_2 = index_2;


            break;
        }

#line 2652
        index_2 = index_2 + 1U;

#line 2652
    }

#line 2652
    float _S56 = cascade_visibility_0(cascade_2, world_position_4, to_light_3, geometric_normal_2, pixel_4, kernelContext_9);

#line 2663
    uint _S57 = cascade_2 + 1U;

#line 2663
    if(_S57 >= 2U)
    {



        return _S56;
    }

#line 2676
    float band_0 = kernelContext_9->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_9->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S56;
    }

#line 2680
    float _S58 = cascade_visibility_0(_S57, world_position_4, to_light_3, geometric_normal_2, pixel_4, kernelContext_9);

#line 2691
    return mix(_S56, _S58, blend_0);
}


#line 2881
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S59 = axis_2.x;

#line 2884
    float _S60 = axis_2.y;

#line 2884
    bool _S61;

#line 2884
    if(_S59 >= _S60)
    {

#line 2884
        _S61 = _S59 >= (axis_2.z);

#line 2884
    }
    else
    {

#line 2884
        _S61 = false;

#line 2884
    }

#line 2884
    uint _S62;

#line 2884
    if(_S61)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 2886
            _S62 = 0U;

#line 2886
        }
        else
        {

#line 2886
            _S62 = 1U;

#line 2886
        }

#line 2886
        return _S62;
    }
    if(_S60 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 2890
            _S62 = 2U;

#line 2890
        }
        else
        {

#line 2890
            _S62 = 3U;

#line 2890
        }

#line 2890
        return _S62;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 2892
        _S62 = 4U;

#line 2892
    }
    else
    {

#line 2892
        _S62 = 5U;

#line 2892
    }

#line 2892
    return _S62;
}


#line 308
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 2794
float punctual_visibility_0(uint tile_4, float3 world_position_5, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_10)
{

#line 2806
    float4 clip_1 = (((float4(world_position_5 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_10->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 2813
    float _S63 = clip_1.w;

#line 2813
    if(_S63 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S63) ;

#line 2817
    bool _S64;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 2818
        _S64 = true;

#line 2818
    }
    else
    {

#line 2818
        _S64 = (ndc_1.z) <= 0.0f;

#line 2818
    }

#line 2818
    if(_S64)
    {

#line 2818
        _S64 = true;

#line 2818
    }
    else
    {

#line 2818
        _S64 = (ndc_1.z) > 1.0f;

#line 2818
    }

#line 2818
    if(_S64)
    {

#line 2825
        return 1.0f;
    }

#line 2825
    float _S65 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_10);

#line 2835
    return _S65;
}


#line 2900
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_6, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_11)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_6 - (float4(light_0->position_0) ).xyz;

#line 2908
    float _S66 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_6, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_11);

#line 2914
    return _S66;
}


#line 2842
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_5, float3 world_position_7, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_12)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 2849
    float4 _S67 = float4(light_1->direction_0) ;

#line 2856
    float cos_outer_1 = _S67.w;

#line 2856
    float _S68 = punctual_visibility_0(tile_5, world_position_7, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_7 - (float4(light_1->position_0) ).xyz, normalize(_S67.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_12);

#line 2863
    return _S68;
}


#line 1740
float decode_specular_albedo_0(float2 texel_2)
{
    return (texel_2.x * 65280.0f + texel_2.y * 255.0f) / 65535.0f;
}


#line 1757
float specular_albedo_at_0(float n_dot_v_1, float roughness_1, KernelContext_0 thread* kernelContext_13)
{

#line 1757
    texture2d<float, access::sample> _S69 = kernelContext_13->specular_albedo_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S69).get_width(0)),(*((&height_1)) = (_S69).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1763
    float2 _S70 = float2(1.0f) ;
    float2 _S71 = extent_1 - _S70;

#line 1764
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S71);

    float2 weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S72 = int2(low_1);
    int2 _S73 = int2(min(low_1 + _S70, _S71));
    int _S74 = _S72.x;

#line 1770
    int _S75 = _S72.y;

#line 1770
    int3 _S76 = int3(_S74, _S75, int(0));
    int _S77 = _S73.x;

#line 1771
    int3 _S78 = int3(_S77, _S75, int(0));
    float _S79 = weight_0.x;
    int _S80 = _S73.y;

#line 1773
    int3 _S81 = int3(_S74, _S80, int(0));
    int3 _S82 = int3(_S77, _S80, int(0));

    return mix(mix(decode_specular_albedo_0(((kernelContext_13->specular_albedo_0).read(vec<uint,2>(((_S76)).xy), uint(((_S76)).z)).xy)), decode_specular_albedo_0(((kernelContext_13->specular_albedo_0).read(vec<uint,2>(((_S78)).xy), uint(((_S78)).z)).xy)), _S79), mix(decode_specular_albedo_0(((kernelContext_13->specular_albedo_0).read(vec<uint,2>(((_S81)).xy), uint(((_S81)).z)).xy)), decode_specular_albedo_0(((kernelContext_13->specular_albedo_0).read(vec<uint,2>(((_S82)).xy), uint(((_S82)).z)).xy)), _S79), weight_0.y);
}


#line 1795
float3 specular_compensation_0(float3 f0_1, float n_dot_v_2, float roughness_2, KernelContext_0 thread* kernelContext_14)
{

#line 1795
    float _S83 = specular_albedo_at_0(n_dot_v_2, roughness_2, kernelContext_14);



    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(_S83, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 3094
float3 sky_irradiance_0(float3 normal_5, KernelContext_0 thread* kernelContext_15)
{
    float4 basis_6 = float4(normal_5, 1.0f);
    return max(float3(dot(kernelContext_15->frame_0->sky_sh_r_0, basis_6), dot(kernelContext_15->frame_0->sky_sh_g_0, basis_6), dot(kernelContext_15->frame_0->sky_sh_b_0, basis_6)), float3(0.0f, 0.0f, 0.0f));
}


#line 959
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 2996
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_16)
{

    GpuProbe_natural_0 _S84 = kernelContext_16->probes_0[min((cell_1.z * kernelContext_16->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_16->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_16->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 2999
    GpuProbe_0 _S85 = { float4(_S84.sh_r_0) , float4(_S84.sh_g_0) , float4(_S84.sh_b_0)  };

#line 2999
    return _S85;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_1, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S86 = float4(t_1) ;

#line 3007
    (&blended_0)->sh_r_0 = mix(a_1->sh_r_0, b_0->sh_r_0, _S86);
    (&blended_0)->sh_g_0 = mix(a_1->sh_g_0, b_0->sh_g_0, _S86);
    (&blended_0)->sh_b_0 = mix(a_1->sh_b_0, b_0->sh_b_0, _S86);
    return blended_0;
}


#line 3047
float3 probe_irradiance_0(float3 world_position_8, float3 normal_6, KernelContext_0 thread* kernelContext_17)
{

#line 3047
    float3 _S87 = float3(1.0f) ;

#line 3052
    float3 _S88 = float3(0.0f, 0.0f, 0.0f);

#line 3052
    float3 last_0 = max(float3(kernelContext_17->frame_0->probe_counts_0.xyz) - _S87, _S88);
    float3 grid_2 = clamp((world_position_8 - kernelContext_17->frame_0->probe_origin_0.xyz) * kernelContext_17->frame_0->probe_inv_spacing_0.xyz, _S88, last_0);

    float3 base_2 = floor(grid_2);
    float3 f_0 = grid_2 - base_2;

    uint3 _S89 = uint3(base_2);



    uint3 _S90 = uint3(min(base_2 + _S87, last_0));

#line 3069
    uint _S91 = _S89.x;

#line 3069
    uint _S92 = _S89.y;

#line 3069
    uint _S93 = _S89.z;

#line 3069
    GpuProbe_0 _S94 = probe_at_0(uint3(_S91, _S92, _S93), kernelContext_17);

#line 3069
    uint _S95 = _S90.x;

#line 3069
    GpuProbe_0 _S96 = probe_at_0(uint3(_S95, _S92, _S93), kernelContext_17);

#line 3069
    float _S97 = f_0.x;

#line 3069
    thread GpuProbe_0 _S98 = _S94;

#line 3069
    thread GpuProbe_0 _S99 = _S96;

#line 3069
    GpuProbe_0 _S100 = lerp_probe_0(&_S98, &_S99, _S97);
    uint _S101 = _S90.y;

#line 3070
    GpuProbe_0 _S102 = probe_at_0(uint3(_S91, _S101, _S93), kernelContext_17);

#line 3070
    GpuProbe_0 _S103 = probe_at_0(uint3(_S95, _S101, _S93), kernelContext_17);

#line 3070
    thread GpuProbe_0 _S104 = _S102;

#line 3070
    thread GpuProbe_0 _S105 = _S103;

#line 3070
    GpuProbe_0 _S106 = lerp_probe_0(&_S104, &_S105, _S97);
    uint _S107 = _S90.z;

#line 3071
    GpuProbe_0 _S108 = probe_at_0(uint3(_S91, _S92, _S107), kernelContext_17);

#line 3071
    GpuProbe_0 _S109 = probe_at_0(uint3(_S95, _S92, _S107), kernelContext_17);

#line 3071
    thread GpuProbe_0 _S110 = _S108;

#line 3071
    thread GpuProbe_0 _S111 = _S109;

#line 3071
    GpuProbe_0 _S112 = lerp_probe_0(&_S110, &_S111, _S97);

#line 3071
    GpuProbe_0 _S113 = probe_at_0(uint3(_S91, _S101, _S107), kernelContext_17);

#line 3071
    GpuProbe_0 _S114 = probe_at_0(uint3(_S95, _S101, _S107), kernelContext_17);

#line 3071
    thread GpuProbe_0 _S115 = _S113;

#line 3071
    thread GpuProbe_0 _S116 = _S114;

#line 3071
    GpuProbe_0 _S117 = lerp_probe_0(&_S115, &_S116, _S97);

    float _S118 = f_0.y;

#line 3073
    thread GpuProbe_0 _S119 = _S100;

#line 3073
    thread GpuProbe_0 _S120 = _S106;

#line 3073
    GpuProbe_0 _S121 = lerp_probe_0(&_S119, &_S120, _S118);

#line 3073
    thread GpuProbe_0 _S122 = _S112;

#line 3073
    thread GpuProbe_0 _S123 = _S117;

#line 3073
    GpuProbe_0 _S124 = lerp_probe_0(&_S122, &_S123, _S118);

    float _S125 = f_0.z;

#line 3075
    thread GpuProbe_0 _S126 = _S121;

#line 3075
    thread GpuProbe_0 _S127 = _S124;

#line 3075
    GpuProbe_0 _S128 = lerp_probe_0(&_S126, &_S127, _S125);

    float4 basis_7 = float4(normal_6, 1.0f);
    return max(float3(dot(_S128.sh_r_0, basis_7), dot(_S128.sh_g_0, basis_7), dot(_S128.sh_b_0, basis_7)), _S88);
}


#line 932
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_4)
{
    return float3(material_4->emissive_r_0, material_4->emissive_g_0, material_4->emissive_b_0);
}


#line 1857
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S129 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 1865
    float kernel_0 = 0.0001984127011383f;

#line 1865
    int term_0 = int(6);

    for(;;)
    {

#line 1867
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 1867
            break;
        }
        float _S130 = kernel_0 * _S129 + FOG_KERNEL_0[term_0];

#line 1867
        int term_1 = term_0 - int(1);

#line 1867
        kernel_0 = _S130;

#line 1867
        term_0 = term_1;

#line 1867
    }

#line 1874
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 1884
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S131 = - d_0;

#line 1888
        float series_0 = 0.00833333376795053f;

#line 1888
        int term_2 = int(3);

        for(;;)
        {

#line 1890
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 1890
                break;
            }
            float _S132 = series_0 * _S131 + FOG_RATIO_KERNEL_0[term_2];

#line 1890
            int term_3 = term_2 - int(1);

#line 1890
            series_0 = _S132;

#line 1890
            term_2 = term_3;

#line 1890
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 1918
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_1)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_1, 0.0f, 32.0f);
    }

#line 1929
    return clamp(density_0 * distance_1 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 1937
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 3120
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 3120
struct pixelInput_0
{
    float3 world_position_9 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    [[flat]] uint material_5 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
    float3 world_tangent_1 [[user(TEXCOORD_4)]];
    [[flat]] uint frame_2 [[user(TEXCOORD_5)]];
};


#line 3459
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S133 [[stage_in]], float4 position_4 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_1 [[texture(4)]], sampler base_color_sampler_1 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_1 [[texture(3)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 3459
    thread KernelContext_0 kernelContext_18;

#line 3459
    (&kernelContext_18)->draw_0 = draw_1;

#line 3459
    (&kernelContext_18)->visible_instances_0 = visible_instances_1;

#line 3459
    (&kernelContext_18)->instances_0 = instances_1;

#line 3459
    (&kernelContext_18)->meshes_0 = meshes_1;

#line 3459
    (&kernelContext_18)->frame_0 = frame_3;

#line 3459
    (&kernelContext_18)->vertices_0 = vertices_1;

#line 3459
    (&kernelContext_18)->ambient_occlusion_0 = ambient_occlusion_1;

#line 3459
    (&kernelContext_18)->materials_0 = materials_1;

#line 3459
    (&kernelContext_18)->normal_textures_0 = normal_textures_1;

#line 3459
    (&kernelContext_18)->base_color_sampler_0 = base_color_sampler_1;

#line 3459
    (&kernelContext_18)->base_color_textures_0 = base_color_textures_1;

#line 3459
    (&kernelContext_18)->cluster_lights_0 = cluster_lights_1;

#line 3459
    (&kernelContext_18)->lights_0 = lights_1;

#line 3459
    (&kernelContext_18)->shadow_atlas_0 = shadow_atlas_1;

#line 3459
    (&kernelContext_18)->shadow_sampler_0 = shadow_sampler_1;

#line 3459
    (&kernelContext_18)->specular_albedo_0 = specular_albedo_1;

#line 3459
    (&kernelContext_18)->probes_0 = probes_1;

#line 3471
    float3 vertex_normal_0 = normalize(_S133.world_normal_1);

#line 3476
    float2 motion_1 = motion_vector_0(_S133.clip_position_1, _S133.previous_clip_position_1);

#line 3485
    if((frame_3->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 3527
    if((frame_3->ambient_0.w) >= 3.5f)
    {

#line 3527
        float _S134 = occlusion_at_0(position_4.xy, &kernelContext_18);

        thread FragmentOutput_0 occlusion_0;

#line 3538
        (&occlusion_0)->lit_0 = float4(_S134, _S134, _S134, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_3->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S133.color_3.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

#line 3555
    thread GpuMaterial_natural_0 _S135 = (&kernelContext_18)->materials_0[_S133.material_5];

#line 3555
    float2 uv_3;

#line 3580
    if(((&_S135)->tiling_0) == 1U)
    {

#line 3580
        uv_3 = physical_tile_uv_0(_S133.world_position_9, vertex_normal_0, (&_S135)->tile_metres_0);

#line 3580
    }
    else
    {

#line 3580
        uv_3 = _S133.uv_2;

#line 3580
    }

#line 3580
    uint _S136 = normal_layer_0(&_S135);

#line 3580
    thread VertexOutput_0 _S137;

#line 3580
    (&_S137)->position_3 = position_4;

#line 3580
    (&_S137)->world_position_1 = _S133.world_position_9;

#line 3580
    (&_S137)->world_normal_0 = _S133.world_normal_1;

#line 3580
    (&_S137)->color_2 = _S133.color_3;

#line 3580
    (&_S137)->material_2 = _S133.material_5;

#line 3580
    (&_S137)->uv_0 = _S133.uv_2;

#line 3580
    (&_S137)->clip_position_0 = _S133.clip_position_1;

#line 3580
    (&_S137)->previous_clip_position_0 = _S133.previous_clip_position_1;

#line 3580
    (&_S137)->world_tangent_0 = _S133.world_tangent_1;

#line 3580
    (&_S137)->frame_1 = _S133.frame_2;

#line 3580
    float3 _S138 = shading_normal_of_0(_S136, (&_S135)->normal_scale_0, &_S137, vertex_normal_0, uv_3, &kernelContext_18);

#line 3587
    if((frame_3->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 3589
        float3 _S139 = float3(0.5f) ;

#line 3601
        (&normals_0)->lit_0 = float4(_S138 * _S139 + _S139, 1.0f);

#line 3607
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_18)->frame_0->camera_position_0.xyz - _S133.world_position_9);



    float3 _S140 = geometric_normal_of_0(_S133.world_position_9, vertex_normal_0);

#line 3616
    uint _S141 = base_color_layer_0(&_S135);

#line 3631
    float3 _S142 = float3(uv_3, float(_S141));
    float4 albedo_0 = _S133.color_3 * float4((&_S135)->base_color_0)  * (((&kernelContext_18)->base_color_textures_0).sample(((&kernelContext_18)->base_color_sampler_0), ((_S142)).xy, uint(((_S142)).z)));

#line 3638
    float metallic_1 = saturate((&_S135)->metallic_0);
    float roughness_3 = clamp((&_S135)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_3 * roughness_3;
    float _S143 = alpha_0 * alpha_0;

#line 3647
    float3 _S144 = albedo_0.xyz;

#line 3647
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S144, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S144 * float3((1.0f - metallic_1)) ;

#line 3654
    float _S145 = max(dot(_S138, to_eye_0), 0.00009999999747379f);

#line 3664
    float2 _S146 = position_4.xy;

#line 3664
    uint _S147 = froxel_of_0(_S146, (((float4(_S133.world_position_9, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_18);

#line 3664
    uint base_3 = _S147 * 17U;

#line 3669
    uint _S148 = min((&kernelContext_18)->cluster_lights_0[base_3], 16U);

#line 3675
    float3 _S149 = float3(0.0f, 0.0f, 0.0f);

#line 3675
    uint slot_0 = 0U;

#line 3675
    float3 direct_0 = _S149;

#line 3675
    float3 gloss_0 = _S149;

    for(;;)
    {

#line 3677
        if(slot_0 < _S148)
        {
        }
        else
        {

#line 3677
            break;
        }

#line 3677
        thread GpuLight_natural_0 _S150 = (&kernelContext_18)->lights_0[(&kernelContext_18)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 3677
        uint _S151 = (&_S150)->kind_0;

#line 3686
        bool _S152 = ((&_S150)->kind_0) == 0U;

#line 3686
        float3 to_light_7;

#line 3686
        float reach_0;

#line 3686
        if(_S152)
        {

#line 3686
            to_light_7 = normalize((float4((&_S150)->direction_0) ).xyz);

#line 3686
            reach_0 = 1.0f;

#line 3686
        }
        else
        {

#line 3686
            float4 _S153 = float4((&_S150)->position_0) ;

#line 3693
            float3 offset_0 = _S153.xyz - _S133.world_position_9;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S153.w);
            if(_S151 == 2U)
            {

#line 3697
                float4 _S154 = float4((&_S150)->direction_0) ;

#line 3697
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S154.xyz, _S154.w, (&_S150)->cos_inner_0);

#line 3697
            }
            else
            {

#line 3697
                reach_0 = reach_1;

#line 3697
            }

#line 3697
            to_light_7 = to_light_8;

#line 3686
        }

#line 3704
        float n_dot_l_5 = dot(_S138, to_light_7);
        float _S155 = max(n_dot_l_5, 0.0f);

#line 3711
        float3 half_vector_0 = normalize(to_light_7 + to_eye_0);

#line 3718
        float3 specular_0 = ggx_lobe_0(_S143, f0_2, _S155, _S145, max(dot(_S138, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S155) ;

#line 3718
        float reach_2;

#line 3733
        if(_S152)
        {

#line 3733
            float _S156 = sun_visibility_0(_S133.world_position_9, to_light_7, n_dot_l_5, _S140, _S146, &kernelContext_18);

#line 3733
            reach_2 = _S156;

#line 3733
        }
        else
        {


            if(_S151 == 1U)
            {

#line 3738
                uint _S157 = (&_S150)->shadow_tile_0;

#line 3750
                if(((&_S150)->shadow_tile_0) <= 8U)
                {

#line 3750
                    float _S158 = point_visibility_0(&_S150, _S157, _S133.world_position_9, to_light_7, n_dot_l_5, _S140, _S146, &kernelContext_18);

#line 3750
                    reach_2 = reach_0 * _S158;

#line 3750
                }
                else
                {

#line 3750
                    reach_2 = reach_0;

#line 3750
                }

#line 3738
            }
            else
            {

#line 3738
                uint _S159 = (&_S150)->shadow_tile_0;

#line 3756
                if(((&_S150)->shadow_tile_0) < 14U)
                {

#line 3756
                    float _S160 = spot_visibility_0(&_S150, _S159, _S133.world_position_9, to_light_7, n_dot_l_5, _S140, _S146, &kernelContext_18);

#line 3756
                    reach_2 = reach_0 * _S160;

#line 3756
                }
                else
                {

#line 3756
                    reach_2 = reach_0;

#line 3756
                }

#line 3738
            }

#line 3733
        }

#line 3764
        float3 _S161 = (float4((&_S150)->color_0) ).xyz;

#line 3764
        float3 direct_1 = direct_0 + _S161 * float3((_S155 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S161 * (specular_0 * float3(reach_2) );

#line 3677
        slot_0 = slot_0 + 1U;

#line 3677
        direct_0 = direct_1;

#line 3677
        gloss_0 = gloss_1;

#line 3677
    }

#line 3677
    float3 _S162 = specular_compensation_0(f0_2, _S145, roughness_3, &kernelContext_18);

#line 3779
    float3 gloss_2 = gloss_0 * _S162;

#line 3779
    float _S163 = occlusion_at_0(_S146, &kernelContext_18);

#line 3815
    float3 _S164 = frame_3->ambient_0.xyz;

#line 3815
    float3 _S165 = sky_irradiance_0(_S138, &kernelContext_18);

#line 3815
    float3 _S166 = _S164 + _S165;

#line 3815
    float3 _S167 = probe_irradiance_0(_S133.world_position_9, _S138, &kernelContext_18);

#line 3836
    float3 lit_1 = diffuse_albedo_0 * ((_S166 + _S167) * float3(_S163)  + direct_0) + gloss_2;

#line 3836
    float3 _S168 = emissive_of_0(&_S135);

#line 3872
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_18)->frame_0->fog_params_0.x, (&kernelContext_18)->frame_0->fog_params_0.y, (&kernelContext_18)->frame_0->camera_position_0.y - (&kernelContext_18)->frame_0->fog_params_0.z, _S133.world_position_9.y - (&kernelContext_18)->frame_0->fog_params_0.z, length((&kernelContext_18)->frame_0->camera_position_0.xyz - _S133.world_position_9)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S168) * float3(fog_survives_0)  + (&kernelContext_18)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_0)->reflectivity_0 = float4(f0_2, floor(roughness_3 * 255.0f + 0.5f) / 255.0f);

    (&output_0)->motion_0 = motion_1;
    return output_0;
}


#line 3885
struct vertexMain_Result_0
{
    float4 position_5 [[position]];
    float3 world_position_10 [[user(POSITION)]];
    float3 world_normal_2 [[user(NORMAL)]];
    float4 color_4 [[user(COLOR)]];
    uint material_6 [[user(TEXCOORD)]];
    float2 uv_4 [[user(TEXCOORD_1)]];
    float4 clip_position_2 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_2 [[user(TEXCOORD_3)]];
    float3 world_tangent_2 [[user(TEXCOORD_4)]];
    uint frame_4 [[user(TEXCOORD_5)]];
};


#line 3885
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> normal_textures_2 [[texture(4)]], sampler base_color_sampler_2 [[sampler(0)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_2 [[texture(3)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 3885
    thread KernelContext_0 kernelContext_19;

#line 3885
    (&kernelContext_19)->draw_0 = draw_2;

#line 3885
    (&kernelContext_19)->visible_instances_0 = visible_instances_2;

#line 3885
    (&kernelContext_19)->instances_0 = instances_2;

#line 3885
    (&kernelContext_19)->meshes_0 = meshes_2;

#line 3885
    (&kernelContext_19)->frame_0 = frame_5;

#line 3885
    (&kernelContext_19)->vertices_0 = vertices_2;

#line 3885
    (&kernelContext_19)->ambient_occlusion_0 = ambient_occlusion_2;

#line 3885
    (&kernelContext_19)->materials_0 = materials_2;

#line 3885
    (&kernelContext_19)->normal_textures_0 = normal_textures_2;

#line 3885
    (&kernelContext_19)->base_color_sampler_0 = base_color_sampler_2;

#line 3885
    (&kernelContext_19)->base_color_textures_0 = base_color_textures_2;

#line 3885
    (&kernelContext_19)->cluster_lights_0 = cluster_lights_2;

#line 3885
    (&kernelContext_19)->lights_0 = lights_2;

#line 3885
    (&kernelContext_19)->shadow_atlas_0 = shadow_atlas_2;

#line 3885
    (&kernelContext_19)->shadow_sampler_0 = shadow_sampler_2;

#line 3885
    (&kernelContext_19)->specular_albedo_0 = specular_albedo_2;

#line 3885
    (&kernelContext_19)->probes_0 = probes_2;

#line 3885
    GpuInstance_natural_0 device* _S169 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1556
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1564
    bool _S170 = ((_S169->flags_0) & 2U) != 0U;

#line 1564
    uint base_vertex_2;
    if(_S170)
    {

#line 1565
        base_vertex_2 = _S169->base_vertex_0;

#line 1565
    }
    else
    {

#line 1565
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1565
    }

#line 1565
    MeshVertex_0 _S171 = load_vertex_0(index_3 + base_vertex_2, float4(mesh_2.uv_scale_u_0, mesh_2.uv_scale_v_0, mesh_2.uv_offset_u_0, mesh_2.uv_offset_v_0), &kernelContext_19);

#line 1565
    uint previous_base_0;

#line 1578
    if(_S170)
    {

#line 1578
        previous_base_0 = _S169->previous_base_vertex_0;

#line 1578
    }
    else
    {

#line 1578
        previous_base_0 = base_vertex_2;

#line 1578
    }

#line 1578
    float3 _S172 = load_position_0(index_3 + previous_base_0, &kernelContext_19);

#line 1578
    matrix<float,int(4),int(4)>  _S173 = matrix<float,int(4),int(4)> (_S169->transform_0.data_0[int(0)][int(0)], _S169->transform_0.data_0[int(1)][int(0)], _S169->transform_0.data_0[int(2)][int(0)], _S169->transform_0.data_0[int(3)][int(0)], _S169->transform_0.data_0[int(0)][int(1)], _S169->transform_0.data_0[int(1)][int(1)], _S169->transform_0.data_0[int(2)][int(1)], _S169->transform_0.data_0[int(3)][int(1)], _S169->transform_0.data_0[int(0)][int(2)], _S169->transform_0.data_0[int(1)][int(2)], _S169->transform_0.data_0[int(2)][int(2)], _S169->transform_0.data_0[int(3)][int(2)], _S169->transform_0.data_0[int(0)][int(3)], _S169->transform_0.data_0[int(1)][int(3)], _S169->transform_0.data_0[int(2)][int(3)], _S169->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S171.position_1, 1.0f)) * (_S173)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_19)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_1 = world_0.xyz;

#line 1592
    matrix<float,int(3),int(3)>  _S174 = matrix<float,int(3),int(3)> (_S173[int(0)].xyz, _S173[int(1)].xyz, _S173[int(2)].xyz);

#line 1592
    (&output_1)->world_normal_0 = (((_S171.basis_1.normal_0) * (normal_basis_0(_S174))));

#line 1598
    (&output_1)->world_tangent_0 = (((_S171.basis_1.tangent_0) * (_S174)));

#line 1598
    thread TangentFrame_0 _S175 = _S171.basis_1;

#line 1598
    uint _S176 = frame_word_0(mesh_2.flags_1, &_S175);
    (&output_1)->frame_1 = _S176;

#line 1599
    float4 _S177;

#line 1606
    if(((&kernelContext_19)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1606
        _S177 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1606
    }
    else
    {

#line 1606
        _S177 = _S171.color_1;

#line 1606
    }

#line 1605
    (&output_1)->color_2 = _S177;

#line 1612
    (&output_1)->material_2 = _S169->material_0;
    (&output_1)->uv_0 = _S171.uv0_0;

#line 1619
    (&output_1)->clip_position_0 = (&output_1)->position_3;
    (&output_1)->previous_clip_position_0 = ((((((float4(_S172, 1.0f)) * (matrix<float,int(4),int(4)> (_S169->previous_transform_0.data_0[int(0)][int(0)], _S169->previous_transform_0.data_0[int(1)][int(0)], _S169->previous_transform_0.data_0[int(2)][int(0)], _S169->previous_transform_0.data_0[int(3)][int(0)], _S169->previous_transform_0.data_0[int(0)][int(1)], _S169->previous_transform_0.data_0[int(1)][int(1)], _S169->previous_transform_0.data_0[int(2)][int(1)], _S169->previous_transform_0.data_0[int(3)][int(1)], _S169->previous_transform_0.data_0[int(0)][int(2)], _S169->previous_transform_0.data_0[int(1)][int(2)], _S169->previous_transform_0.data_0[int(2)][int(2)], _S169->previous_transform_0.data_0[int(3)][int(2)], _S169->previous_transform_0.data_0[int(0)][int(3)], _S169->previous_transform_0.data_0[int(1)][int(3)], _S169->previous_transform_0.data_0[int(2)][int(3)], _S169->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_19)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S178 = output_1;

#line 1623
    thread vertexMain_Result_0 _S179;

#line 1623
    (&_S179)->position_5 = _S178.position_3;

#line 1623
    (&_S179)->world_position_10 = _S178.world_position_1;

#line 1623
    (&_S179)->world_normal_2 = _S178.world_normal_0;

#line 1623
    (&_S179)->color_4 = _S178.color_2;

#line 1623
    (&_S179)->material_6 = _S178.material_2;

#line 1623
    (&_S179)->uv_4 = _S178.uv_0;

#line 1623
    (&_S179)->clip_position_2 = _S178.clip_position_0;

#line 1623
    (&_S179)->previous_clip_position_2 = _S178.previous_clip_position_0;

#line 1623
    (&_S179)->world_tangent_2 = _S178.world_tangent_0;

#line 1623
    (&_S179)->frame_4 = _S178.frame_1;

#line 1623
    return _S179;
}

