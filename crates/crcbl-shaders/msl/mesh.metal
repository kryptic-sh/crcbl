#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 1380 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 1375
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1639
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 1808
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 1671
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 1699
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1089
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_0)
{
    return matrix<float,int(3),int(3)> (cross(basis_0[int(1)], basis_0[int(2)]), cross(basis_0[int(2)], basis_0[int(0)]), cross(basis_0[int(0)], basis_0[int(1)]));
}


#line 1515
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1526
    float3 _S1;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1527
        _S1 = - facet_1;

#line 1527
    }
    else
    {

#line 1527
        _S1 = facet_1;

#line 1527
    }

#line 1527
    return _S1;
}


#line 2613
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_0, float tile_metres_0)
{
    float3 axis_0 = abs(normal_0);

    float _S2 = axis_0.x;

#line 2617
    float _S3 = axis_0.y;

#line 2617
    bool _S4;

#line 2617
    if(_S2 >= _S3)
    {

#line 2617
        _S4 = _S2 >= (axis_0.z);

#line 2617
    }
    else
    {

#line 2617
        _S4 = false;

#line 2617
    }

#line 2617
    float2 planar_0;

#line 2617
    if(_S4)
    {

#line 2617
        planar_0 = world_position_1.zy;

#line 2617
    }
    else
    {

        if(_S3 >= (axis_0.z))
        {

#line 2621
            planar_0 = world_position_1.xz;

#line 2621
        }
        else
        {

#line 2621
            planar_0 = world_position_1.xy;

#line 2621
        }

#line 2617
    }

#line 2629
    return planar_0 / float2(max(tile_metres_0, 0.00009999999747379f)) ;
}


#line 791
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1136
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1136
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint pad0_1;
    uint pad1_1;
    uint pad2_0;
};


#line 573
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
};


#line 1142
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_1;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 1142
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1142
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
};


#line 3332
struct GpuMaterial_natural_0
{
    packed_float4 base_color_0;
    uint base_color_texture_0;
    float metallic_0;
    float roughness_0;
    uint tiling_0;
    float tile_metres_1;
    float emissive_r_0;
    float emissive_g_0;
    float emissive_b_0;
};


#line 3332
struct GpuLight_natural_0
{
    packed_float4 position_1;
    packed_float4 color_1;
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
    MeshVertex_natural_0 device* vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
    GpuMaterial_natural_0 device* materials_0;
    texture2d_array<float, access::sample> base_color_textures_0;
    sampler base_color_sampler_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> specular_albedo_0;
    texture2d<float, access::sample> ambient_occlusion_0;
    GpuProbe_natural_0 device* probes_0;
};


#line 2423 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S5 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S6 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S7 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S8 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 2433
    uint _S9 = uint(pixel_0.x) / _S8;

#line 2433
    uint _S10 = min(_S9, _S5 - 1U);
    uint _S11 = uint(pixel_0.y) / _S8;

    float scale_0 = 24.0f / log2(10000.0f);

#line 2444
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S7 - 1U))) * _S6 + min(_S11, _S6 - 1U)) * _S5 + _S10;
}


#line 2388
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 2402
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 2409
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 1239
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 1246
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1253
    float _S12 = 1.0f - alpha2_0;

#line 1258
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S12 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S12 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 1566
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 1830
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 237
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 1912
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S13 = float2(0.5f, 0.5f) * texel_0 * grid_0;


    float2 _S14 = float2(1.0f, 1.0f);

#line 1919
    float2 _S15 = _S14 / texel_0;

#line 1919
    uint index_0 = 0U;

#line 1919
    float sum_0 = 0.0f;

#line 1919
    float found_0 = 0.0f;



    for(;;)
    {

#line 1923
        if(index_0 < 16U)
        {
        }
        else
        {

#line 1923
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_0] * float2(8.0f) ;
        float _S16 = spoke_0.x;

#line 1926
        float _S17 = rotation_0.x;

#line 1926
        float _S18 = spoke_0.y;

#line 1926
        float _S19 = rotation_0.y;

#line 1935
        int3 _S20 = int3(int2(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + float2(_S16 * _S17 - _S18 * _S19, _S16 * _S19 + _S18 * _S17) * texel_0 * grid_0, _S13, float2(1.0f)  - _S13)) * _S15, _S15 - _S14)), int(0));

#line 1935
        float depth_1 = ((kernelContext_1->shadow_atlas_0).read(vec<uint,2>(((_S20)).xy), uint(((_S20)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 1939
            sum_0 = sum_0 + depth_1;

#line 1939
            found_0 = found_1;

#line 1936
        }

#line 1923
        index_0 = index_0 + 1U;

#line 1923
    }

#line 1943
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 1954
    float _S21 = 2.0f * kernelContext_1->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_0 / found_0 - reference_0) * (_S21 + 40.0f) * 0.01999999955296516f / (_S21 / 768.0f), 2.0f, 8.0f);
}


#line 1854
float tile_pcf_0(uint tile_1, float2 tile_uv_2, float reference_1, float2 pixel_2, float radius_1, KernelContext_0 thread* kernelContext_2)
{
    float2 texel_1 = kernelContext_2->frame_0->shadow_params_0.xy;

#line 1861
    float2 grid_1 = float2(4.0f, 4.0f);
    float2 _S22 = float2(0.5f, 0.5f) * texel_1 * grid_1;

    float2 _S23 = shadow_rotation_0(pixel_2);

#line 1864
    uint index_1 = 0U;

#line 1864
    float visibility_0 = 0.0f;


    for(;;)
    {

#line 1867
        if(index_1 < 32U)
        {
        }
        else
        {

#line 1867
            break;
        }
        float2 spoke_1 = SHADOW_DISC_0[index_1] * float2(radius_1) ;

        float _S24 = spoke_1.x;

#line 1871
        float _S25 = _S23.x;

#line 1871
        float _S26 = spoke_1.y;

#line 1871
        float _S27 = _S23.y;



        float _S28 = ((kernelContext_2->shadow_atlas_0).sample_compare((kernelContext_2->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + float2(_S24 * _S25 - _S26 * _S27, _S24 * _S27 + _S26 * _S25) * texel_1 * grid_1, _S22, float2(1.0f)  - _S22))), (reference_1), level((0.0f))));

#line 1874
        float visibility_1 = visibility_0 + _S28;

#line 1867
        index_1 = index_1 + 1U;

#line 1867
        visibility_0 = visibility_1;

#line 1867
    }

#line 1877
    return visibility_0 / 32.0f;
}


#line 2008
float cascade_visibility_0(uint cascade_1, float3 world_position_2, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_3)
{

#line 2039
    float texel_world_0 = 2.0f * kernelContext_3->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 2046
    float4 clip_0 = (((float4(world_position_2 + geometric_normal_1 * float3((texel_world_0 * kernelContext_3->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_3->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 2050
    bool _S29;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 2051
        _S29 = true;

#line 2051
    }
    else
    {

#line 2051
        _S29 = (ndc_0.z) <= 0.0f;

#line 2051
    }

#line 2051
    if(_S29)
    {



        return 1.0f;
    }



    float2 tile_uv_3 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 2078
    float _S30 = ndc_0.z;

#line 2078
    float _S31 = sun_penumbra_texels_0(cascade_1, tile_uv_3, _S30, shadow_rotation_0(pixel_3), kernelContext_3);

#line 2078
    float _S32 = tile_pcf_0(cascade_1, tile_uv_3, _S30, pixel_3, _S31, kernelContext_3);
    return _S32;
}


#line 2095
float sun_visibility_0(float3 world_position_3, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_4)
{

#line 2096
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 2108
    float eye_distance_0 = length(world_position_3 - kernelContext_4->frame_0->camera_position_0.xyz);

#line 2108
    uint index_2 = 0U;

    for(;;)
    {

#line 2110
        if(index_2 < 2U)
        {
        }
        else
        {

#line 2110
            cascade_2 = 1U;

#line 2110
            break;
        }
        if(eye_distance_0 < kernelContext_4->frame_0->cascade_far_0[index_2])
        {

#line 2112
            cascade_2 = index_2;


            break;
        }

#line 2110
        index_2 = index_2 + 1U;

#line 2110
    }

#line 2110
    float _S33 = cascade_visibility_0(cascade_2, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_4);

#line 2121
    uint _S34 = cascade_2 + 1U;

#line 2121
    if(_S34 >= 2U)
    {



        return _S33;
    }

#line 2134
    float band_0 = kernelContext_4->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_4->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S33;
    }

#line 2138
    float _S35 = cascade_visibility_0(_S34, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_4);

#line 2149
    return mix(_S33, _S35, blend_0);
}


#line 2339
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S36 = axis_2.x;

#line 2342
    float _S37 = axis_2.y;

#line 2342
    bool _S38;

#line 2342
    if(_S36 >= _S37)
    {

#line 2342
        _S38 = _S36 >= (axis_2.z);

#line 2342
    }
    else
    {

#line 2342
        _S38 = false;

#line 2342
    }

#line 2342
    uint _S39;

#line 2342
    if(_S38)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 2344
            _S39 = 0U;

#line 2344
        }
        else
        {

#line 2344
            _S39 = 1U;

#line 2344
        }

#line 2344
        return _S39;
    }
    if(_S37 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 2348
            _S39 = 2U;

#line 2348
        }
        else
        {

#line 2348
            _S39 = 3U;

#line 2348
        }

#line 2348
        return _S39;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 2350
        _S39 = 4U;

#line 2350
    }
    else
    {

#line 2350
        _S39 = 5U;

#line 2350
    }

#line 2350
    return _S39;
}


#line 225
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 2252
float punctual_visibility_0(uint tile_3, float3 world_position_4, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_5)
{

#line 2264
    float4 clip_1 = (((float4(world_position_4 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_5->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 2271
    float _S40 = clip_1.w;

#line 2271
    if(_S40 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S40) ;

#line 2275
    bool _S41;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 2276
        _S41 = true;

#line 2276
    }
    else
    {

#line 2276
        _S41 = (ndc_1.z) <= 0.0f;

#line 2276
    }

#line 2276
    if(_S41)
    {

#line 2276
        _S41 = true;

#line 2276
    }
    else
    {

#line 2276
        _S41 = (ndc_1.z) > 1.0f;

#line 2276
    }

#line 2276
    if(_S41)
    {

#line 2283
        return 1.0f;
    }

#line 2283
    float _S42 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_5);

#line 2293
    return _S42;
}


#line 2358
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_5, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_6)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_5 - (float4(light_0->position_1) ).xyz;

#line 2366
    float _S43 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_5, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_6);

#line 2372
    return _S43;
}


#line 2300
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_6, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_7)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 2307
    float4 _S44 = float4(light_1->direction_0) ;

#line 2314
    float cos_outer_1 = _S44.w;

#line 2314
    float _S45 = punctual_visibility_0(tile_4, world_position_6, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_6 - (float4(light_1->position_1) ).xyz, normalize(_S44.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_7);

#line 2321
    return _S45;
}


#line 1283
float decode_specular_albedo_0(float2 texel_2)
{
    return (texel_2.x * 65280.0f + texel_2.y * 255.0f) / 65535.0f;
}


#line 1300
float specular_albedo_at_0(float n_dot_v_1, float roughness_1, KernelContext_0 thread* kernelContext_8)
{

#line 1300
    texture2d<float, access::sample> _S46 = kernelContext_8->specular_albedo_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S46).get_width(0)),(*((&height_0)) = (_S46).get_height(0));
    float2 extent_1 = float2(float(width_0), float(height_0));
    float2 scaled_0 = float2(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1306
    float2 _S47 = float2(1.0f) ;
    float2 _S48 = extent_1 - _S47;

#line 1307
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S48);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S49 = int2(low_0);
    int2 _S50 = int2(min(low_0 + _S47, _S48));
    int _S51 = _S49.x;

#line 1313
    int _S52 = _S49.y;

#line 1313
    int3 _S53 = int3(_S51, _S52, int(0));
    int _S54 = _S50.x;

#line 1314
    int3 _S55 = int3(_S54, _S52, int(0));
    float _S56 = weight_0.x;
    int _S57 = _S50.y;

#line 1316
    int3 _S58 = int3(_S51, _S57, int(0));
    int3 _S59 = int3(_S54, _S57, int(0));

    return mix(mix(decode_specular_albedo_0(((kernelContext_8->specular_albedo_0).read(vec<uint,2>(((_S53)).xy), uint(((_S53)).z)).xy)), decode_specular_albedo_0(((kernelContext_8->specular_albedo_0).read(vec<uint,2>(((_S55)).xy), uint(((_S55)).z)).xy)), _S56), mix(decode_specular_albedo_0(((kernelContext_8->specular_albedo_0).read(vec<uint,2>(((_S58)).xy), uint(((_S58)).z)).xy)), decode_specular_albedo_0(((kernelContext_8->specular_albedo_0).read(vec<uint,2>(((_S59)).xy), uint(((_S59)).z)).xy)), _S56), weight_0.y);
}


#line 1338
float3 specular_compensation_0(float3 f0_1, float n_dot_v_2, float roughness_2, KernelContext_0 thread* kernelContext_9)
{

#line 1338
    float _S60 = specular_albedo_at_0(n_dot_v_2, roughness_2, kernelContext_9);



    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(_S60, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 2552
float3 sky_irradiance_0(float3 normal_2, KernelContext_0 thread* kernelContext_10)
{
    float4 basis_1 = float4(normal_2, 1.0f);
    return max(float3(dot(kernelContext_10->frame_0->sky_sh_r_0, basis_1), dot(kernelContext_10->frame_0->sky_sh_g_0, basis_1), dot(kernelContext_10->frame_0->sky_sh_b_0, basis_1)), float3(0.0f, 0.0f, 0.0f));
}


#line 702
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 2454
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_11)
{

    GpuProbe_natural_0 _S61 = kernelContext_11->probes_0[min((cell_1.z * kernelContext_11->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_11->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_11->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 2457
    GpuProbe_0 _S62 = { float4(_S61.sh_r_0) , float4(_S61.sh_g_0) , float4(_S61.sh_b_0)  };

#line 2457
    return _S62;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_0)
{
    thread GpuProbe_0 blended_0;
    float4 _S63 = float4(t_0) ;

#line 2465
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S63);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S63);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S63);
    return blended_0;
}


#line 2505
float3 probe_irradiance_0(float3 world_position_7, float3 normal_3, KernelContext_0 thread* kernelContext_12)
{

#line 2505
    float3 _S64 = float3(1.0f) ;

#line 2510
    float3 _S65 = float3(0.0f, 0.0f, 0.0f);

#line 2510
    float3 last_0 = max(float3(kernelContext_12->frame_0->probe_counts_0.xyz) - _S64, _S65);
    float3 grid_2 = clamp((world_position_7 - kernelContext_12->frame_0->probe_origin_0.xyz) * kernelContext_12->frame_0->probe_inv_spacing_0.xyz, _S65, last_0);

    float3 base_2 = floor(grid_2);
    float3 f_0 = grid_2 - base_2;

    uint3 _S66 = uint3(base_2);



    uint3 _S67 = uint3(min(base_2 + _S64, last_0));

#line 2527
    uint _S68 = _S66.x;

#line 2527
    uint _S69 = _S66.y;

#line 2527
    uint _S70 = _S66.z;

#line 2527
    GpuProbe_0 _S71 = probe_at_0(uint3(_S68, _S69, _S70), kernelContext_12);

#line 2527
    uint _S72 = _S67.x;

#line 2527
    GpuProbe_0 _S73 = probe_at_0(uint3(_S72, _S69, _S70), kernelContext_12);

#line 2527
    float _S74 = f_0.x;

#line 2527
    thread GpuProbe_0 _S75 = _S71;

#line 2527
    thread GpuProbe_0 _S76 = _S73;

#line 2527
    GpuProbe_0 _S77 = lerp_probe_0(&_S75, &_S76, _S74);
    uint _S78 = _S67.y;

#line 2528
    GpuProbe_0 _S79 = probe_at_0(uint3(_S68, _S78, _S70), kernelContext_12);

#line 2528
    GpuProbe_0 _S80 = probe_at_0(uint3(_S72, _S78, _S70), kernelContext_12);

#line 2528
    thread GpuProbe_0 _S81 = _S79;

#line 2528
    thread GpuProbe_0 _S82 = _S80;

#line 2528
    GpuProbe_0 _S83 = lerp_probe_0(&_S81, &_S82, _S74);
    uint _S84 = _S67.z;

#line 2529
    GpuProbe_0 _S85 = probe_at_0(uint3(_S68, _S69, _S84), kernelContext_12);

#line 2529
    GpuProbe_0 _S86 = probe_at_0(uint3(_S72, _S69, _S84), kernelContext_12);

#line 2529
    thread GpuProbe_0 _S87 = _S85;

#line 2529
    thread GpuProbe_0 _S88 = _S86;

#line 2529
    GpuProbe_0 _S89 = lerp_probe_0(&_S87, &_S88, _S74);

#line 2529
    GpuProbe_0 _S90 = probe_at_0(uint3(_S68, _S78, _S84), kernelContext_12);

#line 2529
    GpuProbe_0 _S91 = probe_at_0(uint3(_S72, _S78, _S84), kernelContext_12);

#line 2529
    thread GpuProbe_0 _S92 = _S90;

#line 2529
    thread GpuProbe_0 _S93 = _S91;

#line 2529
    GpuProbe_0 _S94 = lerp_probe_0(&_S92, &_S93, _S74);

    float _S95 = f_0.y;

#line 2531
    thread GpuProbe_0 _S96 = _S77;

#line 2531
    thread GpuProbe_0 _S97 = _S83;

#line 2531
    GpuProbe_0 _S98 = lerp_probe_0(&_S96, &_S97, _S95);

#line 2531
    thread GpuProbe_0 _S99 = _S89;

#line 2531
    thread GpuProbe_0 _S100 = _S94;

#line 2531
    GpuProbe_0 _S101 = lerp_probe_0(&_S99, &_S100, _S95);

    float _S102 = f_0.z;

#line 2533
    thread GpuProbe_0 _S103 = _S98;

#line 2533
    thread GpuProbe_0 _S104 = _S101;

#line 2533
    GpuProbe_0 _S105 = lerp_probe_0(&_S103, &_S104, _S102);

    float4 basis_2 = float4(normal_3, 1.0f);
    return max(float3(dot(_S105.sh_r_0, basis_2), dot(_S105.sh_g_0, basis_2), dot(_S105.sh_b_0, basis_2)), _S65);
}


#line 675
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_1)
{
    return float3(material_1->emissive_r_0, material_1->emissive_g_0, material_1->emissive_b_0);
}


#line 1400
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S106 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 1408
    float kernel_0 = 0.0001984127011383f;

#line 1408
    int term_0 = int(6);

    for(;;)
    {

#line 1410
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 1410
            break;
        }
        float _S107 = kernel_0 * _S106 + FOG_KERNEL_0[term_0];

#line 1410
        int term_1 = term_0 - int(1);

#line 1410
        kernel_0 = _S107;

#line 1410
        term_0 = term_1;

#line 1410
    }

#line 1417
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 1427
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S108 = - d_0;

#line 1431
        float series_0 = 0.00833333376795053f;

#line 1431
        int term_2 = int(3);

        for(;;)
        {

#line 1433
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 1433
                break;
            }
            float _S109 = series_0 * _S108 + FOG_RATIO_KERNEL_0[term_2];

#line 1433
            int term_3 = term_2 - int(1);

#line 1433
            series_0 = _S109;

#line 1433
            term_2 = term_3;

#line 1433
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 1461
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_1)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_1, 0.0f, 32.0f);
    }

#line 1472
    return clamp(density_0 * distance_1 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 1480
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 2578
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
};


#line 2578
struct pixelInput_0
{
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_2 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 2633
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S110 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_1 [[texture(3)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 2633
    thread KernelContext_0 kernelContext_13;

#line 2633
    (&kernelContext_13)->draw_0 = draw_1;

#line 2633
    (&kernelContext_13)->visible_instances_0 = visible_instances_1;

#line 2633
    (&kernelContext_13)->instances_0 = instances_1;

#line 2633
    (&kernelContext_13)->meshes_0 = meshes_1;

#line 2633
    (&kernelContext_13)->vertices_0 = vertices_1;

#line 2633
    (&kernelContext_13)->frame_0 = frame_1;

#line 2633
    (&kernelContext_13)->materials_0 = materials_1;

#line 2633
    (&kernelContext_13)->base_color_textures_0 = base_color_textures_1;

#line 2633
    (&kernelContext_13)->base_color_sampler_0 = base_color_sampler_1;

#line 2633
    (&kernelContext_13)->cluster_lights_0 = cluster_lights_1;

#line 2633
    (&kernelContext_13)->lights_0 = lights_1;

#line 2633
    (&kernelContext_13)->shadow_atlas_0 = shadow_atlas_1;

#line 2633
    (&kernelContext_13)->shadow_sampler_0 = shadow_sampler_1;

#line 2633
    (&kernelContext_13)->specular_albedo_0 = specular_albedo_1;

#line 2633
    (&kernelContext_13)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2633
    (&kernelContext_13)->probes_0 = probes_1;

#line 2639
    float3 normal_4 = normalize(_S110.world_normal_0);

#line 2657
    if((frame_1->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S110.color_2.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return tint_0;
    }

    if((frame_1->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 2670
        float3 _S111 = float3(0.5f) ;

#line 2677
        (&normals_0)->lit_0 = float4(normal_4 * _S111 + _S111, 1.0f);

#line 2683
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_13)->frame_0->camera_position_0.xyz - _S110.world_position_8);



    float3 _S112 = geometric_normal_of_0(_S110.world_position_8, normal_4);

#line 2691
    thread GpuMaterial_natural_0 _S113 = (&kernelContext_13)->materials_0[_S110.material_2];

#line 2691
    float2 uv_2;

#line 2710
    if(((&_S113)->tiling_0) == 1U)
    {

#line 2710
        uv_2 = physical_tile_uv_0(_S110.world_position_8, normal_4, (&_S113)->tile_metres_1);

#line 2710
    }
    else
    {

#line 2710
        uv_2 = _S110.uv_1;

#line 2710
    }

#line 2715
    float3 _S114 = float3(uv_2, float((&_S113)->base_color_texture_0));
    float4 albedo_0 = _S110.color_2 * float4((&_S113)->base_color_0)  * (((&kernelContext_13)->base_color_textures_0).sample(((&kernelContext_13)->base_color_sampler_0), ((_S114)).xy, uint(((_S114)).z)));

#line 2722
    float metallic_1 = saturate((&_S113)->metallic_0);
    float roughness_3 = clamp((&_S113)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_3 * roughness_3;
    float _S115 = alpha_0 * alpha_0;

#line 2731
    float3 _S116 = albedo_0.xyz;

#line 2731
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S116, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S116 * float3((1.0f - metallic_1)) ;

#line 2738
    float _S117 = max(dot(normal_4, to_eye_0), 0.00009999999747379f);

#line 2748
    float2 _S118 = position_2.xy;

#line 2748
    uint _S119 = froxel_of_0(_S118, (((float4(_S110.world_position_8, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_13);

#line 2748
    uint base_3 = _S119 * 17U;

#line 2753
    uint _S120 = min((&kernelContext_13)->cluster_lights_0[base_3], 16U);

#line 2759
    float3 _S121 = float3(0.0f, 0.0f, 0.0f);

#line 2759
    uint slot_0 = 0U;

#line 2759
    float3 direct_0 = _S121;

#line 2759
    float3 gloss_0 = _S121;

    for(;;)
    {

#line 2761
        if(slot_0 < _S120)
        {
        }
        else
        {

#line 2761
            break;
        }

#line 2761
        thread GpuLight_natural_0 _S122 = (&kernelContext_13)->lights_0[(&kernelContext_13)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 2761
        uint _S123 = (&_S122)->kind_0;

#line 2770
        bool _S124 = ((&_S122)->kind_0) == 0U;

#line 2770
        float3 to_light_7;

#line 2770
        float reach_0;

#line 2770
        if(_S124)
        {

#line 2770
            to_light_7 = normalize((float4((&_S122)->direction_0) ).xyz);

#line 2770
            reach_0 = 1.0f;

#line 2770
        }
        else
        {

#line 2770
            float4 _S125 = float4((&_S122)->position_1) ;

#line 2777
            float3 offset_0 = _S125.xyz - _S110.world_position_8;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S125.w);
            if(_S123 == 2U)
            {

#line 2781
                float4 _S126 = float4((&_S122)->direction_0) ;

#line 2781
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S126.xyz, _S126.w, (&_S122)->cos_inner_0);

#line 2781
            }
            else
            {

#line 2781
                reach_0 = reach_1;

#line 2781
            }

#line 2781
            to_light_7 = to_light_8;

#line 2770
        }

#line 2788
        float n_dot_l_5 = dot(normal_4, to_light_7);
        float _S127 = max(n_dot_l_5, 0.0f);

#line 2795
        float3 half_vector_0 = normalize(to_light_7 + to_eye_0);

#line 2802
        float3 specular_0 = ggx_lobe_0(_S115, f0_2, _S127, _S117, max(dot(normal_4, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S127) ;

#line 2802
        float reach_2;

#line 2817
        if(_S124)
        {

#line 2817
            float _S128 = sun_visibility_0(_S110.world_position_8, to_light_7, n_dot_l_5, _S112, _S118, &kernelContext_13);

#line 2817
            reach_2 = _S128;

#line 2817
        }
        else
        {


            if(_S123 == 1U)
            {

#line 2822
                uint _S129 = (&_S122)->shadow_tile_0;

#line 2834
                if(((&_S122)->shadow_tile_0) <= 8U)
                {

#line 2834
                    float _S130 = point_visibility_0(&_S122, _S129, _S110.world_position_8, to_light_7, n_dot_l_5, _S112, _S118, &kernelContext_13);

#line 2834
                    reach_2 = reach_0 * _S130;

#line 2834
                }
                else
                {

#line 2834
                    reach_2 = reach_0;

#line 2834
                }

#line 2822
            }
            else
            {

#line 2822
                uint _S131 = (&_S122)->shadow_tile_0;

#line 2840
                if(((&_S122)->shadow_tile_0) < 14U)
                {

#line 2840
                    float _S132 = spot_visibility_0(&_S122, _S131, _S110.world_position_8, to_light_7, n_dot_l_5, _S112, _S118, &kernelContext_13);

#line 2840
                    reach_2 = reach_0 * _S132;

#line 2840
                }
                else
                {

#line 2840
                    reach_2 = reach_0;

#line 2840
                }

#line 2822
            }

#line 2817
        }

#line 2848
        float3 _S133 = (float4((&_S122)->color_1) ).xyz;

#line 2848
        float3 direct_1 = direct_0 + _S133 * float3((_S127 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S133 * (specular_0 * float3(reach_2) );

#line 2761
        slot_0 = slot_0 + 1U;

#line 2761
        direct_0 = direct_1;

#line 2761
        gloss_0 = gloss_1;

#line 2761
    }

#line 2761
    float3 _S134 = specular_compensation_0(f0_2, _S117, roughness_3, &kernelContext_13);

#line 2863
    float3 gloss_2 = gloss_0 * _S134;

#line 2863
    texture2d<float, access::sample> _S135 = (&kernelContext_13)->ambient_occlusion_0;

#line 2881
    thread uint occlusion_width_0;
    thread uint occlusion_height_0;
    (*((&occlusion_width_0)) = (_S135).get_width(0)),(*((&occlusion_height_0)) = (_S135).get_height(0));


    int3 _S136 = int3(min(int2(_S118), int2(int(occlusion_width_0), int(occlusion_height_0)) - int2(int(1)) ), int(0));

#line 2886
    float occluded_0 = (((&kernelContext_13)->ambient_occlusion_0).read(vec<uint,2>(((_S136)).xy), uint(((_S136)).z)).x);

#line 2904
    float3 _S137 = frame_1->ambient_0.xyz;

#line 2904
    float3 _S138 = sky_irradiance_0(normal_4, &kernelContext_13);

#line 2904
    float3 _S139 = _S137 + _S138;

#line 2904
    float3 _S140 = probe_irradiance_0(_S110.world_position_8, normal_4, &kernelContext_13);

#line 2925
    float3 lit_1 = diffuse_albedo_0 * ((_S139 + _S140) * float3(occluded_0)  + direct_0) + gloss_2;

#line 2925
    float3 _S141 = emissive_of_0(&_S113);

#line 2961
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_13)->frame_0->fog_params_0.x, (&kernelContext_13)->frame_0->fog_params_0.y, (&kernelContext_13)->frame_0->camera_position_0.y - (&kernelContext_13)->frame_0->fog_params_0.z, _S110.world_position_8.y - (&kernelContext_13)->frame_0->fog_params_0.z, length((&kernelContext_13)->frame_0->camera_position_0.xyz - _S110.world_position_8)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S141) * float3(fog_survives_0)  + (&kernelContext_13)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);

#line 2973
    (&output_0)->reflectivity_0 = float4(f0_2, saturate(1.0f - roughness_3 / 0.5f));
    return output_0;
}


#line 2974
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_9 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
};


#line 1096
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_10;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_4;
};


#line 1096
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_2 [[texture(3)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1096
    thread KernelContext_0 kernelContext_14;

#line 1096
    (&kernelContext_14)->draw_0 = draw_2;

#line 1096
    (&kernelContext_14)->visible_instances_0 = visible_instances_2;

#line 1096
    (&kernelContext_14)->instances_0 = instances_2;

#line 1096
    (&kernelContext_14)->meshes_0 = meshes_2;

#line 1096
    (&kernelContext_14)->vertices_0 = vertices_2;

#line 1096
    (&kernelContext_14)->frame_0 = frame_2;

#line 1096
    (&kernelContext_14)->materials_0 = materials_2;

#line 1096
    (&kernelContext_14)->base_color_textures_0 = base_color_textures_2;

#line 1096
    (&kernelContext_14)->base_color_sampler_0 = base_color_sampler_2;

#line 1096
    (&kernelContext_14)->cluster_lights_0 = cluster_lights_2;

#line 1096
    (&kernelContext_14)->lights_0 = lights_2;

#line 1096
    (&kernelContext_14)->shadow_atlas_0 = shadow_atlas_2;

#line 1096
    (&kernelContext_14)->shadow_sampler_0 = shadow_sampler_2;

#line 1096
    (&kernelContext_14)->specular_albedo_0 = specular_albedo_2;

#line 1096
    (&kernelContext_14)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1096
    (&kernelContext_14)->probes_0 = probes_2;

#line 1096
    GpuInstance_natural_0 device* _S142 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1142
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1142
    uint base_vertex_2;

#line 1151
    if(((_S142->flags_0) & 2U) != 0U)
    {

#line 1151
        base_vertex_2 = _S142->base_vertex_0;

#line 1151
    }
    else
    {

#line 1151
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1151
    }

    MeshVertex_natural_0 vertex_0 = (&kernelContext_14)->vertices_0[index_3 + base_vertex_2];

#line 1153
    matrix<float,int(4),int(4)>  _S143 = matrix<float,int(4),int(4)> (_S142->transform_0.data_0[int(0)][int(0)], _S142->transform_0.data_0[int(1)][int(0)], _S142->transform_0.data_0[int(2)][int(0)], _S142->transform_0.data_0[int(3)][int(0)], _S142->transform_0.data_0[int(0)][int(1)], _S142->transform_0.data_0[int(1)][int(1)], _S142->transform_0.data_0[int(2)][int(1)], _S142->transform_0.data_0[int(3)][int(1)], _S142->transform_0.data_0[int(0)][int(2)], _S142->transform_0.data_0[int(1)][int(2)], _S142->transform_0.data_0[int(2)][int(2)], _S142->transform_0.data_0[int(3)][int(2)], _S142->transform_0.data_0[int(0)][int(3)], _S142->transform_0.data_0[int(1)][int(3)], _S142->transform_0.data_0[int(2)][int(3)], _S142->transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S143)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_14)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_14)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_10 = world_0.xyz;

#line 1165
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_1) ).xyz) * (normal_basis_0(matrix<float,int(3),int(3)> (_S143[int(0)].xyz, _S143[int(1)].xyz, _S143[int(2)].xyz)))));

#line 1165
    float4 _S144;

#line 1172
    if(((&kernelContext_14)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1172
        _S144 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1172
    }
    else
    {

#line 1172
        _S144 = float4(vertex_0.color_0) ;

#line 1172
    }

#line 1171
    (&output_1)->color_4 = _S144;

#line 1178
    (&output_1)->material_4 = _S142->material_0;
    (&output_1)->uv_4 = (float4(vertex_0.uv_0) ).xy;
    VertexOutput_0 _S145 = output_1;

#line 1180
    thread vertexMain_Result_0 _S146;

#line 1180
    (&_S146)->position_3 = _S145.position_4;

#line 1180
    (&_S146)->world_position_9 = _S145.world_position_10;

#line 1180
    (&_S146)->world_normal_1 = _S145.world_normal_2;

#line 1180
    (&_S146)->color_3 = _S145.color_4;

#line 1180
    (&_S146)->material_3 = _S145.material_4;

#line 1180
    (&_S146)->uv_3 = _S145.uv_4;

#line 1180
    return _S146;
}

