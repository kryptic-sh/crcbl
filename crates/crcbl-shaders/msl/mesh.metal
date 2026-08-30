#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 1647 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 1642
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1914
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 1974
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 2126
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 1989
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 2017
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 964
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1367
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1367
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


#line 736
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
};


#line 1373
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1373
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
    uint base_color_texture_0;
    float metallic_0;
    float roughness_0;
    uint tiling_0;
    float tile_metres_0;
    float emissive_r_0;
    float emissive_g_0;
    float emissive_b_0;
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
    texture2d_array<float, access::sample> base_color_textures_0;
    sampler base_color_sampler_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> specular_albedo_0;
    GpuProbe_natural_0 device* probes_0;
};


#line 1007 "shaders/mesh.slang"
float3 load_position_0(uint at_0, KernelContext_0 thread* kernelContext_0)
{
    uint word_0 = at_0 * 3U;
    return float3((as_type<float>((kernelContext_0->vertices_0[word_0]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 1U]))), (as_type<float>((kernelContext_0->vertices_0[word_0 + 2U]))));
}


#line 177
float dequantise_snorm_0(int lane_0)
{
    return max(float(lane_0) / 32767.0f, -1.0f);
}


float4 unpack_snorm16x4_0(uint low_0, uint high_0)
{
    return float4(dequantise_snorm_0((as_type<int>((low_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((low_0))) >> 16U), dequantise_snorm_0((as_type<int>((high_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((high_0))) >> 16U));
}


#line 209
float3 rotate_by_0(float4 q_0, float3 v_0)
{
    float3 _S1 = q_0.xyz;

#line 211
    float3 t_0 = float3(2.0f)  * cross(_S1, v_0);
    return v_0 + float3(q_0.w)  * t_0 + cross(_S1, t_0);
}


#line 167
struct TangentFrame_0
{
    float3 tangent_0;
    float3 bitangent_0;
    float3 normal_0;
};


#line 223
TangentFrame_0 decode_qtangent_0(float4 lanes_0)
{
    float4 q_1 = normalize(lanes_0);
    thread TangentFrame_0 basis_0;
    float3 _S2 = rotate_by_0(q_1, float3(1.0f, 0.0f, 0.0f));

#line 227
    (&basis_0)->tangent_0 = _S2;
    float3 _S3 = rotate_by_0(q_1, float3(0.0f, 0.0f, 1.0f));

#line 228
    (&basis_0)->normal_0 = _S3;
    float3 _S4 = cross(_S3, _S2);

#line 229
    float _S5;

#line 229
    if((lanes_0.w) < 0.0f)
    {

#line 229
        _S5 = -1.0f;

#line 229
    }
    else
    {

#line 229
        _S5 = 1.0f;

#line 229
    }

#line 229
    (&basis_0)->bitangent_0 = _S4 * float3(_S5) ;
    return basis_0;
}


#line 192
float2 unpack_unorm16x2_0(uint word_1)
{
    return float2(float(word_1 & 65535U), float(word_1 >> 16U)) / float2(65535.0f) ;
}


float4 unpack_rgba8_0(uint word_2)
{
    return float4(float(word_2 & 255U), float((word_2 >> 8U) & 255U), float((word_2 >> 16U) & 255U), float(word_2 >> 24U)) / float4(255.0f) ;
}


#line 238
struct MeshVertex_0
{
    float3 position_1;
    TangentFrame_0 basis_1;
    float2 uv0_0;
    float4 color_1;
};


#line 1018
MeshVertex_0 load_vertex_0(uint at_1, float4 range_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_3 = kernelContext_1->frame_0->vertex_pool_0.x + at_1 * 5U;
    thread MeshVertex_0 vertex_0;

#line 1021
    float3 _S6 = load_position_0(at_1, kernelContext_1);
    (&vertex_0)->position_1 = _S6;
    (&vertex_0)->basis_1 = decode_qtangent_0(unpack_snorm16x4_0(kernelContext_1->vertices_0[word_3], kernelContext_1->vertices_0[word_3 + 1U]));
    (&vertex_0)->uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(kernelContext_1->vertices_0[word_3 + 2U]);
    (&vertex_0)->color_1 = unpack_rgba8_0(kernelContext_1->vertices_0[word_3 + 4U]);
    return vertex_0;
}


#line 1298
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 3039
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S7 = previous_0.w;

#line 3041
    if(_S7 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S7) ) * float2(0.5f, -0.5f);
}


#line 3007
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_2)
{

#line 3007
    texture2d<float, access::sample> _S8 = kernelContext_2->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S8).get_width(0)),(*((&height_0)) = (_S8).get_height(0));

    int3 _S9 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 3013
    return ((kernelContext_2->ambient_occlusion_0).read(vec<uint,2>(((_S9)).xy), uint(((_S9)).z)).x);
}


#line 1782
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1793
    float3 _S10;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1794
        _S10 = - facet_1;

#line 1794
    }
    else
    {

#line 1794
        _S10 = facet_1;

#line 1794
    }

#line 1794
    return _S10;
}


#line 2977
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S11 = axis_0.x;

#line 2981
    float _S12 = axis_0.y;

#line 2981
    bool _S13;

#line 2981
    if(_S11 >= _S12)
    {

#line 2981
        _S13 = _S11 >= (axis_0.z);

#line 2981
    }
    else
    {

#line 2981
        _S13 = false;

#line 2981
    }

#line 2981
    float2 planar_0;

#line 2981
    if(_S13)
    {

#line 2981
        planar_0 = world_position_1.zy;

#line 2981
    }
    else
    {

        if(_S12 >= (axis_0.z))
        {

#line 2985
            planar_0 = world_position_1.xz;

#line 2985
        }
        else
        {

#line 2985
            planar_0 = world_position_1.xy;

#line 2985
        }

#line 2981
    }

#line 2993
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 2775
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_3)
{
    uint _S14 = max(kernelContext_3->frame_0->cluster_grid_0.x, 1U);
    uint _S15 = max(kernelContext_3->frame_0->cluster_grid_0.y, 1U);
    uint _S16 = max(kernelContext_3->frame_0->cluster_grid_0.z, 1U);
    uint _S17 = max(kernelContext_3->frame_0->cluster_grid_0.w, 1U);

#line 2785
    uint _S18 = uint(pixel_0.x) / _S17;

#line 2785
    uint _S19 = min(_S18, _S14 - 1U);
    uint _S20 = uint(pixel_0.y) / _S17;

    float scale_0 = 24.0f / log2(10000.0f);

#line 2796
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S16 - 1U))) * _S15 + min(_S20, _S15 - 1U)) * _S14 + _S19;
}


#line 2740
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 2754
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 2761
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 1506
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 1513
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1520
    float _S21 = 1.0f - alpha2_0;

#line 1525
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S21 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S21 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 1833
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 2148
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 319
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 2264
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_4)
{
    float2 texel_0 = kernelContext_4->frame_0->shadow_params_0.xy;
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S22 = float2(0.5f, 0.5f) * texel_0 * grid_0;


    float2 _S23 = float2(1.0f, 1.0f);

#line 2271
    float2 _S24 = _S23 / texel_0;

#line 2271
    uint index_0 = 0U;

#line 2271
    float sum_0 = 0.0f;

#line 2271
    float found_0 = 0.0f;



    for(;;)
    {

#line 2275
        if(index_0 < 16U)
        {
        }
        else
        {

#line 2275
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_0] * float2(8.0f) ;
        float _S25 = spoke_0.x;

#line 2278
        float _S26 = rotation_0.x;

#line 2278
        float _S27 = spoke_0.y;

#line 2278
        float _S28 = rotation_0.y;

#line 2287
        int3 _S29 = int3(int2(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + float2(_S25 * _S26 - _S27 * _S28, _S25 * _S28 + _S27 * _S26) * texel_0 * grid_0, _S22, float2(1.0f)  - _S22)) * _S24, _S24 - _S23)), int(0));

#line 2287
        float depth_1 = ((kernelContext_4->shadow_atlas_0).read(vec<uint,2>(((_S29)).xy), uint(((_S29)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 2291
            sum_0 = sum_0 + depth_1;

#line 2291
            found_0 = found_1;

#line 2288
        }

#line 2275
        index_0 = index_0 + 1U;

#line 2275
    }

#line 2295
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 2306
    float _S30 = 2.0f * kernelContext_4->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_0 / found_0 - reference_0) * (_S30 + 40.0f) * 0.01999999955296516f / (_S30 / 768.0f), 2.0f, 8.0f);
}


#line 2166
float tile_tap_0(uint tile_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_5)
{
    float2 texel_1 = kernelContext_5->frame_0->shadow_params_0.xy;

#line 2173
    float2 grid_1 = float2(4.0f, 4.0f);
    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_1 * grid_1;

    float _S31 = spoke_1.x;

#line 2176
    float _S32 = rotation_1.x;

#line 2176
    float _S33 = spoke_1.y;

#line 2176
    float _S34 = rotation_1.y;


    float _S35 = ((kernelContext_5->shadow_atlas_0).sample_compare((kernelContext_5->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + float2(_S31 * _S32 - _S33 * _S34, _S31 * _S34 + _S33 * _S32) * texel_1 * grid_1, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2179
    return _S35;
}


#line 2201
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_1, KernelContext_0 thread* kernelContext_6)
{
    float2 _S36 = shadow_rotation_0(pixel_2);

#line 2203
    uint spot_0 = 0U;

#line 2203
    float probe_0 = 0.0f;


    for(;;)
    {

#line 2206
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2206
            break;
        }

#line 2206
        float _S37 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_1) , _S36, reference_2, kernelContext_6);

        float probe_1 = probe_0 + _S37;

#line 2206
        spot_0 = spot_0 + 1U;

#line 2206
        probe_0 = probe_1;

#line 2206
    }

#line 2215
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2221
    uint index_1 = 0U;

#line 2221
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2225
        if(index_1 < 32U)
        {
        }
        else
        {

#line 2225
            break;
        }

#line 2225
        float _S38 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[index_1] * float2(radius_1) , _S36, reference_2, kernelContext_6);

        float visibility_1 = visibility_0 + _S38;

#line 2225
        index_1 = index_1 + 1U;

#line 2225
        visibility_0 = visibility_1;

#line 2225
    }



    return visibility_0 / 32.0f;
}


#line 2360
float cascade_visibility_0(uint cascade_1, float3 world_position_2, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_7)
{

#line 2391
    float texel_world_0 = 2.0f * kernelContext_7->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 2398
    float4 clip_0 = (((float4(world_position_2 + geometric_normal_1 * float3((texel_world_0 * kernelContext_7->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_7->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_7->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 2402
    bool _S39;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 2403
        _S39 = true;

#line 2403
    }
    else
    {

#line 2403
        _S39 = (ndc_0.z) <= 0.0f;

#line 2403
    }

#line 2403
    if(_S39)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 2430
    float _S40 = ndc_0.z;

#line 2430
    float _S41 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S40, shadow_rotation_0(pixel_3), kernelContext_7);

#line 2430
    float _S42 = tile_pcf_0(cascade_1, tile_uv_4, _S40, pixel_3, _S41, kernelContext_7);
    return _S42;
}


#line 2447
float sun_visibility_0(float3 world_position_3, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_8)
{

#line 2448
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 2460
    float eye_distance_0 = length(world_position_3 - kernelContext_8->frame_0->camera_position_0.xyz);

#line 2460
    uint index_2 = 0U;

    for(;;)
    {

#line 2462
        if(index_2 < 2U)
        {
        }
        else
        {

#line 2462
            cascade_2 = 1U;

#line 2462
            break;
        }
        if(eye_distance_0 < kernelContext_8->frame_0->cascade_far_0[index_2])
        {

#line 2464
            cascade_2 = index_2;


            break;
        }

#line 2462
        index_2 = index_2 + 1U;

#line 2462
    }

#line 2462
    float _S43 = cascade_visibility_0(cascade_2, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_8);

#line 2473
    uint _S44 = cascade_2 + 1U;

#line 2473
    if(_S44 >= 2U)
    {



        return _S43;
    }

#line 2486
    float band_0 = kernelContext_8->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_8->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S43;
    }

#line 2490
    float _S45 = cascade_visibility_0(_S44, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_8);

#line 2501
    return mix(_S43, _S45, blend_0);
}


#line 2691
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S46 = axis_2.x;

#line 2694
    float _S47 = axis_2.y;

#line 2694
    bool _S48;

#line 2694
    if(_S46 >= _S47)
    {

#line 2694
        _S48 = _S46 >= (axis_2.z);

#line 2694
    }
    else
    {

#line 2694
        _S48 = false;

#line 2694
    }

#line 2694
    uint _S49;

#line 2694
    if(_S48)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 2696
            _S49 = 0U;

#line 2696
        }
        else
        {

#line 2696
            _S49 = 1U;

#line 2696
        }

#line 2696
        return _S49;
    }
    if(_S47 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 2700
            _S49 = 2U;

#line 2700
        }
        else
        {

#line 2700
            _S49 = 3U;

#line 2700
        }

#line 2700
        return _S49;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 2702
        _S49 = 4U;

#line 2702
    }
    else
    {

#line 2702
        _S49 = 5U;

#line 2702
    }

#line 2702
    return _S49;
}


#line 307
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 2604
float punctual_visibility_0(uint tile_4, float3 world_position_4, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_9)
{

#line 2616
    float4 clip_1 = (((float4(world_position_4 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_9->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 2623
    float _S50 = clip_1.w;

#line 2623
    if(_S50 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S50) ;

#line 2627
    bool _S51;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 2628
        _S51 = true;

#line 2628
    }
    else
    {

#line 2628
        _S51 = (ndc_1.z) <= 0.0f;

#line 2628
    }

#line 2628
    if(_S51)
    {

#line 2628
        _S51 = true;

#line 2628
    }
    else
    {

#line 2628
        _S51 = (ndc_1.z) > 1.0f;

#line 2628
    }

#line 2628
    if(_S51)
    {

#line 2635
        return 1.0f;
    }

#line 2635
    float _S52 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_9);

#line 2645
    return _S52;
}


#line 2710
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_5, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_10)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_5 - (float4(light_0->position_0) ).xyz;

#line 2718
    float _S53 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_5, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_10);

#line 2724
    return _S53;
}


#line 2652
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_5, float3 world_position_6, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_11)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 2659
    float4 _S54 = float4(light_1->direction_0) ;

#line 2666
    float cos_outer_1 = _S54.w;

#line 2666
    float _S55 = punctual_visibility_0(tile_5, world_position_6, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_6 - (float4(light_1->position_0) ).xyz, normalize(_S54.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_11);

#line 2673
    return _S55;
}


#line 1550
float decode_specular_albedo_0(float2 texel_2)
{
    return (texel_2.x * 65280.0f + texel_2.y * 255.0f) / 65535.0f;
}


#line 1567
float specular_albedo_at_0(float n_dot_v_1, float roughness_1, KernelContext_0 thread* kernelContext_12)
{

#line 1567
    texture2d<float, access::sample> _S56 = kernelContext_12->specular_albedo_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S56).get_width(0)),(*((&height_1)) = (_S56).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1573
    float2 _S57 = float2(1.0f) ;
    float2 _S58 = extent_1 - _S57;

#line 1574
    float2 low_1 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S58);

    float2 weight_0 = clamp(scaled_0 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S59 = int2(low_1);
    int2 _S60 = int2(min(low_1 + _S57, _S58));
    int _S61 = _S59.x;

#line 1580
    int _S62 = _S59.y;

#line 1580
    int3 _S63 = int3(_S61, _S62, int(0));
    int _S64 = _S60.x;

#line 1581
    int3 _S65 = int3(_S64, _S62, int(0));
    float _S66 = weight_0.x;
    int _S67 = _S60.y;

#line 1583
    int3 _S68 = int3(_S61, _S67, int(0));
    int3 _S69 = int3(_S64, _S67, int(0));

    return mix(mix(decode_specular_albedo_0(((kernelContext_12->specular_albedo_0).read(vec<uint,2>(((_S63)).xy), uint(((_S63)).z)).xy)), decode_specular_albedo_0(((kernelContext_12->specular_albedo_0).read(vec<uint,2>(((_S65)).xy), uint(((_S65)).z)).xy)), _S66), mix(decode_specular_albedo_0(((kernelContext_12->specular_albedo_0).read(vec<uint,2>(((_S68)).xy), uint(((_S68)).z)).xy)), decode_specular_albedo_0(((kernelContext_12->specular_albedo_0).read(vec<uint,2>(((_S69)).xy), uint(((_S69)).z)).xy)), _S66), weight_0.y);
}


#line 1605
float3 specular_compensation_0(float3 f0_1, float n_dot_v_2, float roughness_2, KernelContext_0 thread* kernelContext_13)
{

#line 1605
    float _S70 = specular_albedo_at_0(n_dot_v_2, roughness_2, kernelContext_13);



    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(_S70, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 2904
float3 sky_irradiance_0(float3 normal_2, KernelContext_0 thread* kernelContext_14)
{
    float4 basis_3 = float4(normal_2, 1.0f);
    return max(float3(dot(kernelContext_14->frame_0->sky_sh_r_0, basis_3), dot(kernelContext_14->frame_0->sky_sh_g_0, basis_3), dot(kernelContext_14->frame_0->sky_sh_b_0, basis_3)), float3(0.0f, 0.0f, 0.0f));
}


#line 875
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 2806
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_15)
{

    GpuProbe_natural_0 _S71 = kernelContext_15->probes_0[min((cell_1.z * kernelContext_15->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_15->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_15->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 2809
    GpuProbe_0 _S72 = { float4(_S71.sh_r_0) , float4(_S71.sh_g_0) , float4(_S71.sh_b_0)  };

#line 2809
    return _S72;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_1)
{
    thread GpuProbe_0 blended_0;
    float4 _S73 = float4(t_1) ;

#line 2817
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S73);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S73);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S73);
    return blended_0;
}


#line 2857
float3 probe_irradiance_0(float3 world_position_7, float3 normal_3, KernelContext_0 thread* kernelContext_16)
{

#line 2857
    float3 _S74 = float3(1.0f) ;

#line 2862
    float3 _S75 = float3(0.0f, 0.0f, 0.0f);

#line 2862
    float3 last_0 = max(float3(kernelContext_16->frame_0->probe_counts_0.xyz) - _S74, _S75);
    float3 grid_2 = clamp((world_position_7 - kernelContext_16->frame_0->probe_origin_0.xyz) * kernelContext_16->frame_0->probe_inv_spacing_0.xyz, _S75, last_0);

    float3 base_2 = floor(grid_2);
    float3 f_0 = grid_2 - base_2;

    uint3 _S76 = uint3(base_2);



    uint3 _S77 = uint3(min(base_2 + _S74, last_0));

#line 2879
    uint _S78 = _S76.x;

#line 2879
    uint _S79 = _S76.y;

#line 2879
    uint _S80 = _S76.z;

#line 2879
    GpuProbe_0 _S81 = probe_at_0(uint3(_S78, _S79, _S80), kernelContext_16);

#line 2879
    uint _S82 = _S77.x;

#line 2879
    GpuProbe_0 _S83 = probe_at_0(uint3(_S82, _S79, _S80), kernelContext_16);

#line 2879
    float _S84 = f_0.x;

#line 2879
    thread GpuProbe_0 _S85 = _S81;

#line 2879
    thread GpuProbe_0 _S86 = _S83;

#line 2879
    GpuProbe_0 _S87 = lerp_probe_0(&_S85, &_S86, _S84);
    uint _S88 = _S77.y;

#line 2880
    GpuProbe_0 _S89 = probe_at_0(uint3(_S78, _S88, _S80), kernelContext_16);

#line 2880
    GpuProbe_0 _S90 = probe_at_0(uint3(_S82, _S88, _S80), kernelContext_16);

#line 2880
    thread GpuProbe_0 _S91 = _S89;

#line 2880
    thread GpuProbe_0 _S92 = _S90;

#line 2880
    GpuProbe_0 _S93 = lerp_probe_0(&_S91, &_S92, _S84);
    uint _S94 = _S77.z;

#line 2881
    GpuProbe_0 _S95 = probe_at_0(uint3(_S78, _S79, _S94), kernelContext_16);

#line 2881
    GpuProbe_0 _S96 = probe_at_0(uint3(_S82, _S79, _S94), kernelContext_16);

#line 2881
    thread GpuProbe_0 _S97 = _S95;

#line 2881
    thread GpuProbe_0 _S98 = _S96;

#line 2881
    GpuProbe_0 _S99 = lerp_probe_0(&_S97, &_S98, _S84);

#line 2881
    GpuProbe_0 _S100 = probe_at_0(uint3(_S78, _S88, _S94), kernelContext_16);

#line 2881
    GpuProbe_0 _S101 = probe_at_0(uint3(_S82, _S88, _S94), kernelContext_16);

#line 2881
    thread GpuProbe_0 _S102 = _S100;

#line 2881
    thread GpuProbe_0 _S103 = _S101;

#line 2881
    GpuProbe_0 _S104 = lerp_probe_0(&_S102, &_S103, _S84);

    float _S105 = f_0.y;

#line 2883
    thread GpuProbe_0 _S106 = _S87;

#line 2883
    thread GpuProbe_0 _S107 = _S93;

#line 2883
    GpuProbe_0 _S108 = lerp_probe_0(&_S106, &_S107, _S105);

#line 2883
    thread GpuProbe_0 _S109 = _S99;

#line 2883
    thread GpuProbe_0 _S110 = _S104;

#line 2883
    GpuProbe_0 _S111 = lerp_probe_0(&_S109, &_S110, _S105);

    float _S112 = f_0.z;

#line 2885
    thread GpuProbe_0 _S113 = _S108;

#line 2885
    thread GpuProbe_0 _S114 = _S111;

#line 2885
    GpuProbe_0 _S115 = lerp_probe_0(&_S113, &_S114, _S112);

    float4 basis_4 = float4(normal_3, 1.0f);
    return max(float3(dot(_S115.sh_r_0, basis_4), dot(_S115.sh_g_0, basis_4), dot(_S115.sh_b_0, basis_4)), _S75);
}


#line 848
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_1)
{
    return float3(material_1->emissive_r_0, material_1->emissive_g_0, material_1->emissive_b_0);
}


#line 1667
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S116 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 1675
    float kernel_0 = 0.0001984127011383f;

#line 1675
    int term_0 = int(6);

    for(;;)
    {

#line 1677
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 1677
            break;
        }
        float _S117 = kernel_0 * _S116 + FOG_KERNEL_0[term_0];

#line 1677
        int term_1 = term_0 - int(1);

#line 1677
        kernel_0 = _S117;

#line 1677
        term_0 = term_1;

#line 1677
    }

#line 1684
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 1694
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S118 = - d_0;

#line 1698
        float series_0 = 0.00833333376795053f;

#line 1698
        int term_2 = int(3);

        for(;;)
        {

#line 1700
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 1700
                break;
            }
            float _S119 = series_0 * _S118 + FOG_RATIO_KERNEL_0[term_2];

#line 1700
            int term_3 = term_2 - int(1);

#line 1700
            series_0 = _S119;

#line 1700
            term_2 = term_3;

#line 1700
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 1728
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_1)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_1, 0.0f, 32.0f);
    }

#line 1739
    return clamp(density_0 * distance_1 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 1747
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 2930
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 2930
struct pixelInput_0
{
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_2 [[user(TEXCOORD)]];
    float2 uv_0 [[user(TEXCOORD_1)]];
    float4 clip_position_0 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_0 [[user(TEXCOORD_3)]];
};


#line 3049
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S120 [[stage_in]], float4 position_3 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], uint device* vertices_1 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_1 [[texture(3)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 3049
    thread KernelContext_0 kernelContext_17;

#line 3049
    (&kernelContext_17)->draw_0 = draw_1;

#line 3049
    (&kernelContext_17)->visible_instances_0 = visible_instances_1;

#line 3049
    (&kernelContext_17)->instances_0 = instances_1;

#line 3049
    (&kernelContext_17)->meshes_0 = meshes_1;

#line 3049
    (&kernelContext_17)->frame_0 = frame_1;

#line 3049
    (&kernelContext_17)->vertices_0 = vertices_1;

#line 3049
    (&kernelContext_17)->ambient_occlusion_0 = ambient_occlusion_1;

#line 3049
    (&kernelContext_17)->materials_0 = materials_1;

#line 3049
    (&kernelContext_17)->base_color_textures_0 = base_color_textures_1;

#line 3049
    (&kernelContext_17)->base_color_sampler_0 = base_color_sampler_1;

#line 3049
    (&kernelContext_17)->cluster_lights_0 = cluster_lights_1;

#line 3049
    (&kernelContext_17)->lights_0 = lights_1;

#line 3049
    (&kernelContext_17)->shadow_atlas_0 = shadow_atlas_1;

#line 3049
    (&kernelContext_17)->shadow_sampler_0 = shadow_sampler_1;

#line 3049
    (&kernelContext_17)->specular_albedo_0 = specular_albedo_1;

#line 3049
    (&kernelContext_17)->probes_0 = probes_1;

#line 3055
    float3 normal_4 = normalize(_S120.world_normal_0);

#line 3060
    float2 motion_1 = motion_vector_0(_S120.clip_position_0, _S120.previous_clip_position_0);

#line 3069
    if((frame_1->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 3111
    if((frame_1->ambient_0.w) >= 3.5f)
    {

#line 3111
        float _S121 = occlusion_at_0(position_3.xy, &kernelContext_17);

        thread FragmentOutput_0 occlusion_0;

#line 3122
        (&occlusion_0)->lit_0 = float4(_S121, _S121, _S121, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_1->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S120.color_2.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

    if((frame_1->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 3144
        float3 _S122 = float3(0.5f) ;

#line 3151
        (&normals_0)->lit_0 = float4(normal_4 * _S122 + _S122, 1.0f);

#line 3157
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_17)->frame_0->camera_position_0.xyz - _S120.world_position_8);



    float3 _S123 = geometric_normal_of_0(_S120.world_position_8, normal_4);

#line 3166
    thread GpuMaterial_natural_0 _S124 = (&kernelContext_17)->materials_0[_S120.material_2];

#line 3166
    float2 uv_1;

#line 3185
    if(((&_S124)->tiling_0) == 1U)
    {

#line 3185
        uv_1 = physical_tile_uv_0(_S120.world_position_8, normal_4, (&_S124)->tile_metres_0);

#line 3185
    }
    else
    {

#line 3185
        uv_1 = _S120.uv_0;

#line 3185
    }

#line 3190
    float3 _S125 = float3(uv_1, float((&_S124)->base_color_texture_0));
    float4 albedo_0 = _S120.color_2 * float4((&_S124)->base_color_0)  * (((&kernelContext_17)->base_color_textures_0).sample(((&kernelContext_17)->base_color_sampler_0), ((_S125)).xy, uint(((_S125)).z)));

#line 3197
    float metallic_1 = saturate((&_S124)->metallic_0);
    float roughness_3 = clamp((&_S124)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_3 * roughness_3;
    float _S126 = alpha_0 * alpha_0;

#line 3206
    float3 _S127 = albedo_0.xyz;

#line 3206
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S127, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S127 * float3((1.0f - metallic_1)) ;

#line 3213
    float _S128 = max(dot(normal_4, to_eye_0), 0.00009999999747379f);

#line 3223
    float2 _S129 = position_3.xy;

#line 3223
    uint _S130 = froxel_of_0(_S129, (((float4(_S120.world_position_8, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_17)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_17)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_17);

#line 3223
    uint base_3 = _S130 * 17U;

#line 3228
    uint _S131 = min((&kernelContext_17)->cluster_lights_0[base_3], 16U);

#line 3234
    float3 _S132 = float3(0.0f, 0.0f, 0.0f);

#line 3234
    uint slot_0 = 0U;

#line 3234
    float3 direct_0 = _S132;

#line 3234
    float3 gloss_0 = _S132;

    for(;;)
    {

#line 3236
        if(slot_0 < _S131)
        {
        }
        else
        {

#line 3236
            break;
        }

#line 3236
        thread GpuLight_natural_0 _S133 = (&kernelContext_17)->lights_0[(&kernelContext_17)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 3236
        uint _S134 = (&_S133)->kind_0;

#line 3245
        bool _S135 = ((&_S133)->kind_0) == 0U;

#line 3245
        float3 to_light_7;

#line 3245
        float reach_0;

#line 3245
        if(_S135)
        {

#line 3245
            to_light_7 = normalize((float4((&_S133)->direction_0) ).xyz);

#line 3245
            reach_0 = 1.0f;

#line 3245
        }
        else
        {

#line 3245
            float4 _S136 = float4((&_S133)->position_0) ;

#line 3252
            float3 offset_0 = _S136.xyz - _S120.world_position_8;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S136.w);
            if(_S134 == 2U)
            {

#line 3256
                float4 _S137 = float4((&_S133)->direction_0) ;

#line 3256
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S137.xyz, _S137.w, (&_S133)->cos_inner_0);

#line 3256
            }
            else
            {

#line 3256
                reach_0 = reach_1;

#line 3256
            }

#line 3256
            to_light_7 = to_light_8;

#line 3245
        }

#line 3263
        float n_dot_l_5 = dot(normal_4, to_light_7);
        float _S138 = max(n_dot_l_5, 0.0f);

#line 3270
        float3 half_vector_0 = normalize(to_light_7 + to_eye_0);

#line 3277
        float3 specular_0 = ggx_lobe_0(_S126, f0_2, _S138, _S128, max(dot(normal_4, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S138) ;

#line 3277
        float reach_2;

#line 3292
        if(_S135)
        {

#line 3292
            float _S139 = sun_visibility_0(_S120.world_position_8, to_light_7, n_dot_l_5, _S123, _S129, &kernelContext_17);

#line 3292
            reach_2 = _S139;

#line 3292
        }
        else
        {


            if(_S134 == 1U)
            {

#line 3297
                uint _S140 = (&_S133)->shadow_tile_0;

#line 3309
                if(((&_S133)->shadow_tile_0) <= 8U)
                {

#line 3309
                    float _S141 = point_visibility_0(&_S133, _S140, _S120.world_position_8, to_light_7, n_dot_l_5, _S123, _S129, &kernelContext_17);

#line 3309
                    reach_2 = reach_0 * _S141;

#line 3309
                }
                else
                {

#line 3309
                    reach_2 = reach_0;

#line 3309
                }

#line 3297
            }
            else
            {

#line 3297
                uint _S142 = (&_S133)->shadow_tile_0;

#line 3315
                if(((&_S133)->shadow_tile_0) < 14U)
                {

#line 3315
                    float _S143 = spot_visibility_0(&_S133, _S142, _S120.world_position_8, to_light_7, n_dot_l_5, _S123, _S129, &kernelContext_17);

#line 3315
                    reach_2 = reach_0 * _S143;

#line 3315
                }
                else
                {

#line 3315
                    reach_2 = reach_0;

#line 3315
                }

#line 3297
            }

#line 3292
        }

#line 3323
        float3 _S144 = (float4((&_S133)->color_0) ).xyz;

#line 3323
        float3 direct_1 = direct_0 + _S144 * float3((_S138 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S144 * (specular_0 * float3(reach_2) );

#line 3236
        slot_0 = slot_0 + 1U;

#line 3236
        direct_0 = direct_1;

#line 3236
        gloss_0 = gloss_1;

#line 3236
    }

#line 3236
    float3 _S145 = specular_compensation_0(f0_2, _S128, roughness_3, &kernelContext_17);

#line 3338
    float3 gloss_2 = gloss_0 * _S145;

#line 3338
    float _S146 = occlusion_at_0(_S129, &kernelContext_17);

#line 3374
    float3 _S147 = frame_1->ambient_0.xyz;

#line 3374
    float3 _S148 = sky_irradiance_0(normal_4, &kernelContext_17);

#line 3374
    float3 _S149 = _S147 + _S148;

#line 3374
    float3 _S150 = probe_irradiance_0(_S120.world_position_8, normal_4, &kernelContext_17);

#line 3395
    float3 lit_1 = diffuse_albedo_0 * ((_S149 + _S150) * float3(_S146)  + direct_0) + gloss_2;

#line 3395
    float3 _S151 = emissive_of_0(&_S124);

#line 3431
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_17)->frame_0->fog_params_0.x, (&kernelContext_17)->frame_0->fog_params_0.y, (&kernelContext_17)->frame_0->camera_position_0.y - (&kernelContext_17)->frame_0->fog_params_0.z, _S120.world_position_8.y - (&kernelContext_17)->frame_0->fog_params_0.z, length((&kernelContext_17)->frame_0->camera_position_0.xyz - _S120.world_position_8)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S151) * float3(fog_survives_0)  + (&kernelContext_17)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_0)->reflectivity_0 = float4(f0_2, floor(roughness_3 * 255.0f + 0.5f) / 255.0f);

    (&output_0)->motion_0 = motion_1;
    return output_0;
}


#line 3444
struct vertexMain_Result_0
{
    float4 position_4 [[position]];
    float3 world_position_9 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
};


#line 1305
struct VertexOutput_0
{
    float4 position_5;
    float3 world_position_10;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_3;
    float4 clip_position_2;
    float4 previous_clip_position_2;
};


#line 1305
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], uint device* vertices_2 [[buffer(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_2 [[texture(3)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1305
    thread KernelContext_0 kernelContext_18;

#line 1305
    (&kernelContext_18)->draw_0 = draw_2;

#line 1305
    (&kernelContext_18)->visible_instances_0 = visible_instances_2;

#line 1305
    (&kernelContext_18)->instances_0 = instances_2;

#line 1305
    (&kernelContext_18)->meshes_0 = meshes_2;

#line 1305
    (&kernelContext_18)->frame_0 = frame_2;

#line 1305
    (&kernelContext_18)->vertices_0 = vertices_2;

#line 1305
    (&kernelContext_18)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1305
    (&kernelContext_18)->materials_0 = materials_2;

#line 1305
    (&kernelContext_18)->base_color_textures_0 = base_color_textures_2;

#line 1305
    (&kernelContext_18)->base_color_sampler_0 = base_color_sampler_2;

#line 1305
    (&kernelContext_18)->cluster_lights_0 = cluster_lights_2;

#line 1305
    (&kernelContext_18)->lights_0 = lights_2;

#line 1305
    (&kernelContext_18)->shadow_atlas_0 = shadow_atlas_2;

#line 1305
    (&kernelContext_18)->shadow_sampler_0 = shadow_sampler_2;

#line 1305
    (&kernelContext_18)->specular_albedo_0 = specular_albedo_2;

#line 1305
    (&kernelContext_18)->probes_0 = probes_2;

#line 1305
    GpuInstance_natural_0 device* _S152 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1373
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1381
    bool _S153 = ((_S152->flags_0) & 2U) != 0U;

#line 1381
    uint base_vertex_2;
    if(_S153)
    {

#line 1382
        base_vertex_2 = _S152->base_vertex_0;

#line 1382
    }
    else
    {

#line 1382
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1382
    }

#line 1382
    MeshVertex_0 _S154 = load_vertex_0(index_3 + base_vertex_2, float4(mesh_2.uv_scale_u_0, mesh_2.uv_scale_v_0, mesh_2.uv_offset_u_0, mesh_2.uv_offset_v_0), &kernelContext_18);

#line 1382
    uint previous_base_0;

#line 1395
    if(_S153)
    {

#line 1395
        previous_base_0 = _S152->previous_base_vertex_0;

#line 1395
    }
    else
    {

#line 1395
        previous_base_0 = base_vertex_2;

#line 1395
    }

#line 1395
    float3 _S155 = load_position_0(index_3 + previous_base_0, &kernelContext_18);

#line 1395
    matrix<float,int(4),int(4)>  _S156 = matrix<float,int(4),int(4)> (_S152->transform_0.data_0[int(0)][int(0)], _S152->transform_0.data_0[int(1)][int(0)], _S152->transform_0.data_0[int(2)][int(0)], _S152->transform_0.data_0[int(3)][int(0)], _S152->transform_0.data_0[int(0)][int(1)], _S152->transform_0.data_0[int(1)][int(1)], _S152->transform_0.data_0[int(2)][int(1)], _S152->transform_0.data_0[int(3)][int(1)], _S152->transform_0.data_0[int(0)][int(2)], _S152->transform_0.data_0[int(1)][int(2)], _S152->transform_0.data_0[int(2)][int(2)], _S152->transform_0.data_0[int(3)][int(2)], _S152->transform_0.data_0[int(0)][int(3)], _S152->transform_0.data_0[int(1)][int(3)], _S152->transform_0.data_0[int(2)][int(3)], _S152->transform_0.data_0[int(3)][int(3)]);



    float4 world_0 = (((float4(_S154.position_1, 1.0f)) * (_S156)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_5 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_18)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_10 = world_0.xyz;

#line 1409
    (&output_1)->world_normal_2 = (((_S154.basis_1.normal_0) * (normal_basis_0(matrix<float,int(3),int(3)> (_S156[int(0)].xyz, _S156[int(1)].xyz, _S156[int(2)].xyz)))));

#line 1409
    float4 _S157;

#line 1416
    if(((&kernelContext_18)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1416
        _S157 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1416
    }
    else
    {

#line 1416
        _S157 = _S154.color_1;

#line 1416
    }

#line 1415
    (&output_1)->color_4 = _S157;

#line 1422
    (&output_1)->material_4 = _S152->material_0;
    (&output_1)->uv_3 = _S154.uv0_0;

#line 1429
    (&output_1)->clip_position_2 = (&output_1)->position_5;
    (&output_1)->previous_clip_position_2 = ((((((float4(_S155, 1.0f)) * (matrix<float,int(4),int(4)> (_S152->previous_transform_0.data_0[int(0)][int(0)], _S152->previous_transform_0.data_0[int(1)][int(0)], _S152->previous_transform_0.data_0[int(2)][int(0)], _S152->previous_transform_0.data_0[int(3)][int(0)], _S152->previous_transform_0.data_0[int(0)][int(1)], _S152->previous_transform_0.data_0[int(1)][int(1)], _S152->previous_transform_0.data_0[int(2)][int(1)], _S152->previous_transform_0.data_0[int(3)][int(1)], _S152->previous_transform_0.data_0[int(0)][int(2)], _S152->previous_transform_0.data_0[int(1)][int(2)], _S152->previous_transform_0.data_0[int(2)][int(2)], _S152->previous_transform_0.data_0[int(3)][int(2)], _S152->previous_transform_0.data_0[int(0)][int(3)], _S152->previous_transform_0.data_0[int(1)][int(3)], _S152->previous_transform_0.data_0[int(2)][int(3)], _S152->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_18)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S158 = output_1;

#line 1433
    thread vertexMain_Result_0 _S159;

#line 1433
    (&_S159)->position_4 = _S158.position_5;

#line 1433
    (&_S159)->world_position_9 = _S158.world_position_10;

#line 1433
    (&_S159)->world_normal_1 = _S158.world_normal_2;

#line 1433
    (&_S159)->color_3 = _S158.color_4;

#line 1433
    (&_S159)->material_3 = _S158.material_4;

#line 1433
    (&_S159)->uv_2 = _S158.uv_3;

#line 1433
    (&_S159)->clip_position_1 = _S158.clip_position_2;

#line 1433
    (&_S159)->previous_clip_position_1 = _S158.previous_clip_position_2;

#line 1433
    return _S159;
}

