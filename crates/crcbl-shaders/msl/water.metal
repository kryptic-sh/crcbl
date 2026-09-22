#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 260 "shaders/water.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 313
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 276
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 295
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 306
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 257
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1203
float2 water_ndc_0(int2 pixel_0, float2 extent_0)
{
    return float2((float(pixel_0.x) + 0.5f) / extent_0.x * 2.0f - 1.0f, 1.0f - (float(pixel_0.y) + 0.5f) / extent_0.y * 2.0f);
}


#line 90 "core"
struct WaterVertex_natural_0
{
    packed_float4 position_0;
    packed_uint4 body_0;
};


#line 90
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 72 "shaders/water.slang"
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
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
    _MatrixStorage_float4x4_ColMajornatural_0 previous_view_proj_0;
    uint4 vertex_pool_0;
    array<float4, int(16)> shadow_atlas_rect_0;
    uint4 shadow_filter_0;
};


#line 72
struct WaterMedium_natural_0
{
    packed_float4 absorption_0;
    packed_float4 scattering_0;
};


#line 72
struct VolumetricParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inverse_view_proj_0;
    float4 eye_0;
    float4 depth_row_0;
    float4 fog_params_1;
    float4 fog_color_1;
    float4 sun_direction_0;
    float4 sun_radiance_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_1;
    float4 cascade_far_1;
    float4 shadow_params_1;
    uint grid_x_0;
    uint grid_y_0;
    uint slices_0;
    uint tile_pixels_0;
    uint viewport_x_0;
    uint viewport_y_0;
    uint froxel_count_0;
    uint pad0_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_1;
    array<float4, int(16)> shadow_atlas_rect_1;
};


#line 153
struct WaterParams_0
{
    float4 sun_direction_1;
    float4 sun_color_0;
    uint4 froxels_0;
};


#line 109
struct SsrParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    uint4 probe_counts_1;
    uint4 probe_levels_1;
    array<float4, int(4)> probe_level_origin_1;
    array<float4, int(4)> probe_level_inv_spacing_1;
    array<uint4, int(4)> probe_level_offset_1;
    uint4 hiz_0;
    array<float4, int(3)> sky_0;
    float4 atmosphere_0;
};


#line 109
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 109
struct KernelContext_0
{
    WaterVertex_natural_0 device* water_vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
    depth2d<float, access::sample> scene_depth_0;
    WaterMedium_natural_0 device* media_0;
    texture2d<float, access::sample> scene_color_0;
    VolumetricParams_natural_0 constant* params_0;
    WaterParams_0 constant* water_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    SsrParams_natural_0 constant* camera_0;
    GpuProbe_natural_0 device* probes_0;
    texture2d_array<float, access::sample> probe_visibility_0;
    texture2d<float, access::sample> sky_prefilter_0;
    packed_float4 device* sky_view_0;
    packed_float4 device* volumetrics_0;
    packed_float4 device* lighting_0;
};


#line 403
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 403
float3 volumetric_unproject_1(float2 ndc_1, float depth_1, KernelContext_0 thread* kernelContext_1)
{
    float4 world_1 = (((float4(ndc_1, depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_1.xyz / float3(world_1.w) ;
}


#line 1212
float3 water_opaque_point_0(int2 pixel_1, float depth_2, float2 extent_1, float3 surface_0, float3 direction_0, KernelContext_0 thread* kernelContext_2)
{

    if(depth_2 <= 0.0f)
    {
        return surface_0 + direction_0 * float3(1000.0f) ;
    }

#line 1217
    float3 _S1 = volumetric_unproject_1(water_ndc_0(pixel_1, extent_1), depth_2, kernelContext_2);

    return _S1;
}


#line 1193
float3 water_refract_0(float3 incident_0, float3 normal_0, float eta_0)
{
    float cos_incident_0 = - dot(incident_0, normal_0);

    return incident_0 * float3(eta_0)  + normal_0 * float3((eta_0 * cos_incident_0 - sqrt(max(1.0f - eta_0 * eta_0 * (1.0f - cos_incident_0 * cos_incident_0), 0.0f)))) ;
}


#line 349
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S2 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 357
    float kernel_0 = 0.0001984127011383f;

#line 357
    int term_0 = int(6);

    for(;;)
    {

#line 359
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 359
            break;
        }
        float _S3 = kernel_0 * _S2 + FOG_KERNEL_0[term_0];

#line 359
        int term_1 = term_0 - int(1);

#line 359
        kernel_0 = _S3;

#line 359
        term_0 = term_1;

#line 359
    }

#line 366
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 456
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_3)
{
    return kernelContext_3->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 456
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_4)
{
    return kernelContext_4->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 471
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 466
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_5)
{
    return rect_1.x / kernelContext_5->frame_0->shadow_params_0.x;
}


#line 433
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_0)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_0));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 445
uint shadow_filter_mode_0(float2 pixel_2, KernelContext_0 thread* kernelContext_6)
{

#line 445
    uint _S4;

    if(uint(pixel_2.x) < (kernelContext_6->frame_0->shadow_filter_0.z))
    {

#line 447
        _S4 = kernelContext_6->frame_0->shadow_filter_0.x;

#line 447
    }
    else
    {

#line 447
        _S4 = kernelContext_6->frame_0->shadow_filter_0.y;

#line 447
    }

#line 447
    return _S4;
}


#line 461
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_7)
{
    return kernelContext_7->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 461
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 451
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 476
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_9)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S5 = spoke_0.x;

#line 481
    float _S6 = rotation_0.x;

#line 481
    float _S7 = spoke_0.y;

#line 481
    float _S8 = rotation_0.y;


    float _S9 = ((kernelContext_9->shadow_atlas_0).sample_compare((kernelContext_9->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S5 * _S6 - _S7 * _S8, _S5 * _S8 + _S7 * _S6) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 484
    return _S9;
}


#line 525
float tile_box_pcf_0(uint tile_2, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_10)
{

#line 525
    float4 _S10 = atlas_rect_1(tile_2, kernelContext_10);


    if(atlas_rect_is_empty_0(_S10))
    {
        return 1.0f;
    }

#line 530
    float2 _S11 = atlas_step_1(_S10, kernelContext_10);

#line 530
    int y_0 = int(-1);

#line 530
    float visibility_0 = 0.0f;

#line 535
    for(;;)
    {

#line 535
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 535
            break;
        }

#line 535
        int x_1 = int(-1);

        for(;;)
        {

#line 537
            if(x_1 <= int(1))
            {
            }
            else
            {

#line 537
                break;
            }

#line 537
            float _S12 = tile_tap_0(_S10, _S11, tile_uv_2, float2(float(x_1), float(y_0)), float2(1.0f, 0.0f), reference_1, kernelContext_10);

            float visibility_1 = visibility_0 + _S12;

#line 537
            x_1 = x_1 + int(1);

#line 537
            visibility_0 = visibility_1;

#line 537
        }

#line 535
        y_0 = y_0 + int(1);

#line 535
    }

#line 543
    return visibility_0 / 9.0f;
}


#line 439
float2 shadow_rotation_0(float2 pixel_3)
{
    uint2 cell_0 = uint2(pixel_3) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 487
float tile_pcf_0(uint tile_3, float2 tile_uv_3, float reference_2, float2 pixel_4, float radius_0, KernelContext_0 thread* kernelContext_11)
{
    float2 _S13 = shadow_rotation_0(pixel_4);

#line 489
    float4 _S14 = atlas_rect_1(tile_3, kernelContext_11);

    if(atlas_rect_is_empty_0(_S14))
    {
        return 1.0f;
    }

#line 493
    float2 _S15 = atlas_step_1(_S14, kernelContext_11);

#line 493
    uint spot_0 = 0U;

#line 493
    float probe_0 = 0.0f;

#line 498
    for(;;)
    {

#line 498
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 498
            break;
        }

#line 498
        float _S16 = tile_tap_0(_S14, _S15, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_0) , _S13, reference_2, kernelContext_11);

        float probe_1 = probe_0 + _S16;

#line 498
        spot_0 = spot_0 + 1U;

#line 498
        probe_0 = probe_1;

#line 498
    }

#line 507
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 513
    uint index_0 = 0U;

#line 513
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 517
        if(index_0 < 32U)
        {
        }
        else
        {

#line 517
            break;
        }

#line 517
        float _S17 = tile_tap_0(_S14, _S15, tile_uv_3, SHADOW_DISC_0[index_0] * float2(radius_0) , _S13, reference_2, kernelContext_11);

        float visibility_3 = visibility_2 + _S17;

#line 517
        index_0 = index_0 + 1U;

#line 517
        visibility_2 = visibility_3;

#line 517
    }

#line 522
    return visibility_2 / 32.0f;
}


#line 546
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_12)
{
    float2 texel_0 = kernelContext_12->frame_0->shadow_params_0.xy;

#line 548
    float4 _S18 = atlas_rect_0(cascade_0, kernelContext_12);

#line 548
    float2 _S19 = atlas_step_0(_S18, kernelContext_12);


    float2 _S20 = float2(0.5f, 0.5f) * _S19;


    float2 _S21 = float2(1.0f, 1.0f);

#line 554
    float2 _S22 = _S21 / texel_0;

#line 554
    uint index_1 = 0U;

#line 554
    float sum_0 = 0.0f;

#line 554
    float found_0 = 0.0f;



    for(;;)
    {

#line 558
        if(index_1 < 16U)
        {
        }
        else
        {

#line 558
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_1] * float2(8.0f) ;
        float _S23 = spoke_1.x;

#line 561
        float _S24 = rotation_1.x;

#line 561
        float _S25 = spoke_1.y;

#line 561
        float _S26 = rotation_1.y;

#line 569
        int3 _S27 = int3(int2(min(atlas_uv_0(_S18, clamp(tile_uv_4 + float2(_S23 * _S24 - _S25 * _S26, _S23 * _S26 + _S25 * _S24) * _S19, _S20, float2(1.0f)  - _S20)) * _S22, _S22 - _S21)), int(0));

#line 569
        float depth_3 = ((kernelContext_12->shadow_atlas_0).read(vec<uint,2>(((_S27)).xy), uint(((_S27)).z)));
        if(depth_3 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 573
            sum_0 = sum_0 + depth_3;

#line 573
            found_0 = found_1;

#line 570
        }

#line 558
        index_1 = index_1 + 1U;

#line 558
    }

#line 577
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 588
    float _S28 = 2.0f * kernelContext_12->frame_0->cascade_far_0[cascade_0];

#line 588
    float separation_0 = (sum_0 / found_0 - reference_3) * (_S28 + 40.0f);

#line 588
    float _S29 = tile_texels_0(_S18, kernelContext_12);

    return clamp(separation_0 * 0.01999999955296516f / (_S28 / _S29), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_0, float3 to_light_1, float3 geometric_normal_1, float2 pixel_5, KernelContext_0 thread* kernelContext_13)
{

#line 596
    float4 _S30 = atlas_rect_0(cascade_1, kernelContext_13);

#line 630
    if(atlas_rect_is_empty_0(_S30))
    {


        return 1.0f;
    }
    float _S31 = 2.0f * kernelContext_13->frame_0->cascade_far_0[cascade_1];

#line 636
    float _S32 = tile_texels_0(_S30, kernelContext_13);

#line 636
    float texel_world_0 = _S31 / _S32;

#line 643
    float4 clip_0 = (((float4(world_position_0 + geometric_normal_1 * float3((texel_world_0 * kernelContext_13->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_1)))  + to_light_1 * float3((texel_world_0 * kernelContext_13->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_13->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_2 = clip_0.xyz / float3(clip_0.w) ;

#line 647
    bool _S33;
    if(any((abs(ndc_2.xy)) > (float2(1.0f) )))
    {

#line 648
        _S33 = true;

#line 648
    }
    else
    {

#line 648
        _S33 = (ndc_2.z) <= 0.0f;

#line 648
    }

#line 648
    if(_S33)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_2.x * 0.5f + 0.5f, 0.5f - ndc_2.y * 0.5f);

#line 658
    uint _S34 = shadow_filter_mode_0(pixel_5, kernelContext_13);

#line 675
    if(_S34 == 2U)
    {

#line 675
        float _S35 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_2.z, kernelContext_13);

        return _S35;
    }
    if(_S34 == 1U)
    {

#line 679
        float _S36 = tile_pcf_0(cascade_1, tile_uv_5, ndc_2.z, pixel_5, 2.0f, kernelContext_13);



        return _S36;
    }

    float _S37 = ndc_2.z;

#line 686
    float _S38 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S37, shadow_rotation_0(pixel_5), kernelContext_13);

#line 686
    float _S39 = tile_pcf_0(cascade_1, tile_uv_5, _S37, pixel_5, _S38, kernelContext_13);
    return _S39;
}

float sun_visibility_0(float3 world_position_1, float3 to_light_2, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_6, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_14)
{
    uint cascade_2;

#line 692
    bool covered_0;

#line 701
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 713
    float eye_distance_0 = length(world_position_1 - kernelContext_14->frame_0->camera_position_0.xyz);

#line 713
    uint index_2 = 0U;

#line 721
    for(;;)
    {

#line 721
        if(index_2 < 2U)
        {
        }
        else
        {

#line 721
            covered_0 = false;

#line 721
            cascade_2 = 1U;

#line 721
            break;
        }
        if(eye_distance_0 < kernelContext_14->frame_0->cascade_far_0[index_2])
        {

#line 723
            covered_0 = true;

#line 723
            cascade_2 = index_2;



            break;
        }

#line 721
        index_2 = index_2 + 1U;

#line 721
    }

#line 730
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 730
    }

#line 730
    float _S40 = cascade_visibility_0(cascade_2, world_position_1, to_light_2, geometric_normal_2, pixel_6, kernelContext_14);

#line 737
    uint _S41 = cascade_2 + 1U;

#line 737
    if(_S41 >= 2U)
    {



        return _S40;
    }

#line 750
    float band_0 = kernelContext_14->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_14->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S40;
    }

#line 758
    float _S42 = cascade_visibility_0(_S41, world_position_1, to_light_2, geometric_normal_2, pixel_6, kernelContext_14);

#line 769
    return mix(_S40, _S42, blend_0);
}


#line 424
float3 sky_irradiance_0(float3 normal_1, KernelContext_0 thread* kernelContext_15)
{
    float4 basis_0 = float4(normal_1, 1.0f);
    return max(float3(dot(kernelContext_15->frame_0->sky_sh_r_0, basis_0), dot(kernelContext_15->frame_0->sky_sh_g_0, basis_0), dot(kernelContext_15->frame_0->sky_sh_b_0, basis_0)), float3(0.0f, 0.0f, 0.0f));
}


#line 976
float probe_level_reach_0(float3 world_position_2, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 976
    float reach_0 = 0.0f;

#line 976
    uint axis_0 = 0U;


    for(;;)
    {

#line 979
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 979
            break;
        }

#line 979
        uint _S43 = axis_0;

#line 979
        bool _S44;

        if((last_0[axis_0]) == 0.0f)
        {

#line 981
            _S44 = true;

#line 981
        }
        else
        {

#line 981
            _S44 = (inv_spacing_0[axis_0]) == 0.0f;

#line 981
        }

#line 981
        if(_S44)
        {

#line 982
            axis_0 = axis_0 + 1U;

#line 979
            continue;
        }

#line 979
        reach_0 = max(reach_0, abs(2.0f * ((world_position_2[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S43] - 1.0f));

#line 979
        axis_0 = axis_0 + 1U;

#line 979
    }

#line 986
    return reach_0;
}

float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 989
    uint level_0 = 0U;

    for(;;)
    {

#line 991
        uint _S45 = level_0 + 1U;

#line 991
        if(_S45 < levels_0)
        {
        }
        else
        {

#line 991
            break;
        }
        float _S46 = float(level_0);

#line 993
        float at_0 = reach_1 * exp2(- _S46);
        if(at_0 < 1.0f)
        {

#line 995
            return float2(_S46, saturate((1.0f - at_0) / 0.25f));
        }

#line 991
        level_0 = _S45;

#line 991
    }

#line 997
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 926
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_0)
{
    uint at_1 = cell_1 + offset_0;

#line 928
    uint _S47;
    if(at_1 >= count_0)
    {

#line 929
        _S47 = at_1 - count_0;

#line 929
    }
    else
    {

#line 929
        _S47 = at_1;

#line 929
    }

#line 929
    return _S47;
}

uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_16)
{
    uint3 counts_0 = kernelContext_16->camera_0->probe_counts_1.xyz;
    uint3 offset_1 = kernelContext_16->camera_0->probe_level_offset_1[level_1].xyz;
    uint _S48 = counts_0.x;
    uint _S49 = counts_0.y;



    return min(kernelContext_16->camera_0->probe_levels_1.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S49 + probe_wrap_0(cell_2.y, offset_1.y, _S49)) * _S48 + probe_wrap_0(cell_2.x, offset_1.x, _S48), max(kernelContext_16->camera_0->probe_counts_1.w, 1U) - 1U);
}


#line 867
float sign_not_zero_0(float value_0)
{

#line 867
    float _S50;

    if(value_0 >= 0.0f)
    {

#line 869
        _S50 = 1.0f;

#line 869
    }
    else
    {

#line 869
        _S50 = -1.0f;

#line 869
    }

#line 869
    return _S50;
}

float2 oct_encode_0(float3 direction_1)
{
    float _S51 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S51) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 875
    float2 p_1;
    if(_S51 < 0.0f)
    {
        float _S52 = p_0.y;

#line 878
        float _S53 = p_0.x;

#line 878
        p_1 = float2((1.0f - abs(_S52)) * sign_not_zero_0(_S53), (1.0f - abs(_S53)) * sign_not_zero_0(_S52));

#line 876
    }
    else
    {

#line 876
        p_1 = p_0;

#line 876
    }

#line 881
    return p_1;
}

float2 probe_moments_0(uint index_3, float3 direction_2, KernelContext_0 thread* kernelContext_17)
{

#line 884
    texture2d_array<float, access::sample> _S54 = kernelContext_17->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S54).get_width(0)),(*((&height_0)) = (_S54).get_height(0)),(*((&layers_0)) = (_S54).get_array_size());

#line 889
    float2 _S55 = float2(0.5f) ;

#line 889
    float2 _S56 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_2) * _S55 + _S55) * float2(16.0f)  + _S56 - _S55;
    float2 _S57 = float2(float(width_0), float(height_0)) - _S56;

#line 893
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S57);
    float2 high_0 = min(low_0 + _S56, _S57);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_3, max(layers_0, 1U) - 1U));

    int _S58 = int(low_0.x);

#line 898
    int _S59 = int(low_0.y);

#line 898
    int4 _S60 = int4(_S58, _S59, layer_0, int(0));
    int _S61 = int(high_0.x);

#line 899
    int4 _S62 = int4(_S61, _S59, layer_0, int(0));
    int _S63 = int(high_0.y);

#line 900
    int4 _S64 = int4(_S58, _S63, layer_0, int(0));
    int4 _S65 = int4(_S61, _S63, layer_0, int(0));
    float2 _S66 = float2(weight_0.x) ;

#line 902
    return mix(mix(((kernelContext_17->probe_visibility_0).read(vec<uint,2>(((_S60)).xy), uint(((_S60)).z), uint(((_S60)).w))).xy, ((kernelContext_17->probe_visibility_0).read(vec<uint,2>(((_S62)).xy), uint(((_S62)).z), uint(((_S62)).w))).xy, _S66), mix(((kernelContext_17->probe_visibility_0).read(vec<uint,2>(((_S64)).xy), uint(((_S64)).z), uint(((_S64)).w))).xy, ((kernelContext_17->probe_visibility_0).read(vec<uint,2>(((_S65)).xy), uint(((_S65)).z), uint(((_S65)).w))).xy, _S66), float2(weight_0.y) );
}

float probe_chebyshev_0(uint index_4, float3 probe_position_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_18)
{
    float3 to_probe_0 = probe_position_0 - (world_position_3 + normal_2 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 908
    float2 _S67 = probe_moments_0(index_4, - to_probe_0, kernelContext_18);

#line 914
    float _S68 = _S67.x;

#line 914
    float _S69 = max(_S67.y - _S68 * _S68, 0.0f);
    float behind_0 = to_surface_0 - _S68;
    float bound_0 = _S69 / (_S69 + behind_0 * behind_0);

#line 916
    float _S70;
    if(to_surface_0 <= _S68)
    {

#line 917
        _S70 = 1.0f;

#line 917
    }
    else
    {

#line 917
        _S70 = bound_0 * bound_0 * bound_0;

#line 917
    }

#line 917
    return _S70;
}

float probe_weight_0(uint index_5, float3 probe_position_1, float3 world_position_4, float3 normal_3, KernelContext_0 thread* kernelContext_19)
{

#line 920
    float _S71 = probe_chebyshev_0(index_5, probe_position_1, world_position_4, normal_3, kernelContext_19);

    return max(_S71, 0.00009999999747379f);
}


#line 100
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 944
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 960
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_5, float3 normal_4, KernelContext_0 thread* kernelContext_20)
{

#line 961
    uint _S72 = probe_row_0(level_2, cell_3, kernelContext_20);


    GpuProbe_natural_0 stored_0 = kernelContext_20->probes_0[_S72];

#line 964
    float _S73 = probe_weight_0(_S72, origin_1 + float3(cell_3) * spacing_0, world_position_5, normal_4, kernelContext_20);



    thread WeightedProbe_0 corner_0;

#line 968
    float4 _S74 = float4(_S73) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S74;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S74;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S74;
    (&corner_0)->weight_1 = _S73;
    return corner_0;
}


#line 950
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S75 = float4(t_0) ;

#line 953
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S75);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S75);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S75);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1000
float3 probe_level_environment_0(uint level_3, float3 world_position_6, float3 normal_5, float3 direction_3, KernelContext_0 thread* kernelContext_21)
{

#line 1000
    float3 _S76 = float3(1.0f) ;

    float3 _S77 = float3(0.0f, 0.0f, 0.0f);

#line 1002
    float3 last_1 = max(float3(kernelContext_21->camera_0->probe_counts_1.xyz) - _S76, _S77);



    float3 origin_2 = kernelContext_21->camera_0->probe_level_origin_1[level_3].xyz;
    float3 inv_0 = kernelContext_21->camera_0->probe_level_inv_spacing_1[level_3].xyz;
    float3 grid_0 = clamp((world_position_6 - origin_2) * inv_0, _S77, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S78 = uint3(base_0);
    uint3 _S79 = uint3(min(base_0 + _S76, last_1));

#line 1017
    float _S80 = inv_0.x;

#line 1017
    float _S81;

#line 1017
    if(_S80 != 0.0f)
    {

#line 1017
        _S81 = 1.0f / _S80;

#line 1017
    }
    else
    {

#line 1017
        _S81 = 0.0f;

#line 1017
    }
    float _S82 = inv_0.y;

#line 1018
    float _S83;

#line 1018
    if(_S82 != 0.0f)
    {

#line 1018
        _S83 = 1.0f / _S82;

#line 1018
    }
    else
    {

#line 1018
        _S83 = 0.0f;

#line 1018
    }
    float _S84 = inv_0.z;

#line 1019
    float _S85;

#line 1019
    if(_S84 != 0.0f)
    {

#line 1019
        _S85 = 1.0f / _S84;

#line 1019
    }
    else
    {

#line 1019
        _S85 = 0.0f;

#line 1019
    }

#line 1017
    float3 spacing_1 = float3(_S81, _S83, _S85);

#line 1026
    uint _S86 = _S78.x;

#line 1026
    uint _S87 = _S78.y;

#line 1026
    uint _S88 = _S78.z;

#line 1026
    WeightedProbe_0 _S89 = probe_corner_0(level_3, uint3(_S86, _S87, _S88), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);
    uint _S90 = _S79.x;

#line 1027
    WeightedProbe_0 _S91 = probe_corner_0(level_3, uint3(_S90, _S87, _S88), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);

#line 1027
    float _S92 = f_0.x;

#line 1027
    thread WeightedProbe_0 _S93 = _S89;

#line 1027
    thread WeightedProbe_0 _S94 = _S91;

#line 1027
    WeightedProbe_0 _S95 = lerp_probe_0(&_S93, &_S94, _S92);
    uint _S96 = _S79.y;

#line 1028
    WeightedProbe_0 _S97 = probe_corner_0(level_3, uint3(_S86, _S96, _S88), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);

#line 1028
    WeightedProbe_0 _S98 = probe_corner_0(level_3, uint3(_S90, _S96, _S88), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);

#line 1028
    thread WeightedProbe_0 _S99 = _S97;

#line 1028
    thread WeightedProbe_0 _S100 = _S98;

#line 1028
    WeightedProbe_0 _S101 = lerp_probe_0(&_S99, &_S100, _S92);

    uint _S102 = _S79.z;

#line 1030
    WeightedProbe_0 _S103 = probe_corner_0(level_3, uint3(_S86, _S87, _S102), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);

#line 1030
    WeightedProbe_0 _S104 = probe_corner_0(level_3, uint3(_S90, _S87, _S102), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);

#line 1030
    thread WeightedProbe_0 _S105 = _S103;

#line 1030
    thread WeightedProbe_0 _S106 = _S104;

#line 1030
    WeightedProbe_0 _S107 = lerp_probe_0(&_S105, &_S106, _S92);

#line 1030
    WeightedProbe_0 _S108 = probe_corner_0(level_3, uint3(_S86, _S96, _S102), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);

#line 1030
    WeightedProbe_0 _S109 = probe_corner_0(level_3, uint3(_S90, _S96, _S102), origin_2, spacing_1, world_position_6, normal_5, kernelContext_21);

#line 1030
    thread WeightedProbe_0 _S110 = _S108;

#line 1030
    thread WeightedProbe_0 _S111 = _S109;

#line 1030
    WeightedProbe_0 _S112 = lerp_probe_0(&_S110, &_S111, _S92);



    float _S113 = f_0.y;

#line 1034
    thread WeightedProbe_0 _S114 = _S95;

#line 1034
    thread WeightedProbe_0 _S115 = _S101;

#line 1034
    WeightedProbe_0 _S116 = lerp_probe_0(&_S114, &_S115, _S113);

#line 1034
    thread WeightedProbe_0 _S117 = _S107;

#line 1034
    thread WeightedProbe_0 _S118 = _S112;

#line 1034
    WeightedProbe_0 _S119 = lerp_probe_0(&_S117, &_S118, _S113);

    float _S120 = f_0.z;

#line 1036
    thread WeightedProbe_0 _S121 = _S116;

#line 1036
    thread WeightedProbe_0 _S122 = _S119;

#line 1036
    WeightedProbe_0 _S123 = lerp_probe_0(&_S121, &_S122, _S120);

#line 1036
    float3 _S124 = float3(2.09439516067504883f) ;

#line 1042
    return max(float3(dot(_S123.sh_0.sh_r_0.xyz / _S124, direction_3) + _S123.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S123.sh_0.sh_g_0.xyz / _S124, direction_3) + _S123.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S123.sh_0.sh_b_0.xyz / _S124, direction_3) + _S123.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S123.weight_1) , _S77);
}

float3 probe_environment_0(float3 world_position_7, float3 normal_6, float3 direction_4, KernelContext_0 thread* kernelContext_22)
{

#line 1053
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_7, kernelContext_22->camera_0->probe_level_origin_1[int(0)].xyz, kernelContext_22->camera_0->probe_level_inv_spacing_1[int(0)].xyz, max(float3(kernelContext_22->camera_0->probe_counts_1.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_22->camera_0->probe_levels_1.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1055
    float3 _S125 = probe_level_environment_0(level_4, world_position_7, normal_6, direction_4, kernelContext_22);


    if(share_0 >= 1.0f)
    {

#line 1059
        return _S125;
    }

#line 1059
    float3 _S126 = probe_level_environment_0(level_4 + 1U, world_position_7, normal_6, direction_4, kernelContext_22);

    return _S126 * float3((1.0f - share_0))  + _S125 * float3(share_0) ;
}


#line 772
float2 decode_fixed_pair_0(float4 texel_1)
{
    return float2(texel_1.x * 65280.0f + texel_1.y * 255.0f, texel_1.z * 65280.0f + texel_1.w * 255.0f) / float2(65535.0f) ;
}


float2 fixed_pair_at_0(texture2d<float, access::sample> table_0, float2 at_2)
{
    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (table_0).get_width(0)),(*((&height_1)) = (table_0).get_height(0));
    float2 extent_2 = float2(float(width_1), float(height_1));
    float2 scaled_1 = saturate(at_2) * extent_2 - float2(0.5f) ;

#line 784
    float2 _S127 = float2(1.0f) ;
    float2 _S128 = extent_2 - _S127;

#line 785
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S128);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S129 = int2(low_1);
    int2 _S130 = int2(min(low_1 + _S127, _S128));
    int _S131 = _S129.x;

#line 791
    int _S132 = _S129.y;

#line 791
    int3 _S133 = int3(_S131, _S132, int(0));
    int _S134 = _S130.x;

#line 792
    int3 _S135 = int3(_S134, _S132, int(0));
    float2 _S136 = float2(weight_2.x) ;
    int _S137 = _S130.y;

#line 794
    int3 _S138 = int3(_S131, _S137, int(0));
    int3 _S139 = int3(_S134, _S137, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S133)).xy), uint(((_S133)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S135)).xy), uint(((_S135)).z)))), _S136), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S138)).xy), uint(((_S138)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S139)).xy), uint(((_S139)).z)))), _S136), float2(weight_2.y) );
}

float2 sky_prefilter_at_0(float up_0, float roughness_0, KernelContext_0 thread* kernelContext_23)
{
    return fixed_pair_at_0(kernelContext_23->sky_prefilter_0, float2(up_0, roughness_0));
}

float3 sky_prefiltered_0(float3 direction_5, float roughness_1, KernelContext_0 thread* kernelContext_24)
{
    float up_1 = clamp(direction_5.y, -1.0f, 1.0f);

#line 807
    float2 _S140 = sky_prefilter_at_0(abs(up_1), roughness_1, kernelContext_24);

    bool _S141 = up_1 >= 0.0f;

#line 809
    float3 far_0;

#line 809
    if(_S141)
    {

#line 809
        far_0 = kernelContext_24->camera_0->sky_0[int(0)].xyz;

#line 809
    }
    else
    {

#line 809
        far_0 = kernelContext_24->camera_0->sky_0[int(2)].xyz;

#line 809
    }

#line 809
    float3 opposite_0;
    if(_S141)
    {

#line 810
        opposite_0 = kernelContext_24->camera_0->sky_0[int(2)].xyz;

#line 810
    }
    else
    {

#line 810
        opposite_0 = kernelContext_24->camera_0->sky_0[int(0)].xyz;

#line 810
    }
    float _S142 = _S140.x;

#line 811
    float _S143 = _S140.y;
    return kernelContext_24->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S142 - _S143))  + far_0 * float3(_S142)  + opposite_0 * float3(_S143) ;
}

float3 sky_view_at_0(float up_2, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_25)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_1 = clamp(up_2, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_1));

#line 819
    float _S144;
    if(clamped_1 >= 0.0f)
    {

#line 820
        _S144 = root_0;

#line 820
    }
    else
    {

#line 820
        _S144 = - root_0;

#line 820
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S144, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S145 = uint(x0_0);

#line 834
    float3 _S146 = float3((1.0f - fx_0)) ;
    uint _S147 = uint(min(x0_0 + 1.0f, 95.0f));

#line 835
    float3 _S148 = float3(fx_0) ;


    return ((float4(*(kernelContext_25->sky_view_0+(row0_0 + _S145))) ).xyz * _S146 + (float4(*(kernelContext_25->sky_view_0+(row0_0 + _S147))) ).xyz * _S148) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_25->sky_view_0+(row1_0 + _S145))) ).xyz * _S146 + (float4(*(kernelContext_25->sky_view_0+(row1_0 + _S147))) ).xyz * _S148) * float3(fy_0) ;
}

float3 atmosphere_radiance_0(float3 direction_6, KernelContext_0 thread* kernelContext_26)
{
    float3 sun_0 = kernelContext_26->camera_0->atmosphere_0.xyz;
    float _S149 = direction_6.x;

#line 844
    float _S150 = direction_6.z;

#line 844
    float view_flat_0 = sqrt(_S149 * _S149 + _S150 * _S150);
    float _S151 = sun_0.x;

#line 845
    float _S152 = sun_0.z;

#line 845
    float sun_flat_0 = sqrt(_S151 * _S151 + _S152 * _S152);

#line 845
    bool _S153;

    if(view_flat_0 > 0.0f)
    {

#line 847
        _S153 = sun_flat_0 > 0.0f;

#line 847
    }
    else
    {

#line 847
        _S153 = false;

#line 847
    }

#line 847
    float cosine_1;

#line 847
    if(_S153)
    {

#line 847
        cosine_1 = (_S149 * _S151 + _S150 * _S152) / (view_flat_0 * sun_flat_0);

#line 847
    }
    else
    {

#line 847
        cosine_1 = 1.0f;

#line 847
    }

#line 847
    float3 _S154 = sky_view_at_0(direction_6.y, cosine_1, kernelContext_26);



    return _S154;
}

float3 sky_environment_0(float3 direction_7, float roughness_2, float share_1, KernelContext_0 thread* kernelContext_27)
{

#line 854
    float3 _S155 = sky_prefiltered_0(direction_7, roughness_2, kernelContext_27);


    if((kernelContext_27->camera_0->atmosphere_0.w) <= 0.0f)
    {
        return _S155;
    }



    float3 _S156 = _S155 * float3((1.0f - share_1)) ;

#line 864
    float3 _S157 = atmosphere_radiance_0(direction_7, kernelContext_27);

#line 864
    return _S156 + _S157 * float3(share_1) ;
}


#line 369
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S158 = - d_0;

#line 373
        float series_0 = 0.00833333376795053f;

#line 373
        int term_2 = int(3);

        for(;;)
        {

#line 375
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 375
                break;
            }
            float _S159 = series_0 * _S158 + FOG_RATIO_KERNEL_0[term_2];

#line 375
            int term_3 = term_2 - int(1);

#line 375
            series_0 = _S159;

#line 375
            term_2 = term_3;

#line 375
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}

float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_0)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_0, 0.0f, 32.0f);
    }

#line 395
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 409
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_1 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S160 = a_1 * a_1;

#line 412
    float d_1 = 1.0f + _S160 - 2.0f * a_1 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S160) / (d_1 * sqrt(d_1));
}

float3 volumetric_source_0(float3 view_direction_0, float4 lit_0, KernelContext_0 thread* kernelContext_28)
{



    return kernelContext_28->params_0->fog_color_1.xyz + kernelContext_28->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_28->params_0->sun_direction_0.w, dot(kernelContext_28->params_0->sun_direction_0.xyz, view_direction_0)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 1066
struct WaterAir_0
{
    float survives_0;
    float3 inscatter_0;
};


#line 1080
WaterAir_0 water_froxel_air_0(int2 pixel_7, float depth_4, KernelContext_0 thread* kernelContext_29)
{
    thread WaterAir_0 air_0;
    (&air_0)->survives_0 = 1.0f;
    (&air_0)->inscatter_0 = float3(0.0f, 0.0f, 0.0f);

    uint _S161 = max(kernelContext_29->params_0->grid_x_0, 1U);
    uint _S162 = max(kernelContext_29->params_0->grid_y_0, 1U);
    uint _S163 = max(kernelContext_29->params_0->slices_0, 1U);
    uint tiles_0 = _S161 * _S162;
    uint _S164 = max(kernelContext_29->params_0->tile_pixels_0, 1U);


    int _S165 = pixel_7.x;
    int _S166 = pixel_7.y;

#line 1092
    float2 ndc_3 = float2((float(_S165) + 0.5f) / float(max(kernelContext_29->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S166) + 0.5f) / float(max(kernelContext_29->params_0->viewport_y_0, 1U)) * 2.0f);

#line 1092
    float view_depth_0;



    if(depth_4 > 0.0f)
    {

#line 1096
        float3 _S167 = volumetric_unproject_0(ndc_3, depth_4, kernelContext_29);

#line 1096
        view_depth_0 = dot(kernelContext_29->params_0->depth_row_0, float4(_S167, 1.0f));

#line 1096
    }
    else
    {

#line 1096
        view_depth_0 = 1000.0f;

#line 1096
    }

#line 1101
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 1101
    float slice_start_0 = 0.0f;

#line 1101
    uint slice_0 = 0U;

#line 1101
    float next_start_0 = 0.14677993953227997f;

#line 1119
    for(;;)
    {

#line 1119
        uint _S168 = slice_0 + 1U;

#line 1119
        bool _S169;

#line 1119
        if(_S168 < _S163)
        {

#line 1119
            _S169 = next_start_0 <= view_depth_1;

#line 1119
        }
        else
        {

#line 1119
            _S169 = false;

#line 1119
        }

#line 1119
        if(_S169)
        {
        }
        else
        {

#line 1119
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 1122
        slice_start_0 = next_start_0;

#line 1122
        next_start_0 = next_start_1;

#line 1122
        slice_0 = _S168;

#line 1119
    }

#line 1129
    uint _S170 = uint(max(_S165, int(0))) / _S164;

#line 1129
    uint _S171 = min(_S170, _S161 - 1U);
    uint _S172 = uint(max(_S166, int(0))) / _S164;
    uint froxel_0 = _S171 + min(_S172, _S162 - 1U) * _S161 + slice_0 * tiles_0;
    if(froxel_0 >= (kernelContext_29->params_0->froxel_count_0))
    {
        return air_0;
    }

#line 1134
    float4 _S173 = float4(*(kernelContext_29->volumetrics_0+froxel_0)) ;

#line 1134
    float3 _S174 = volumetric_unproject_0(ndc_3, 1.0f, kernelContext_29);

#line 1143
    float3 along_0 = (_S174 - kernelContext_29->params_0->eye_0.xyz) / float3(max(dot(kernelContext_29->params_0->depth_row_0, float4(_S174, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = kernelContext_29->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = kernelContext_29->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;
    float reference_4 = kernelContext_29->params_0->fog_params_1.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_29->params_0->fog_params_1.x, kernelContext_29->params_0->fog_params_1.y, from_0.y - reference_4, to_0.y - reference_4, length_of_0));

#line 1151
    float3 view_direction_1;

#line 1157
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 1157
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 1157
    }
    else
    {

#line 1157
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 1157
    }

#line 1157
    float3 _S175 = volumetric_source_0(view_direction_1, float4(*(kernelContext_29->lighting_0+froxel_0)) , kernelContext_29);

#line 1164
    float3 partial_radiance_0 = _S175 * float3((1.0f - partial_survives_0)) ;

    float _S176 = _S173.w;

#line 1166
    (&air_0)->survives_0 = _S176 * partial_survives_0;
    (&air_0)->inscatter_0 = _S173.xyz + float3(_S176)  * partial_radiance_0;
    return air_0;
}


#line 398
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 1173
WaterAir_0 water_analytic_air_0(float3 surface_1, KernelContext_0 thread* kernelContext_30)
{

#line 1180
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0(kernelContext_30->frame_0->fog_params_0.x, kernelContext_30->frame_0->fog_params_0.y, kernelContext_30->frame_0->camera_position_0.y - kernelContext_30->frame_0->fog_params_0.z, surface_1.y - kernelContext_30->frame_0->fog_params_0.z, length(kernelContext_30->frame_0->camera_position_0.xyz - surface_1)));

    thread WaterAir_0 air_1;
    (&air_1)->survives_0 = fog_survives_0;
    (&air_1)->inscatter_0 = kernelContext_30->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;
    return air_1;
}


#line 1185
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1185
struct pixelInput_0
{
    float3 world_position_8 [[user(TEXCOORD)]];
    [[flat]] uint body_1 [[user(TEXCOORD_1)]];
};


#line 1246
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S177 [[stage_in]], float4 position_1 [[position]], WaterVertex_natural_0 device* water_vertices_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(1)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], WaterMedium_natural_0 device* media_1 [[buffer(5)]], texture2d<float, access::sample> scene_color_1 [[texture(0)]], VolumetricParams_natural_0 constant* params_1 [[buffer(3)]], WaterParams_0 constant* water_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(2)]], sampler shadow_sampler_1 [[sampler(0)]], SsrParams_natural_0 constant* camera_1 [[buffer(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(3)]], packed_float4 device* sky_view_1 [[buffer(7)]], packed_float4 device* volumetrics_1 [[buffer(8)]], packed_float4 device* lighting_1 [[buffer(9)]])
{

#line 1246
    thread KernelContext_0 kernelContext_31;

#line 1246
    (&kernelContext_31)->water_vertices_0 = water_vertices_1;

#line 1246
    (&kernelContext_31)->frame_0 = frame_1;

#line 1246
    (&kernelContext_31)->scene_depth_0 = scene_depth_1;

#line 1246
    (&kernelContext_31)->media_0 = media_1;

#line 1246
    (&kernelContext_31)->scene_color_0 = scene_color_1;

#line 1246
    (&kernelContext_31)->params_0 = params_1;

#line 1246
    (&kernelContext_31)->water_0 = water_1;

#line 1246
    (&kernelContext_31)->shadow_atlas_0 = shadow_atlas_1;

#line 1246
    (&kernelContext_31)->shadow_sampler_0 = shadow_sampler_1;

#line 1246
    (&kernelContext_31)->camera_0 = camera_1;

#line 1246
    (&kernelContext_31)->probes_0 = probes_1;

#line 1246
    (&kernelContext_31)->probe_visibility_0 = probe_visibility_1;

#line 1246
    (&kernelContext_31)->sky_prefilter_0 = sky_prefilter_1;

#line 1246
    (&kernelContext_31)->sky_view_0 = sky_view_1;

#line 1246
    (&kernelContext_31)->volumetrics_0 = volumetrics_1;

#line 1246
    (&kernelContext_31)->lighting_0 = lighting_1;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    float2 extent_3 = float2(float(width_2), float(height_2));
    int2 last_2 = int2(int(width_2) - int(1), int(height_2) - int(1));
    float2 _S178 = position_1.xy;

#line 1253
    int2 _S179 = int2(_S178);
    float surface_depth_0 = position_1.z;

    WaterMedium_natural_0 medium_0 = media_1[_S177.body_1];

    float3 up_3 = float3(0.0f, 1.0f, 0.0f);


    float3 incident_1 = normalize(_S177.world_position_8 - frame_1->camera_position_0.xyz);
    float _S180 = - incident_1.y;

#line 1262
    float cos_incident_1 = saturate(_S180);

#line 1269
    int3 _S181 = int3(_S179, int(0));

#line 1269
    float4 straight_0 = ((scene_color_1).read(vec<uint,2>(((_S181)).xy), uint(((_S181)).z)));

#line 1269
    float3 _S182 = water_opaque_point_0(_S179, ((scene_depth_1).read(vec<uint,2>(((_S181)).xy), uint(((_S181)).z))), extent_3, _S177.world_position_8, incident_1, &kernelContext_31);

#line 1279
    float3 refracted_0 = water_refract_0(incident_1, up_3, 0.75f);

    float4 landing_clip_0 = (((float4(_S177.world_position_8 + refracted_0 * float3((max(_S177.world_position_8.y - _S182.y, 0.0f) / max(- refracted_0.y, 9.99999997475242708e-07f))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_31)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_31)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));



    float _S183 = landing_clip_0.w;

#line 1285
    float3 behind_point_0;

#line 1285
    float4 behind_1;

#line 1285
    if(_S183 > 0.0f)
    {
        float2 landing_ndc_0 = landing_clip_0.xy / float2(_S183) ;
        int2 bent_0 = clamp(int2(float2(landing_ndc_0.x * 0.5f + 0.5f, 0.5f - landing_ndc_0.y * 0.5f) * extent_3), int2(int(0), int(0)), last_2);


        int3 _S184 = int3(bent_0, int(0));

#line 1291
        float bent_depth_0 = (((&kernelContext_31)->scene_depth_0).read(vec<uint,2>(((_S184)).xy), uint(((_S184)).z)));

#line 1297
        if(bent_depth_0 < surface_depth_0)
        {
            float4 _S185 = (((&kernelContext_31)->scene_color_0).read(vec<uint,2>(((_S184)).xy), uint(((_S184)).z)));

#line 1299
            float3 _S186 = water_opaque_point_0(bent_0, bent_depth_0, extent_3, _S177.world_position_8, refracted_0, &kernelContext_31);

#line 1299
            behind_point_0 = _S186;

#line 1299
            behind_1 = _S185;

#line 1297
        }
        else
        {

#line 1297
            behind_point_0 = _S182;

#line 1297
            behind_1 = straight_0;

#line 1297
        }

#line 1285
    }
    else
    {

#line 1285
        behind_point_0 = _S182;

#line 1285
        behind_1 = straight_0;

#line 1285
    }

#line 1305
    float thickness_0 = length(behind_point_0 - _S177.world_position_8);

#line 1305
    float4 _S187 = float4(medium_0.scattering_0) ;
    float3 extinction_0 = (float4(medium_0.absorption_0) ).xyz + _S187.xyz;
    float _S188 = extinction_0.x;
    float _S189 = extinction_0.y;
    float _S190 = extinction_0.z;

#line 1307
    float3 transmittance_0 = float3(fog_exp_neg_0(_S188 * thickness_0), fog_exp_neg_0(_S189 * thickness_0), fog_exp_neg_0(_S190 * thickness_0));

#line 1307
    float _S191;

#line 1315
    if(_S188 > 0.0f)
    {

#line 1315
        _S191 = _S187.x / _S188;

#line 1315
    }
    else
    {

#line 1315
        _S191 = 0.0f;

#line 1315
    }

#line 1315
    float _S192;
    if(_S189 > 0.0f)
    {

#line 1316
        _S192 = _S187.y / _S189;

#line 1316
    }
    else
    {

#line 1316
        _S192 = 0.0f;

#line 1316
    }

#line 1316
    float _S193;
    if(_S190 > 0.0f)
    {

#line 1317
        _S193 = _S187.z / _S190;

#line 1317
    }
    else
    {

#line 1317
        _S193 = 0.0f;

#line 1317
    }

#line 1315
    float3 albedo_0 = float3(_S191, _S192, _S193);


    float3 to_sun_0 = (&kernelContext_31)->water_0->sun_direction_1.xyz;


    float _S194 = to_sun_0.y;

#line 1319
    thread uint cascade_3;
    thread float cascade_fade_0;

#line 1320
    float _S195 = sun_visibility_0(_S177.world_position_8, to_sun_0, _S194, up_3, _S178, &cascade_3, &cascade_fade_0, &kernelContext_31);


    float3 _S196 = (&kernelContext_31)->frame_0->ambient_0.xyz;

#line 1323
    float3 _S197 = sky_irradiance_0(up_3, &kernelContext_31);

#line 1323
    float3 _S198 = float3(1.0f) ;

    float3 scattered_0 = albedo_0 * (_S196 + _S197 + (&kernelContext_31)->water_0->sun_color_0.xyz * float3((max(_S194, 0.0f) * _S195)) ) * (_S198 - transmittance_0);

#line 1332
    float3 mirrored_0 = float3(incident_1.x, _S180, incident_1.z);

#line 1332
    float3 _S199 = probe_environment_0(_S177.world_position_8, up_3, mirrored_0, &kernelContext_31);

#line 1332
    float3 _S200 = sky_environment_0(mirrored_0, 0.0f, 1.0f, &kernelContext_31);

    float3 environment_0 = _S199 + _S200;
    float grazing_0 = 1.0f - cos_incident_1;
    float grazing2_0 = grazing_0 * grazing_0;
    float fresnel_0 = 0.01999999955296516f + 0.98000001907348633f * (grazing2_0 * grazing2_0 * grazing_0);

#line 1337
    WaterAir_0 air_2;

#line 1348
    if(((&kernelContext_31)->water_0->froxels_0.x) != 0U)
    {

#line 1348
        WaterAir_0 _S201 = water_froxel_air_0(_S179, surface_depth_0, &kernelContext_31);

#line 1348
        air_2 = _S201;

#line 1348
    }
    else
    {

#line 1348
        WaterAir_0 _S202 = water_analytic_air_0(_S177.world_position_8, &kernelContext_31);

#line 1348
        air_2 = _S202;

#line 1348
    }

#line 1348
    float3 _S203 = float3((1.0f - fresnel_0)) ;

#line 1365
    float fade_1 = saturate(thickness_0 / 0.25f);

#line 1365
    pixelOutput_0 _S204 = { float4(straight_0.xyz * float3((1.0f - fade_1))  + (behind_1.xyz * transmittance_0 * _S203 + (environment_0 * float3(fresnel_0)  + scattered_0 * _S203) * float3(air_2.survives_0)  + air_2.inscatter_0 * (_S198 - transmittance_0 * _S203)) * float3(fade_1) , straight_0.w) };
    return _S204;
}


#line 1366
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_9 [[user(TEXCOORD)]];
    uint body_2 [[user(TEXCOORD_1)]];
};


#line 1222
struct SurfaceOutput_0
{
    float4 position_3;
    float3 world_position_10;
    [[flat]] uint body_3;
};


#line 1222
[[vertex]] vertexMain_Result_0 vertexMain(uint index_6 [[vertex_id]], WaterVertex_natural_0 device* water_vertices_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(1)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], WaterMedium_natural_0 device* media_2 [[buffer(5)]], texture2d<float, access::sample> scene_color_2 [[texture(0)]], VolumetricParams_natural_0 constant* params_2 [[buffer(3)]], WaterParams_0 constant* water_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(2)]], sampler shadow_sampler_2 [[sampler(0)]], SsrParams_natural_0 constant* camera_2 [[buffer(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(3)]], packed_float4 device* sky_view_2 [[buffer(7)]], packed_float4 device* volumetrics_2 [[buffer(8)]], packed_float4 device* lighting_2 [[buffer(9)]])
{

#line 1222
    thread KernelContext_0 kernelContext_32;

#line 1222
    (&kernelContext_32)->water_vertices_0 = water_vertices_2;

#line 1222
    (&kernelContext_32)->frame_0 = frame_2;

#line 1222
    (&kernelContext_32)->scene_depth_0 = scene_depth_2;

#line 1222
    (&kernelContext_32)->media_0 = media_2;

#line 1222
    (&kernelContext_32)->scene_color_0 = scene_color_2;

#line 1222
    (&kernelContext_32)->params_0 = params_2;

#line 1222
    (&kernelContext_32)->water_0 = water_2;

#line 1222
    (&kernelContext_32)->shadow_atlas_0 = shadow_atlas_2;

#line 1222
    (&kernelContext_32)->shadow_sampler_0 = shadow_sampler_2;

#line 1222
    (&kernelContext_32)->camera_0 = camera_2;

#line 1222
    (&kernelContext_32)->probes_0 = probes_2;

#line 1222
    (&kernelContext_32)->probe_visibility_0 = probe_visibility_2;

#line 1222
    (&kernelContext_32)->sky_prefilter_0 = sky_prefilter_2;

#line 1222
    (&kernelContext_32)->sky_view_0 = sky_view_2;

#line 1222
    (&kernelContext_32)->volumetrics_0 = volumetrics_2;

#line 1222
    (&kernelContext_32)->lighting_0 = lighting_2;

#line 1237
    WaterVertex_natural_0 vertex_0 = water_vertices_2[index_6];
    thread SurfaceOutput_0 output_1;
    float3 _S205 = (float4(vertex_0.position_0) ).xyz;

#line 1239
    (&output_1)->world_position_10 = _S205;
    (&output_1)->position_3 = (((float4(_S205, 1.0f)) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_0[int(0)][int(0)], frame_2->view_proj_0.data_0[int(1)][int(0)], frame_2->view_proj_0.data_0[int(2)][int(0)], frame_2->view_proj_0.data_0[int(3)][int(0)], frame_2->view_proj_0.data_0[int(0)][int(1)], frame_2->view_proj_0.data_0[int(1)][int(1)], frame_2->view_proj_0.data_0[int(2)][int(1)], frame_2->view_proj_0.data_0[int(3)][int(1)], frame_2->view_proj_0.data_0[int(0)][int(2)], frame_2->view_proj_0.data_0[int(1)][int(2)], frame_2->view_proj_0.data_0[int(2)][int(2)], frame_2->view_proj_0.data_0[int(3)][int(2)], frame_2->view_proj_0.data_0[int(0)][int(3)], frame_2->view_proj_0.data_0[int(1)][int(3)], frame_2->view_proj_0.data_0[int(2)][int(3)], frame_2->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_1)->body_3 = (uint4(vertex_0.body_0) ).x;

#line 1241
    thread vertexMain_Result_0 _S206;

#line 1241
    (&_S206)->position_2 = output_1.position_3;

#line 1241
    (&_S206)->world_position_9 = output_1.world_position_10;

#line 1241
    (&_S206)->body_2 = output_1.body_3;

#line 1241
    return _S206;
}

