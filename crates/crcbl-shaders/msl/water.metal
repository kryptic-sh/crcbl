#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 260 "shaders/water.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 276
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 295
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 306
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 257
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1231
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


#line 1240
float3 water_opaque_point_0(int2 pixel_1, float depth_2, float2 extent_1, float3 surface_0, float3 direction_0, KernelContext_0 thread* kernelContext_2)
{

    if(depth_2 <= 0.0f)
    {
        return surface_0 + direction_0 * float3(1000.0f) ;
    }

#line 1245
    float3 _S1 = volumetric_unproject_1(water_ndc_0(pixel_1, extent_1), depth_2, kernelContext_2);

    return _S1;
}


#line 1221
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


#line 476
float tile_tap_1(float4 rect_6, float2 texel_step_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_10)
{

    float2 tile_min_1 = float2(0.5f, 0.5f) * texel_step_1;

    float _S10 = spoke_1.x;

#line 481
    float _S11 = rotation_1.x;

#line 481
    float _S12 = spoke_1.y;

#line 481
    float _S13 = rotation_1.y;


    float _S14 = ((kernelContext_10->shadow_atlas_0).sample_compare((kernelContext_10->shadow_sampler_0), (atlas_uv_0(rect_6, clamp(tile_uv_2 + float2(_S10 * _S11 - _S12 * _S13, _S10 * _S13 + _S12 * _S11) * texel_step_1, tile_min_1, float2(1.0f)  - tile_min_1))), (reference_1), level((0.0f))));

#line 484
    return _S14;
}


#line 547
float tile_box_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, KernelContext_0 thread* kernelContext_11)
{

#line 547
    float4 _S15 = atlas_rect_1(tile_2, kernelContext_11);


    if(atlas_rect_is_empty_0(_S15))
    {
        return 1.0f;
    }

#line 552
    float2 _S16 = atlas_step_1(_S15, kernelContext_11);

#line 552
    int y_0 = int(-1);

#line 552
    float visibility_0 = 0.0f;

#line 557
    for(;;)
    {

#line 557
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 557
            break;
        }

#line 557
        int x_1 = int(-1);

        for(;;)
        {

#line 559
            if(x_1 <= int(1))
            {
            }
            else
            {

#line 559
                break;
            }

#line 559
            float _S17 = tile_tap_1(_S15, _S16, tile_uv_3, float2(float(x_1), float(y_0)), float2(1.0f, 0.0f), reference_2, kernelContext_11);

            float visibility_1 = visibility_0 + _S17;

#line 559
            x_1 = x_1 + int(1);

#line 559
            visibility_0 = visibility_1;

#line 559
        }

#line 557
        y_0 = y_0 + int(1);

#line 557
    }

#line 565
    return visibility_0 / 9.0f;
}


#line 439
float2 shadow_rotation_0(float2 pixel_3)
{
    uint2 cell_0 = uint2(pixel_3) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 487
float tile_pcf_0(uint tile_3, float2 tile_uv_4, float reference_3, float2 pixel_4, float radius_0, KernelContext_0 thread* kernelContext_12)
{
    float2 rotation_2 = shadow_rotation_0(pixel_4);

#line 489
    float4 _S18 = atlas_rect_0(tile_3, kernelContext_12);

    if(atlas_rect_is_empty_0(_S18))
    {
        return 1.0f;
    }

#line 493
    float2 _S19 = atlas_step_0(_S18, kernelContext_12);



    thread array<float, int(5)> probe_taps_0;

#line 497
    uint spot_0 = 0U;

#line 497
    float probe_0 = 0.0f;

    for(;;)
    {

#line 499
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 499
            break;
        }

#line 499
        float _S20 = tile_tap_0(_S18, _S19, tile_uv_4, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_0) , rotation_2, reference_3, kernelContext_12);

        probe_taps_0[spot_0] = _S20;


        float probe_1 = probe_0 + _S20;

#line 499
        spot_0 = spot_0 + 1U;

#line 499
        probe_0 = probe_1;

#line 499
    }

#line 510
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 516
    uint index_0 = 1U;

#line 516
    float visibility_2 = probe_taps_0[int(0)];

#line 524
    for(;;)
    {

#line 524
        if(index_0 < 23U)
        {
        }
        else
        {

#line 524
            break;
        }

#line 524
        float _S21 = tile_tap_0(_S18, _S19, tile_uv_4, SHADOW_DISC_0[index_0] * float2(radius_0) , rotation_2, reference_3, kernelContext_12);

        float visibility_3 = visibility_2 + _S21;

#line 524
        index_0 = index_0 + 1U;

#line 524
        visibility_2 = visibility_3;

#line 524
    }

#line 529
    float visibility_4 = visibility_2 + probe_taps_0[int(1)];

#line 529
    float2 _S22 = float2(radius_0) ;

#line 529
    float _S23 = tile_tap_0(_S18, _S19, tile_uv_4, float2(0.43503299355506897f, 0.75919097661972046f) * _S22, rotation_2, reference_3, kernelContext_12);


    float visibility_5 = visibility_4 + _S23 + probe_taps_0[int(2)];

#line 532
    float _S24 = tile_tap_0(_S18, _S19, tile_uv_4, float2(0.82610201835632324f, -0.38168001174926758f) * _S22, rotation_2, reference_3, kernelContext_12);


    float visibility_6 = visibility_5 + _S24 + probe_taps_0[int(3)];

#line 535
    float _S25 = tile_tap_0(_S18, _S19, tile_uv_4, float2(-0.31940698623657227f, -0.88803398609161377f) * _S22, rotation_2, reference_3, kernelContext_12);


    float visibility_7 = visibility_6 + _S25 + probe_taps_0[int(4)];

#line 538
    index_0 = 30U;

#line 538
    visibility_2 = visibility_7;
    for(;;)
    {

#line 539
        if(index_0 < 32U)
        {
        }
        else
        {

#line 539
            break;
        }

#line 539
        float _S26 = tile_tap_0(_S18, _S19, tile_uv_4, SHADOW_DISC_0[index_0] * _S22, rotation_2, reference_3, kernelContext_12);

        float visibility_8 = visibility_2 + _S26;

#line 539
        index_0 = index_0 + 1U;

#line 539
        visibility_2 = visibility_8;

#line 539
    }

#line 544
    return visibility_2 / 32.0f;
}


#line 568
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_5, float reference_4, float2 rotation_3, KernelContext_0 thread* kernelContext_13)
{
    float2 texel_0 = kernelContext_13->frame_0->shadow_params_0.xy;

#line 570
    float4 _S27 = atlas_rect_0(cascade_0, kernelContext_13);

#line 570
    float2 _S28 = atlas_step_0(_S27, kernelContext_13);


    float2 _S29 = float2(0.5f, 0.5f) * _S28;


    float2 _S30 = float2(1.0f, 1.0f);

#line 576
    float2 _S31 = _S30 / texel_0;

#line 586
    float _S32 = rotation_3.x;

#line 586
    float _S33 = rotation_3.y;

    float2 _S34 = float2(1.0f)  - _S29;

#line 593
    float2 _S35 = _S31 - _S30;
    int3 _S36 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(1.41421604156494141f * _S32 - 0.0f * _S33, 1.41421604156494141f * _S33 + 0.0f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_3 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S36)).xy), uint(((_S36)).z)));

#line 594
    float sum_0;

#line 594
    float found_0;
    if(depth_3 > reference_4)
    {

#line 595
        sum_0 = depth_3;

#line 595
        found_0 = 1.0f;

#line 595
    }
    else
    {

#line 595
        sum_0 = 0.0f;

#line 595
        found_0 = 0.0f;

#line 595
    }

#line 594
    int3 _S37 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-1.80617594718933105f * _S32 - 1.65460801124572754f * _S33, -1.80617594718933105f * _S33 + 1.65460801124572754f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_4 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S37)).xy), uint(((_S37)).z)));
    if(depth_4 > reference_4)
    {

        float found_1 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_4;

#line 598
        found_0 = found_1;

#line 595
    }

#line 594
    int3 _S38 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(0.27646398544311523f * _S32 - -3.15016794204711914f * _S33, 0.27646398544311523f * _S33 + -3.15016794204711914f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_5 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S38)).xy), uint(((_S38)).z)));
    if(depth_5 > reference_4)
    {

        float found_2 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_5;

#line 598
        found_0 = found_2;

#line 595
    }

#line 594
    int3 _S39 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(2.27656793594360352f * _S32 - 2.96938395500183105f * _S33, 2.27656793594360352f * _S33 + 2.96938395500183105f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_6 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S39)).xy), uint(((_S39)).z)));
    if(depth_6 > reference_4)
    {

        float found_3 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_6;

#line 598
        found_0 = found_3;

#line 595
    }

#line 594
    int3 _S40 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-4.17778396606445312f * _S32 - -0.73899197578430176f * _S33, -4.17778396606445312f * _S33 + -0.73899197578430176f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_7 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S40)).xy), uint(((_S40)).z)));
    if(depth_7 > reference_4)
    {

        float found_4 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_7;

#line 598
        found_0 = found_4;

#line 595
    }

#line 594
    int3 _S41 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(3.95756006240844727f * _S32 - -2.51747989654541016f * _S33, 3.95756006240844727f * _S33 + -2.51747989654541016f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_8 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S41)).xy), uint(((_S41)).z)));
    if(depth_8 > reference_4)
    {

        float found_5 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_8;

#line 598
        found_0 = found_5;

#line 595
    }

#line 594
    int3 _S42 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-1.32372796535491943f * _S32 - 4.92420005798339844f * _S33, -1.32372796535491943f * _S33 + 4.92420005798339844f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_9 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S42)).xy), uint(((_S42)).z)));
    if(depth_9 > reference_4)
    {

        float found_6 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_9;

#line 598
        found_0 = found_6;

#line 595
    }

#line 594
    int3 _S43 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-2.52448797225952148f * _S32 - -4.86075210571289062f * _S33, -2.52448797225952148f * _S33 + -4.86075210571289062f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_10 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z)));
    if(depth_10 > reference_4)
    {

        float found_7 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_10;

#line 598
        found_0 = found_7;

#line 595
    }

#line 594
    int3 _S44 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(5.47713613510131836f * _S32 - 2.00024008750915527f * _S33, 5.47713613510131836f * _S33 + 2.00024008750915527f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_11 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S44)).xy), uint(((_S44)).z)));
    if(depth_11 > reference_4)
    {

        float found_8 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_11;

#line 598
        found_0 = found_8;

#line 595
    }

#line 594
    int3 _S45 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-5.69804811477661133f * _S32 - 2.35207200050354004f * _S33, -5.69804811477661133f * _S33 + 2.35207200050354004f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_12 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S45)).xy), uint(((_S45)).z)));
    if(depth_12 > reference_4)
    {

        float found_9 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_12;

#line 598
        found_0 = found_9;

#line 595
    }

#line 594
    int3 _S46 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(2.74683189392089844f * _S32 - -5.86983203887939453f * _S33, 2.74683189392089844f * _S33 + -5.86983203887939453f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_13 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)));
    if(depth_13 > reference_4)
    {

        float found_10 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_13;

#line 598
        found_0 = found_10;

#line 595
    }

#line 594
    int3 _S47 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(2.02983999252319336f * _S32 - 6.47145605087280273f * _S33, 2.02983999252319336f * _S33 + 6.47145605087280273f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_14 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S47)).xy), uint(((_S47)).z)));
    if(depth_14 > reference_4)
    {

        float found_11 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_14;

#line 598
        found_0 = found_11;

#line 595
    }

#line 594
    int3 _S48 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-6.11796808242797852f * _S32 - -3.54548811912536621f * _S33, -6.11796808242797852f * _S33 + -3.54548811912536621f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_15 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)));
    if(depth_15 > reference_4)
    {

        float found_12 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_15;

#line 598
        found_0 = found_12;

#line 595
    }

#line 594
    int3 _S49 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(7.17707204818725586f * _S32 - -1.57785594463348389f * _S33, 7.17707204818725586f * _S33 + -1.57785594463348389f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_16 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S49)).xy), uint(((_S49)).z)));
    if(depth_16 > reference_4)
    {

        float found_13 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_16;

#line 598
        found_0 = found_13;

#line 595
    }

#line 594
    int3 _S50 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-4.38005590438842773f * _S32 - 6.23017597198486328f * _S33, -4.38005590438842773f * _S33 + 6.23017597198486328f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_17 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S50)).xy), uint(((_S50)).z)));
    if(depth_17 > reference_4)
    {

        float found_14 = found_0 + 1.0f;

#line 598
        sum_0 = sum_0 + depth_17;

#line 598
        found_0 = found_14;

#line 595
    }

#line 594
    int3 _S51 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(-1.01189601421356201f * _S32 - -7.80872011184692383f * _S33, -1.01189601421356201f * _S33 + -7.80872011184692383f * _S32) * _S28, _S29, _S34)) * _S31, _S35)), int(0));

#line 594
    float depth_18 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S51)).xy), uint(((_S51)).z)));
    if(depth_18 > reference_4)
    {
        float sum_1 = sum_0 + depth_18;

#line 597
        found_0 = found_0 + 1.0f;

#line 597
        sum_0 = sum_1;

#line 595
    }

#line 602
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 613
    float _S52 = 2.0f * kernelContext_13->frame_0->cascade_far_0[cascade_0];

#line 613
    float separation_0 = (sum_0 / found_0 - reference_4) * (_S52 + 40.0f);

#line 613
    float _S53 = tile_texels_0(_S27, kernelContext_13);

    return clamp(separation_0 * 0.01999999955296516f / (_S52 / _S53), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_0, float3 to_light_1, float3 geometric_normal_1, float2 pixel_5, KernelContext_0 thread* kernelContext_14)
{

#line 621
    float4 _S54 = atlas_rect_0(cascade_1, kernelContext_14);

#line 655
    if(atlas_rect_is_empty_0(_S54))
    {


        return 1.0f;
    }
    float _S55 = 2.0f * kernelContext_14->frame_0->cascade_far_0[cascade_1];

#line 661
    float _S56 = tile_texels_0(_S54, kernelContext_14);

#line 661
    float texel_world_0 = _S55 / _S56;

#line 668
    float4 clip_0 = (((float4(world_position_0 + geometric_normal_1 * float3((texel_world_0 * kernelContext_14->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_1)))  + to_light_1 * float3((texel_world_0 * kernelContext_14->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_2 = clip_0.xyz / float3(clip_0.w) ;

#line 672
    bool _S57;
    if(any((abs(ndc_2.xy)) > (float2(1.0f) )))
    {

#line 673
        _S57 = true;

#line 673
    }
    else
    {

#line 673
        _S57 = (ndc_2.z) <= 0.0f;

#line 673
    }

#line 673
    if(_S57)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_2.x * 0.5f + 0.5f, 0.5f - ndc_2.y * 0.5f);

#line 683
    uint _S58 = shadow_filter_mode_0(pixel_5, kernelContext_14);

#line 700
    if(_S58 == 2U)
    {

#line 700
        float _S59 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_2.z, kernelContext_14);

        return _S59;
    }
    if(_S58 == 1U)
    {

#line 704
        float _S60 = tile_pcf_0(cascade_1, tile_uv_6, ndc_2.z, pixel_5, 2.0f, kernelContext_14);



        return _S60;
    }

    float _S61 = ndc_2.z;

#line 711
    float _S62 = sun_penumbra_texels_0(cascade_1, tile_uv_6, _S61, shadow_rotation_0(pixel_5), kernelContext_14);

#line 711
    float _S63 = tile_pcf_0(cascade_1, tile_uv_6, _S61, pixel_5, _S62, kernelContext_14);
    return _S63;
}

float sun_visibility_0(float3 world_position_1, float3 to_light_2, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_6, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_15)
{
    uint cascade_2;

#line 717
    bool covered_0;

#line 726
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 738
    float eye_distance_0 = length(world_position_1 - kernelContext_15->frame_0->camera_position_0.xyz);

#line 738
    uint index_1 = 0U;

#line 746
    for(;;)
    {

#line 746
        if(index_1 < 2U)
        {
        }
        else
        {

#line 746
            covered_0 = false;

#line 746
            cascade_2 = 1U;

#line 746
            break;
        }
        if(eye_distance_0 < kernelContext_15->frame_0->cascade_far_0[index_1])
        {

#line 748
            covered_0 = true;

#line 748
            cascade_2 = index_1;



            break;
        }

#line 746
        index_1 = index_1 + 1U;

#line 746
    }

#line 755
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 755
    }

#line 755
    float _S64 = cascade_visibility_0(cascade_2, world_position_1, to_light_2, geometric_normal_2, pixel_6, kernelContext_15);

#line 762
    uint _S65 = cascade_2 + 1U;

#line 762
    if(_S65 >= 2U)
    {



        return _S64;
    }

#line 775
    float band_0 = kernelContext_15->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_15->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S64;
    }

#line 783
    float _S66 = cascade_visibility_0(_S65, world_position_1, to_light_2, geometric_normal_2, pixel_6, kernelContext_15);

#line 794
    return mix(_S64, _S66, blend_0);
}


#line 424
float3 sky_irradiance_0(float3 normal_1, KernelContext_0 thread* kernelContext_16)
{
    float4 basis_0 = float4(normal_1, 1.0f);
    return max(float3(dot(kernelContext_16->frame_0->sky_sh_r_0, basis_0), dot(kernelContext_16->frame_0->sky_sh_g_0, basis_0), dot(kernelContext_16->frame_0->sky_sh_b_0, basis_0)), float3(0.0f, 0.0f, 0.0f));
}


#line 1001
float probe_level_reach_0(float3 world_position_2, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 1001
    float reach_0 = 0.0f;

#line 1001
    uint axis_0 = 0U;


    for(;;)
    {

#line 1004
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 1004
            break;
        }

#line 1004
        uint _S67 = axis_0;

#line 1004
        bool _S68;

        if((last_0[axis_0]) == 0.0f)
        {

#line 1006
            _S68 = true;

#line 1006
        }
        else
        {

#line 1006
            _S68 = (inv_spacing_0[axis_0]) == 0.0f;

#line 1006
        }

#line 1006
        if(_S68)
        {

#line 1007
            axis_0 = axis_0 + 1U;

#line 1004
            continue;
        }

#line 1004
        reach_0 = max(reach_0, abs(2.0f * ((world_position_2[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S67] - 1.0f));

#line 1004
        axis_0 = axis_0 + 1U;

#line 1004
    }

#line 1011
    return reach_0;
}

float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1014
    uint level_0 = 0U;

    for(;;)
    {

#line 1016
        uint _S69 = level_0 + 1U;

#line 1016
        if(_S69 < levels_0)
        {
        }
        else
        {

#line 1016
            break;
        }
        float _S70 = float(level_0);

#line 1018
        float at_0 = reach_1 * exp2(- _S70);
        if(at_0 < 1.0f)
        {

#line 1020
            return float2(_S70, saturate((1.0f - at_0) / 0.25f));
        }

#line 1016
        level_0 = _S69;

#line 1016
    }

#line 1022
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 951
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_0)
{
    uint at_1 = cell_1 + offset_0;

#line 953
    uint _S71;
    if(at_1 >= count_0)
    {

#line 954
        _S71 = at_1 - count_0;

#line 954
    }
    else
    {

#line 954
        _S71 = at_1;

#line 954
    }

#line 954
    return _S71;
}

uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_17)
{
    uint3 counts_0 = kernelContext_17->camera_0->probe_counts_1.xyz;
    uint3 offset_1 = kernelContext_17->camera_0->probe_level_offset_1[level_1].xyz;
    uint _S72 = counts_0.x;
    uint _S73 = counts_0.y;



    return min(kernelContext_17->camera_0->probe_levels_1.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S73 + probe_wrap_0(cell_2.y, offset_1.y, _S73)) * _S72 + probe_wrap_0(cell_2.x, offset_1.x, _S72), max(kernelContext_17->camera_0->probe_counts_1.w, 1U) - 1U);
}


#line 892
float sign_not_zero_0(float value_0)
{

#line 892
    float _S74;

    if(value_0 >= 0.0f)
    {

#line 894
        _S74 = 1.0f;

#line 894
    }
    else
    {

#line 894
        _S74 = -1.0f;

#line 894
    }

#line 894
    return _S74;
}

float2 oct_encode_0(float3 direction_1)
{
    float _S75 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S75) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 900
    float2 p_1;
    if(_S75 < 0.0f)
    {
        float _S76 = p_0.y;

#line 903
        float _S77 = p_0.x;

#line 903
        p_1 = float2((1.0f - abs(_S76)) * sign_not_zero_0(_S77), (1.0f - abs(_S77)) * sign_not_zero_0(_S76));

#line 901
    }
    else
    {

#line 901
        p_1 = p_0;

#line 901
    }

#line 906
    return p_1;
}

float2 probe_moments_0(uint index_2, float3 direction_2, KernelContext_0 thread* kernelContext_18)
{

#line 909
    texture2d_array<float, access::sample> _S78 = kernelContext_18->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S78).get_width(0)),(*((&height_0)) = (_S78).get_height(0)),(*((&layers_0)) = (_S78).get_array_size());

#line 914
    float2 _S79 = float2(0.5f) ;

#line 914
    float2 _S80 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_2) * _S79 + _S79) * float2(16.0f)  + _S80 - _S79;
    float2 _S81 = float2(float(width_0), float(height_0)) - _S80;

#line 918
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S81);
    float2 high_0 = min(low_0 + _S80, _S81);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_2, max(layers_0, 1U) - 1U));

    int _S82 = int(low_0.x);

#line 923
    int _S83 = int(low_0.y);

#line 923
    int4 _S84 = int4(_S82, _S83, layer_0, int(0));
    int _S85 = int(high_0.x);

#line 924
    int4 _S86 = int4(_S85, _S83, layer_0, int(0));
    int _S87 = int(high_0.y);

#line 925
    int4 _S88 = int4(_S82, _S87, layer_0, int(0));
    int4 _S89 = int4(_S85, _S87, layer_0, int(0));
    float2 _S90 = float2(weight_0.x) ;

#line 927
    return mix(mix(((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S84)).xy), uint(((_S84)).z), uint(((_S84)).w))).xy, ((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S86)).xy), uint(((_S86)).z), uint(((_S86)).w))).xy, _S90), mix(((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S88)).xy), uint(((_S88)).z), uint(((_S88)).w))).xy, ((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S89)).xy), uint(((_S89)).z), uint(((_S89)).w))).xy, _S90), float2(weight_0.y) );
}

float probe_chebyshev_0(uint index_3, float3 probe_position_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_19)
{
    float3 to_probe_0 = probe_position_0 - (world_position_3 + normal_2 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 933
    float2 _S91 = probe_moments_0(index_3, - to_probe_0, kernelContext_19);

#line 939
    float _S92 = _S91.x;

#line 939
    float _S93 = max(_S91.y - _S92 * _S92, 0.0f);
    float behind_0 = to_surface_0 - _S92;
    float bound_0 = _S93 / (_S93 + behind_0 * behind_0);

#line 941
    float _S94;
    if(to_surface_0 <= _S92)
    {

#line 942
        _S94 = 1.0f;

#line 942
    }
    else
    {

#line 942
        _S94 = bound_0 * bound_0 * bound_0;

#line 942
    }

#line 942
    return _S94;
}

float probe_weight_0(uint index_4, float3 probe_position_1, float3 world_position_4, float3 normal_3, KernelContext_0 thread* kernelContext_20)
{

#line 945
    float _S95 = probe_chebyshev_0(index_4, probe_position_1, world_position_4, normal_3, kernelContext_20);

    return max(_S95, 0.00009999999747379f);
}


#line 100
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 969
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 985
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_5, float3 normal_4, KernelContext_0 thread* kernelContext_21)
{

#line 986
    uint _S96 = probe_row_0(level_2, cell_3, kernelContext_21);


    GpuProbe_natural_0 stored_0 = kernelContext_21->probes_0[_S96];

#line 989
    float _S97 = probe_weight_0(_S96, origin_1 + float3(cell_3) * spacing_0, world_position_5, normal_4, kernelContext_21);



    thread WeightedProbe_0 corner_0;

#line 993
    float4 _S98 = float4(_S97) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S98;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S98;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S98;
    (&corner_0)->weight_1 = _S97;
    return corner_0;
}


#line 975
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S99 = float4(t_0) ;

#line 978
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S99);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S99);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S99);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1025
float3 probe_level_environment_0(uint level_3, float3 world_position_6, float3 normal_5, float3 direction_3, KernelContext_0 thread* kernelContext_22)
{

#line 1025
    float3 _S100 = float3(1.0f) ;

    float3 _S101 = float3(0.0f, 0.0f, 0.0f);

#line 1027
    float3 last_1 = max(float3(kernelContext_22->camera_0->probe_counts_1.xyz) - _S100, _S101);



    float3 origin_2 = kernelContext_22->camera_0->probe_level_origin_1[level_3].xyz;
    float3 inv_0 = kernelContext_22->camera_0->probe_level_inv_spacing_1[level_3].xyz;
    float3 grid_0 = clamp((world_position_6 - origin_2) * inv_0, _S101, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S102 = uint3(base_0);
    uint3 _S103 = uint3(min(base_0 + _S100, last_1));

#line 1042
    float _S104 = inv_0.x;

#line 1042
    float _S105;

#line 1042
    if(_S104 != 0.0f)
    {

#line 1042
        _S105 = 1.0f / _S104;

#line 1042
    }
    else
    {

#line 1042
        _S105 = 0.0f;

#line 1042
    }
    float _S106 = inv_0.y;

#line 1043
    float _S107;

#line 1043
    if(_S106 != 0.0f)
    {

#line 1043
        _S107 = 1.0f / _S106;

#line 1043
    }
    else
    {

#line 1043
        _S107 = 0.0f;

#line 1043
    }
    float _S108 = inv_0.z;

#line 1044
    float _S109;

#line 1044
    if(_S108 != 0.0f)
    {

#line 1044
        _S109 = 1.0f / _S108;

#line 1044
    }
    else
    {

#line 1044
        _S109 = 0.0f;

#line 1044
    }

#line 1042
    float3 spacing_1 = float3(_S105, _S107, _S109);

#line 1051
    uint _S110 = _S102.x;

#line 1051
    uint _S111 = _S102.y;

#line 1051
    uint _S112 = _S102.z;

#line 1051
    WeightedProbe_0 _S113 = probe_corner_0(level_3, uint3(_S110, _S111, _S112), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);
    uint _S114 = _S103.x;

#line 1052
    WeightedProbe_0 _S115 = probe_corner_0(level_3, uint3(_S114, _S111, _S112), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1052
    float _S116 = f_0.x;

#line 1052
    thread WeightedProbe_0 _S117 = _S113;

#line 1052
    thread WeightedProbe_0 _S118 = _S115;

#line 1052
    WeightedProbe_0 _S119 = lerp_probe_0(&_S117, &_S118, _S116);
    uint _S120 = _S103.y;

#line 1053
    WeightedProbe_0 _S121 = probe_corner_0(level_3, uint3(_S110, _S120, _S112), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1053
    WeightedProbe_0 _S122 = probe_corner_0(level_3, uint3(_S114, _S120, _S112), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1053
    thread WeightedProbe_0 _S123 = _S121;

#line 1053
    thread WeightedProbe_0 _S124 = _S122;

#line 1053
    WeightedProbe_0 _S125 = lerp_probe_0(&_S123, &_S124, _S116);

    uint _S126 = _S103.z;

#line 1055
    WeightedProbe_0 _S127 = probe_corner_0(level_3, uint3(_S110, _S111, _S126), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1055
    WeightedProbe_0 _S128 = probe_corner_0(level_3, uint3(_S114, _S111, _S126), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1055
    thread WeightedProbe_0 _S129 = _S127;

#line 1055
    thread WeightedProbe_0 _S130 = _S128;

#line 1055
    WeightedProbe_0 _S131 = lerp_probe_0(&_S129, &_S130, _S116);

#line 1055
    WeightedProbe_0 _S132 = probe_corner_0(level_3, uint3(_S110, _S120, _S126), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1055
    WeightedProbe_0 _S133 = probe_corner_0(level_3, uint3(_S114, _S120, _S126), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1055
    thread WeightedProbe_0 _S134 = _S132;

#line 1055
    thread WeightedProbe_0 _S135 = _S133;

#line 1055
    WeightedProbe_0 _S136 = lerp_probe_0(&_S134, &_S135, _S116);



    float _S137 = f_0.y;

#line 1059
    thread WeightedProbe_0 _S138 = _S119;

#line 1059
    thread WeightedProbe_0 _S139 = _S125;

#line 1059
    WeightedProbe_0 _S140 = lerp_probe_0(&_S138, &_S139, _S137);

#line 1059
    thread WeightedProbe_0 _S141 = _S131;

#line 1059
    thread WeightedProbe_0 _S142 = _S136;

#line 1059
    WeightedProbe_0 _S143 = lerp_probe_0(&_S141, &_S142, _S137);

    float _S144 = f_0.z;

#line 1061
    thread WeightedProbe_0 _S145 = _S140;

#line 1061
    thread WeightedProbe_0 _S146 = _S143;

#line 1061
    WeightedProbe_0 _S147 = lerp_probe_0(&_S145, &_S146, _S144);

#line 1061
    float3 _S148 = float3(2.09439516067504883f) ;

#line 1067
    return max(float3(dot(_S147.sh_0.sh_r_0.xyz / _S148, direction_3) + _S147.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S147.sh_0.sh_g_0.xyz / _S148, direction_3) + _S147.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S147.sh_0.sh_b_0.xyz / _S148, direction_3) + _S147.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S147.weight_1) , _S101);
}

float3 probe_environment_0(float3 world_position_7, float3 normal_6, float3 direction_4, KernelContext_0 thread* kernelContext_23)
{
    if((kernelContext_23->camera_0->probe_counts_1.w) == 0U)
    {

#line 1073
        return float3(0.0f, 0.0f, 0.0f);
    }

#line 1081
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_7, kernelContext_23->camera_0->probe_level_origin_1[int(0)].xyz, kernelContext_23->camera_0->probe_level_inv_spacing_1[int(0)].xyz, max(float3(kernelContext_23->camera_0->probe_counts_1.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_23->camera_0->probe_levels_1.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1083
    float3 _S149 = probe_level_environment_0(level_4, world_position_7, normal_6, direction_4, kernelContext_23);


    if(share_0 >= 1.0f)
    {

#line 1087
        return _S149;
    }

#line 1087
    float3 _S150 = probe_level_environment_0(level_4 + 1U, world_position_7, normal_6, direction_4, kernelContext_23);

    return _S150 * float3((1.0f - share_0))  + _S149 * float3(share_0) ;
}


#line 797
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

#line 809
    float2 _S151 = float2(1.0f) ;
    float2 _S152 = extent_2 - _S151;

#line 810
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S152);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S153 = int2(low_1);
    int2 _S154 = int2(min(low_1 + _S151, _S152));
    int _S155 = _S153.x;

#line 816
    int _S156 = _S153.y;

#line 816
    int3 _S157 = int3(_S155, _S156, int(0));
    int _S158 = _S154.x;

#line 817
    int3 _S159 = int3(_S158, _S156, int(0));
    float2 _S160 = float2(weight_2.x) ;
    int _S161 = _S154.y;

#line 819
    int3 _S162 = int3(_S155, _S161, int(0));
    int3 _S163 = int3(_S158, _S161, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S157)).xy), uint(((_S157)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S159)).xy), uint(((_S159)).z)))), _S160), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S162)).xy), uint(((_S162)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S163)).xy), uint(((_S163)).z)))), _S160), float2(weight_2.y) );
}

float2 sky_prefilter_at_0(float up_0, float roughness_0, KernelContext_0 thread* kernelContext_24)
{
    return fixed_pair_at_0(kernelContext_24->sky_prefilter_0, float2(up_0, roughness_0));
}

float3 sky_prefiltered_0(float3 direction_5, float roughness_1, KernelContext_0 thread* kernelContext_25)
{
    float up_1 = clamp(direction_5.y, -1.0f, 1.0f);

#line 832
    float2 _S164 = sky_prefilter_at_0(abs(up_1), roughness_1, kernelContext_25);

    bool _S165 = up_1 >= 0.0f;

#line 834
    float3 far_0;

#line 834
    if(_S165)
    {

#line 834
        far_0 = kernelContext_25->camera_0->sky_0[int(0)].xyz;

#line 834
    }
    else
    {

#line 834
        far_0 = kernelContext_25->camera_0->sky_0[int(2)].xyz;

#line 834
    }

#line 834
    float3 opposite_0;
    if(_S165)
    {

#line 835
        opposite_0 = kernelContext_25->camera_0->sky_0[int(2)].xyz;

#line 835
    }
    else
    {

#line 835
        opposite_0 = kernelContext_25->camera_0->sky_0[int(0)].xyz;

#line 835
    }
    float _S166 = _S164.x;

#line 836
    float _S167 = _S164.y;
    return kernelContext_25->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S166 - _S167))  + far_0 * float3(_S166)  + opposite_0 * float3(_S167) ;
}

float3 sky_view_at_0(float up_2, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_26)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_1 = clamp(up_2, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_1));

#line 844
    float _S168;
    if(clamped_1 >= 0.0f)
    {

#line 845
        _S168 = root_0;

#line 845
    }
    else
    {

#line 845
        _S168 = - root_0;

#line 845
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S168, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S169 = uint(x0_0);

#line 859
    float3 _S170 = float3((1.0f - fx_0)) ;
    uint _S171 = uint(min(x0_0 + 1.0f, 95.0f));

#line 860
    float3 _S172 = float3(fx_0) ;


    return ((float4(*(kernelContext_26->sky_view_0+(row0_0 + _S169))) ).xyz * _S170 + (float4(*(kernelContext_26->sky_view_0+(row0_0 + _S171))) ).xyz * _S172) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_26->sky_view_0+(row1_0 + _S169))) ).xyz * _S170 + (float4(*(kernelContext_26->sky_view_0+(row1_0 + _S171))) ).xyz * _S172) * float3(fy_0) ;
}

float3 atmosphere_radiance_0(float3 direction_6, KernelContext_0 thread* kernelContext_27)
{
    float3 sun_0 = kernelContext_27->camera_0->atmosphere_0.xyz;
    float _S173 = direction_6.x;

#line 869
    float _S174 = direction_6.z;

#line 869
    float view_flat_0 = sqrt(_S173 * _S173 + _S174 * _S174);
    float _S175 = sun_0.x;

#line 870
    float _S176 = sun_0.z;

#line 870
    float sun_flat_0 = sqrt(_S175 * _S175 + _S176 * _S176);

#line 870
    bool _S177;

    if(view_flat_0 > 0.0f)
    {

#line 872
        _S177 = sun_flat_0 > 0.0f;

#line 872
    }
    else
    {

#line 872
        _S177 = false;

#line 872
    }

#line 872
    float cosine_1;

#line 872
    if(_S177)
    {

#line 872
        cosine_1 = (_S173 * _S175 + _S174 * _S176) / (view_flat_0 * sun_flat_0);

#line 872
    }
    else
    {

#line 872
        cosine_1 = 1.0f;

#line 872
    }

#line 872
    float3 _S178 = sky_view_at_0(direction_6.y, cosine_1, kernelContext_27);



    return _S178;
}

float3 sky_environment_0(float3 direction_7, float roughness_2, float share_1, KernelContext_0 thread* kernelContext_28)
{

#line 879
    float3 _S179 = sky_prefiltered_0(direction_7, roughness_2, kernelContext_28);


    if((kernelContext_28->camera_0->atmosphere_0.w) <= 0.0f)
    {
        return _S179;
    }



    float3 _S180 = _S179 * float3((1.0f - share_1)) ;

#line 889
    float3 _S181 = atmosphere_radiance_0(direction_7, kernelContext_28);

#line 889
    return _S180 + _S181 * float3(share_1) ;
}


#line 369
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S182 = - d_0;

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
            float _S183 = series_0 * _S182 + FOG_RATIO_KERNEL_0[term_2];

#line 375
            int term_3 = term_2 - int(1);

#line 375
            series_0 = _S183;

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
    float _S184 = a_1 * a_1;

#line 412
    float d_1 = 1.0f + _S184 - 2.0f * a_1 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S184) / (d_1 * sqrt(d_1));
}

float3 volumetric_source_0(float3 view_direction_0, float4 lit_0, KernelContext_0 thread* kernelContext_29)
{



    return kernelContext_29->params_0->fog_color_1.xyz + kernelContext_29->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_29->params_0->sun_direction_0.w, dot(kernelContext_29->params_0->sun_direction_0.xyz, view_direction_0)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 1094
struct WaterAir_0
{
    float survives_0;
    float3 inscatter_0;
};


#line 1108
WaterAir_0 water_froxel_air_0(int2 pixel_7, float depth_19, KernelContext_0 thread* kernelContext_30)
{
    thread WaterAir_0 air_0;
    (&air_0)->survives_0 = 1.0f;
    (&air_0)->inscatter_0 = float3(0.0f, 0.0f, 0.0f);

    uint _S185 = max(kernelContext_30->params_0->grid_x_0, 1U);
    uint _S186 = max(kernelContext_30->params_0->grid_y_0, 1U);
    uint _S187 = max(kernelContext_30->params_0->slices_0, 1U);
    uint tiles_0 = _S185 * _S186;
    uint _S188 = max(kernelContext_30->params_0->tile_pixels_0, 1U);


    int _S189 = pixel_7.x;
    int _S190 = pixel_7.y;

#line 1120
    float2 ndc_3 = float2((float(_S189) + 0.5f) / float(max(kernelContext_30->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S190) + 0.5f) / float(max(kernelContext_30->params_0->viewport_y_0, 1U)) * 2.0f);

#line 1120
    float view_depth_0;



    if(depth_19 > 0.0f)
    {

#line 1124
        float3 _S191 = volumetric_unproject_0(ndc_3, depth_19, kernelContext_30);

#line 1124
        view_depth_0 = dot(kernelContext_30->params_0->depth_row_0, float4(_S191, 1.0f));

#line 1124
    }
    else
    {

#line 1124
        view_depth_0 = 1000.0f;

#line 1124
    }

#line 1129
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 1129
    float slice_start_0 = 0.0f;

#line 1129
    uint slice_0 = 0U;

#line 1129
    float next_start_0 = 0.14677993953227997f;

#line 1147
    for(;;)
    {

#line 1147
        uint _S192 = slice_0 + 1U;

#line 1147
        bool _S193;

#line 1147
        if(_S192 < _S187)
        {

#line 1147
            _S193 = next_start_0 <= view_depth_1;

#line 1147
        }
        else
        {

#line 1147
            _S193 = false;

#line 1147
        }

#line 1147
        if(_S193)
        {
        }
        else
        {

#line 1147
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 1150
        slice_start_0 = next_start_0;

#line 1150
        next_start_0 = next_start_1;

#line 1150
        slice_0 = _S192;

#line 1147
    }

#line 1157
    uint _S194 = uint(max(_S189, int(0))) / _S188;

#line 1157
    uint _S195 = min(_S194, _S185 - 1U);
    uint _S196 = uint(max(_S190, int(0))) / _S188;
    uint froxel_0 = _S195 + min(_S196, _S186 - 1U) * _S185 + slice_0 * tiles_0;
    if(froxel_0 >= (kernelContext_30->params_0->froxel_count_0))
    {
        return air_0;
    }

#line 1162
    float4 _S197 = float4(*(kernelContext_30->volumetrics_0+froxel_0)) ;

#line 1162
    float3 _S198 = volumetric_unproject_0(ndc_3, 1.0f, kernelContext_30);

#line 1171
    float3 along_0 = (_S198 - kernelContext_30->params_0->eye_0.xyz) / float3(max(dot(kernelContext_30->params_0->depth_row_0, float4(_S198, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = kernelContext_30->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = kernelContext_30->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;
    float reference_5 = kernelContext_30->params_0->fog_params_1.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_30->params_0->fog_params_1.x, kernelContext_30->params_0->fog_params_1.y, from_0.y - reference_5, to_0.y - reference_5, length_of_0));

#line 1179
    float3 view_direction_1;

#line 1185
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 1185
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 1185
    }
    else
    {

#line 1185
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 1185
    }

#line 1185
    float3 _S199 = volumetric_source_0(view_direction_1, float4(*(kernelContext_30->lighting_0+froxel_0)) , kernelContext_30);

#line 1192
    float3 partial_radiance_0 = _S199 * float3((1.0f - partial_survives_0)) ;

    float _S200 = _S197.w;

#line 1194
    (&air_0)->survives_0 = _S200 * partial_survives_0;
    (&air_0)->inscatter_0 = _S197.xyz + float3(_S200)  * partial_radiance_0;
    return air_0;
}


#line 398
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 1201
WaterAir_0 water_analytic_air_0(float3 surface_1, KernelContext_0 thread* kernelContext_31)
{

#line 1208
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0(kernelContext_31->frame_0->fog_params_0.x, kernelContext_31->frame_0->fog_params_0.y, kernelContext_31->frame_0->camera_position_0.y - kernelContext_31->frame_0->fog_params_0.z, surface_1.y - kernelContext_31->frame_0->fog_params_0.z, length(kernelContext_31->frame_0->camera_position_0.xyz - surface_1)));

    thread WaterAir_0 air_1;
    (&air_1)->survives_0 = fog_survives_0;
    (&air_1)->inscatter_0 = kernelContext_31->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;
    return air_1;
}


#line 1213
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1213
struct pixelInput_0
{
    float3 world_position_8 [[user(TEXCOORD)]];
    [[flat]] uint body_1 [[user(TEXCOORD_1)]];
};


#line 1274
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S201 [[stage_in]], float4 position_1 [[position]], WaterVertex_natural_0 device* water_vertices_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(1)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], WaterMedium_natural_0 device* media_1 [[buffer(5)]], texture2d<float, access::sample> scene_color_1 [[texture(0)]], VolumetricParams_natural_0 constant* params_1 [[buffer(3)]], WaterParams_0 constant* water_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(2)]], sampler shadow_sampler_1 [[sampler(0)]], SsrParams_natural_0 constant* camera_1 [[buffer(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(3)]], packed_float4 device* sky_view_1 [[buffer(7)]], packed_float4 device* volumetrics_1 [[buffer(8)]], packed_float4 device* lighting_1 [[buffer(9)]])
{

#line 1274
    thread KernelContext_0 kernelContext_32;

#line 1274
    (&kernelContext_32)->water_vertices_0 = water_vertices_1;

#line 1274
    (&kernelContext_32)->frame_0 = frame_1;

#line 1274
    (&kernelContext_32)->scene_depth_0 = scene_depth_1;

#line 1274
    (&kernelContext_32)->media_0 = media_1;

#line 1274
    (&kernelContext_32)->scene_color_0 = scene_color_1;

#line 1274
    (&kernelContext_32)->params_0 = params_1;

#line 1274
    (&kernelContext_32)->water_0 = water_1;

#line 1274
    (&kernelContext_32)->shadow_atlas_0 = shadow_atlas_1;

#line 1274
    (&kernelContext_32)->shadow_sampler_0 = shadow_sampler_1;

#line 1274
    (&kernelContext_32)->camera_0 = camera_1;

#line 1274
    (&kernelContext_32)->probes_0 = probes_1;

#line 1274
    (&kernelContext_32)->probe_visibility_0 = probe_visibility_1;

#line 1274
    (&kernelContext_32)->sky_prefilter_0 = sky_prefilter_1;

#line 1274
    (&kernelContext_32)->sky_view_0 = sky_view_1;

#line 1274
    (&kernelContext_32)->volumetrics_0 = volumetrics_1;

#line 1274
    (&kernelContext_32)->lighting_0 = lighting_1;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    float2 extent_3 = float2(float(width_2), float(height_2));
    int2 last_2 = int2(int(width_2) - int(1), int(height_2) - int(1));
    float2 _S202 = position_1.xy;

#line 1281
    int2 _S203 = int2(_S202);
    float surface_depth_0 = position_1.z;

    WaterMedium_natural_0 medium_0 = media_1[_S201.body_1];

    float3 up_3 = float3(0.0f, 1.0f, 0.0f);


    float3 incident_1 = normalize(_S201.world_position_8 - frame_1->camera_position_0.xyz);
    float _S204 = - incident_1.y;

#line 1290
    float cos_incident_1 = saturate(_S204);

#line 1297
    int3 _S205 = int3(_S203, int(0));

#line 1297
    float4 straight_0 = ((scene_color_1).read(vec<uint,2>(((_S205)).xy), uint(((_S205)).z)));

#line 1297
    float3 _S206 = water_opaque_point_0(_S203, ((scene_depth_1).read(vec<uint,2>(((_S205)).xy), uint(((_S205)).z))), extent_3, _S201.world_position_8, incident_1, &kernelContext_32);

#line 1307
    float3 refracted_0 = water_refract_0(incident_1, up_3, 0.75f);

    float4 landing_clip_0 = (((float4(_S201.world_position_8 + refracted_0 * float3((max(_S201.world_position_8.y - _S206.y, 0.0f) / max(- refracted_0.y, 9.99999997475242708e-07f))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));



    float _S207 = landing_clip_0.w;

#line 1313
    float3 behind_point_0;

#line 1313
    float4 behind_1;

#line 1313
    if(_S207 > 0.0f)
    {
        float2 landing_ndc_0 = landing_clip_0.xy / float2(_S207) ;
        int2 bent_0 = clamp(int2(float2(landing_ndc_0.x * 0.5f + 0.5f, 0.5f - landing_ndc_0.y * 0.5f) * extent_3), int2(int(0), int(0)), last_2);


        int3 _S208 = int3(bent_0, int(0));

#line 1319
        float bent_depth_0 = (((&kernelContext_32)->scene_depth_0).read(vec<uint,2>(((_S208)).xy), uint(((_S208)).z)));

#line 1325
        if(bent_depth_0 < surface_depth_0)
        {
            float4 _S209 = (((&kernelContext_32)->scene_color_0).read(vec<uint,2>(((_S208)).xy), uint(((_S208)).z)));

#line 1327
            float3 _S210 = water_opaque_point_0(bent_0, bent_depth_0, extent_3, _S201.world_position_8, refracted_0, &kernelContext_32);

#line 1327
            behind_point_0 = _S210;

#line 1327
            behind_1 = _S209;

#line 1325
        }
        else
        {

#line 1325
            behind_point_0 = _S206;

#line 1325
            behind_1 = straight_0;

#line 1325
        }

#line 1313
    }
    else
    {

#line 1313
        behind_point_0 = _S206;

#line 1313
        behind_1 = straight_0;

#line 1313
    }

#line 1333
    float thickness_0 = length(behind_point_0 - _S201.world_position_8);

#line 1333
    float4 _S211 = float4(medium_0.scattering_0) ;
    float3 extinction_0 = (float4(medium_0.absorption_0) ).xyz + _S211.xyz;
    float _S212 = extinction_0.x;
    float _S213 = extinction_0.y;
    float _S214 = extinction_0.z;

#line 1335
    float3 transmittance_0 = float3(fog_exp_neg_0(_S212 * thickness_0), fog_exp_neg_0(_S213 * thickness_0), fog_exp_neg_0(_S214 * thickness_0));

#line 1335
    float _S215;

#line 1343
    if(_S212 > 0.0f)
    {

#line 1343
        _S215 = _S211.x / _S212;

#line 1343
    }
    else
    {

#line 1343
        _S215 = 0.0f;

#line 1343
    }

#line 1343
    float _S216;
    if(_S213 > 0.0f)
    {

#line 1344
        _S216 = _S211.y / _S213;

#line 1344
    }
    else
    {

#line 1344
        _S216 = 0.0f;

#line 1344
    }

#line 1344
    float _S217;
    if(_S214 > 0.0f)
    {

#line 1345
        _S217 = _S211.z / _S214;

#line 1345
    }
    else
    {

#line 1345
        _S217 = 0.0f;

#line 1345
    }

#line 1343
    float3 albedo_0 = float3(_S215, _S216, _S217);


    float3 to_sun_0 = (&kernelContext_32)->water_0->sun_direction_1.xyz;


    float _S218 = to_sun_0.y;

#line 1347
    thread uint cascade_3;
    thread float cascade_fade_0;

#line 1348
    float _S219 = sun_visibility_0(_S201.world_position_8, to_sun_0, _S218, up_3, _S202, &cascade_3, &cascade_fade_0, &kernelContext_32);


    float3 _S220 = (&kernelContext_32)->frame_0->ambient_0.xyz;

#line 1351
    float3 _S221 = sky_irradiance_0(up_3, &kernelContext_32);

#line 1351
    float3 _S222 = float3(1.0f) ;

    float3 scattered_0 = albedo_0 * (_S220 + _S221 + (&kernelContext_32)->water_0->sun_color_0.xyz * float3((max(_S218, 0.0f) * _S219)) ) * (_S222 - transmittance_0);

#line 1360
    float3 mirrored_0 = float3(incident_1.x, _S204, incident_1.z);

#line 1360
    float3 _S223 = probe_environment_0(_S201.world_position_8, up_3, mirrored_0, &kernelContext_32);

#line 1360
    float3 _S224 = sky_environment_0(mirrored_0, 0.0f, 1.0f, &kernelContext_32);

    float3 environment_0 = _S223 + _S224;
    float grazing_0 = 1.0f - cos_incident_1;
    float grazing2_0 = grazing_0 * grazing_0;
    float fresnel_0 = 0.01999999955296516f + 0.98000001907348633f * (grazing2_0 * grazing2_0 * grazing_0);

#line 1365
    WaterAir_0 air_2;

#line 1376
    if(((&kernelContext_32)->water_0->froxels_0.x) != 0U)
    {

#line 1376
        WaterAir_0 _S225 = water_froxel_air_0(_S203, surface_depth_0, &kernelContext_32);

#line 1376
        air_2 = _S225;

#line 1376
    }
    else
    {

#line 1376
        WaterAir_0 _S226 = water_analytic_air_0(_S201.world_position_8, &kernelContext_32);

#line 1376
        air_2 = _S226;

#line 1376
    }

#line 1376
    float3 _S227 = float3((1.0f - fresnel_0)) ;

#line 1393
    float fade_1 = saturate(thickness_0 / 0.25f);

#line 1393
    pixelOutput_0 _S228 = { float4(straight_0.xyz * float3((1.0f - fade_1))  + (behind_1.xyz * transmittance_0 * _S227 + (environment_0 * float3(fresnel_0)  + scattered_0 * _S227) * float3(air_2.survives_0)  + air_2.inscatter_0 * (_S222 - transmittance_0 * _S227)) * float3(fade_1) , straight_0.w) };
    return _S228;
}


#line 1394
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_9 [[user(TEXCOORD)]];
    uint body_2 [[user(TEXCOORD_1)]];
};


#line 1250
struct SurfaceOutput_0
{
    float4 position_3;
    float3 world_position_10;
    [[flat]] uint body_3;
};


#line 1250
[[vertex]] vertexMain_Result_0 vertexMain(uint index_5 [[vertex_id]], WaterVertex_natural_0 device* water_vertices_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(1)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], WaterMedium_natural_0 device* media_2 [[buffer(5)]], texture2d<float, access::sample> scene_color_2 [[texture(0)]], VolumetricParams_natural_0 constant* params_2 [[buffer(3)]], WaterParams_0 constant* water_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(2)]], sampler shadow_sampler_2 [[sampler(0)]], SsrParams_natural_0 constant* camera_2 [[buffer(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(3)]], packed_float4 device* sky_view_2 [[buffer(7)]], packed_float4 device* volumetrics_2 [[buffer(8)]], packed_float4 device* lighting_2 [[buffer(9)]])
{

#line 1250
    thread KernelContext_0 kernelContext_33;

#line 1250
    (&kernelContext_33)->water_vertices_0 = water_vertices_2;

#line 1250
    (&kernelContext_33)->frame_0 = frame_2;

#line 1250
    (&kernelContext_33)->scene_depth_0 = scene_depth_2;

#line 1250
    (&kernelContext_33)->media_0 = media_2;

#line 1250
    (&kernelContext_33)->scene_color_0 = scene_color_2;

#line 1250
    (&kernelContext_33)->params_0 = params_2;

#line 1250
    (&kernelContext_33)->water_0 = water_2;

#line 1250
    (&kernelContext_33)->shadow_atlas_0 = shadow_atlas_2;

#line 1250
    (&kernelContext_33)->shadow_sampler_0 = shadow_sampler_2;

#line 1250
    (&kernelContext_33)->camera_0 = camera_2;

#line 1250
    (&kernelContext_33)->probes_0 = probes_2;

#line 1250
    (&kernelContext_33)->probe_visibility_0 = probe_visibility_2;

#line 1250
    (&kernelContext_33)->sky_prefilter_0 = sky_prefilter_2;

#line 1250
    (&kernelContext_33)->sky_view_0 = sky_view_2;

#line 1250
    (&kernelContext_33)->volumetrics_0 = volumetrics_2;

#line 1250
    (&kernelContext_33)->lighting_0 = lighting_2;

#line 1265
    WaterVertex_natural_0 vertex_0 = water_vertices_2[index_5];
    thread SurfaceOutput_0 output_1;
    float3 _S229 = (float4(vertex_0.position_0) ).xyz;

#line 1267
    (&output_1)->world_position_10 = _S229;
    (&output_1)->position_3 = (((float4(_S229, 1.0f)) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_0[int(0)][int(0)], frame_2->view_proj_0.data_0[int(1)][int(0)], frame_2->view_proj_0.data_0[int(2)][int(0)], frame_2->view_proj_0.data_0[int(3)][int(0)], frame_2->view_proj_0.data_0[int(0)][int(1)], frame_2->view_proj_0.data_0[int(1)][int(1)], frame_2->view_proj_0.data_0[int(2)][int(1)], frame_2->view_proj_0.data_0[int(3)][int(1)], frame_2->view_proj_0.data_0[int(0)][int(2)], frame_2->view_proj_0.data_0[int(1)][int(2)], frame_2->view_proj_0.data_0[int(2)][int(2)], frame_2->view_proj_0.data_0[int(3)][int(2)], frame_2->view_proj_0.data_0[int(0)][int(3)], frame_2->view_proj_0.data_0[int(1)][int(3)], frame_2->view_proj_0.data_0[int(2)][int(3)], frame_2->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_1)->body_3 = (uint4(vertex_0.body_0) ).x;

#line 1269
    thread vertexMain_Result_0 _S230;

#line 1269
    (&_S230)->position_2 = output_1.position_3;

#line 1269
    (&_S230)->world_position_9 = output_1.world_position_10;

#line 1269
    (&_S230)->body_2 = output_1.body_3;

#line 1269
    return _S230;
}

