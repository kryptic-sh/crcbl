#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 260 "shaders/water.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 313
constant array<float2, int(16)> SHADOW_SEARCH_OFFSET_0 = { float2(1.41421604156494141f, 0.0f), float2(-1.80617594718933105f, 1.65460801124572754f), float2(0.27646398544311523f, -3.15016794204711914f), float2(2.27656793594360352f, 2.96938395500183105f), float2(-4.17778396606445312f, -0.73899197578430176f), float2(3.95756006240844727f, -2.51747989654541016f), float2(-1.32372796535491943f, 4.92420005798339844f), float2(-2.52448797225952148f, -4.86075210571289062f), float2(5.47713613510131836f, 2.00024008750915527f), float2(-5.69804811477661133f, 2.35207200050354004f), float2(2.74683189392089844f, -5.86983203887939453f), float2(2.02983999252319336f, 6.47145605087280273f), float2(-6.11796808242797852f, -3.54548811912536621f), float2(7.17707204818725586f, -1.57785594463348389f), float2(-4.38005590438842773f, 6.23017597198486328f), float2(-1.01189601421356201f, -7.80872011184692383f) };

#line 276
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 295
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 306
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 257
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1228
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


#line 1237
float3 water_opaque_point_0(int2 pixel_1, float depth_2, float2 extent_1, float3 surface_0, float3 direction_0, KernelContext_0 thread* kernelContext_2)
{

    if(depth_2 <= 0.0f)
    {
        return surface_0 + direction_0 * float3(1000.0f) ;
    }

#line 1242
    float3 _S1 = volumetric_unproject_1(water_ndc_0(pixel_1, extent_1), depth_2, kernelContext_2);

    return _S1;
}


#line 1218
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

#line 576
    uint index_1 = 0U;

#line 576
    float sum_0 = 0.0f;

#line 576
    float found_0 = 0.0f;



    for(;;)
    {

#line 580
        if(index_1 < 16U)
        {
        }
        else
        {

#line 580
            break;
        }

        float _S32 = SHADOW_SEARCH_OFFSET_0[index_1].x;

#line 583
        float _S33 = rotation_3.x;

#line 583
        float _S34 = SHADOW_SEARCH_OFFSET_0[index_1].y;

#line 583
        float _S35 = rotation_3.y;

#line 591
        int3 _S36 = int3(int2(min(atlas_uv_0(_S27, clamp(tile_uv_5 + float2(_S32 * _S33 - _S34 * _S35, _S32 * _S35 + _S34 * _S33) * _S28, _S29, float2(1.0f)  - _S29)) * _S31, _S31 - _S30)), int(0));

#line 591
        float depth_3 = ((kernelContext_13->shadow_atlas_0).read(vec<uint,2>(((_S36)).xy), uint(((_S36)).z)));
        if(depth_3 > reference_4)
        {

            float found_1 = found_0 + 1.0f;

#line 595
            sum_0 = sum_0 + depth_3;

#line 595
            found_0 = found_1;

#line 592
        }

#line 580
        index_1 = index_1 + 1U;

#line 580
    }

#line 599
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 610
    float _S37 = 2.0f * kernelContext_13->frame_0->cascade_far_0[cascade_0];

#line 610
    float separation_0 = (sum_0 / found_0 - reference_4) * (_S37 + 40.0f);

#line 610
    float _S38 = tile_texels_0(_S27, kernelContext_13);

    return clamp(separation_0 * 0.01999999955296516f / (_S37 / _S38), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_0, float3 to_light_1, float3 geometric_normal_1, float2 pixel_5, KernelContext_0 thread* kernelContext_14)
{

#line 618
    float4 _S39 = atlas_rect_0(cascade_1, kernelContext_14);

#line 652
    if(atlas_rect_is_empty_0(_S39))
    {


        return 1.0f;
    }
    float _S40 = 2.0f * kernelContext_14->frame_0->cascade_far_0[cascade_1];

#line 658
    float _S41 = tile_texels_0(_S39, kernelContext_14);

#line 658
    float texel_world_0 = _S40 / _S41;

#line 665
    float4 clip_0 = (((float4(world_position_0 + geometric_normal_1 * float3((texel_world_0 * kernelContext_14->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_1)))  + to_light_1 * float3((texel_world_0 * kernelContext_14->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_2 = clip_0.xyz / float3(clip_0.w) ;

#line 669
    bool _S42;
    if(any((abs(ndc_2.xy)) > (float2(1.0f) )))
    {

#line 670
        _S42 = true;

#line 670
    }
    else
    {

#line 670
        _S42 = (ndc_2.z) <= 0.0f;

#line 670
    }

#line 670
    if(_S42)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_2.x * 0.5f + 0.5f, 0.5f - ndc_2.y * 0.5f);

#line 680
    uint _S43 = shadow_filter_mode_0(pixel_5, kernelContext_14);

#line 697
    if(_S43 == 2U)
    {

#line 697
        float _S44 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_2.z, kernelContext_14);

        return _S44;
    }
    if(_S43 == 1U)
    {

#line 701
        float _S45 = tile_pcf_0(cascade_1, tile_uv_6, ndc_2.z, pixel_5, 2.0f, kernelContext_14);



        return _S45;
    }

    float _S46 = ndc_2.z;

#line 708
    float _S47 = sun_penumbra_texels_0(cascade_1, tile_uv_6, _S46, shadow_rotation_0(pixel_5), kernelContext_14);

#line 708
    float _S48 = tile_pcf_0(cascade_1, tile_uv_6, _S46, pixel_5, _S47, kernelContext_14);
    return _S48;
}

float sun_visibility_0(float3 world_position_1, float3 to_light_2, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_6, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_15)
{
    uint cascade_2;

#line 714
    bool covered_0;

#line 723
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 735
    float eye_distance_0 = length(world_position_1 - kernelContext_15->frame_0->camera_position_0.xyz);

#line 735
    uint index_2 = 0U;

#line 743
    for(;;)
    {

#line 743
        if(index_2 < 2U)
        {
        }
        else
        {

#line 743
            covered_0 = false;

#line 743
            cascade_2 = 1U;

#line 743
            break;
        }
        if(eye_distance_0 < kernelContext_15->frame_0->cascade_far_0[index_2])
        {

#line 745
            covered_0 = true;

#line 745
            cascade_2 = index_2;



            break;
        }

#line 743
        index_2 = index_2 + 1U;

#line 743
    }

#line 752
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 752
    }

#line 752
    float _S49 = cascade_visibility_0(cascade_2, world_position_1, to_light_2, geometric_normal_2, pixel_6, kernelContext_15);

#line 759
    uint _S50 = cascade_2 + 1U;

#line 759
    if(_S50 >= 2U)
    {



        return _S49;
    }

#line 772
    float band_0 = kernelContext_15->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_15->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S49;
    }

#line 780
    float _S51 = cascade_visibility_0(_S50, world_position_1, to_light_2, geometric_normal_2, pixel_6, kernelContext_15);

#line 791
    return mix(_S49, _S51, blend_0);
}


#line 424
float3 sky_irradiance_0(float3 normal_1, KernelContext_0 thread* kernelContext_16)
{
    float4 basis_0 = float4(normal_1, 1.0f);
    return max(float3(dot(kernelContext_16->frame_0->sky_sh_r_0, basis_0), dot(kernelContext_16->frame_0->sky_sh_g_0, basis_0), dot(kernelContext_16->frame_0->sky_sh_b_0, basis_0)), float3(0.0f, 0.0f, 0.0f));
}


#line 998
float probe_level_reach_0(float3 world_position_2, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 998
    float reach_0 = 0.0f;

#line 998
    uint axis_0 = 0U;


    for(;;)
    {

#line 1001
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 1001
            break;
        }

#line 1001
        uint _S52 = axis_0;

#line 1001
        bool _S53;

        if((last_0[axis_0]) == 0.0f)
        {

#line 1003
            _S53 = true;

#line 1003
        }
        else
        {

#line 1003
            _S53 = (inv_spacing_0[axis_0]) == 0.0f;

#line 1003
        }

#line 1003
        if(_S53)
        {

#line 1004
            axis_0 = axis_0 + 1U;

#line 1001
            continue;
        }

#line 1001
        reach_0 = max(reach_0, abs(2.0f * ((world_position_2[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S52] - 1.0f));

#line 1001
        axis_0 = axis_0 + 1U;

#line 1001
    }

#line 1008
    return reach_0;
}

float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1011
    uint level_0 = 0U;

    for(;;)
    {

#line 1013
        uint _S54 = level_0 + 1U;

#line 1013
        if(_S54 < levels_0)
        {
        }
        else
        {

#line 1013
            break;
        }
        float _S55 = float(level_0);

#line 1015
        float at_0 = reach_1 * exp2(- _S55);
        if(at_0 < 1.0f)
        {

#line 1017
            return float2(_S55, saturate((1.0f - at_0) / 0.25f));
        }

#line 1013
        level_0 = _S54;

#line 1013
    }

#line 1019
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 948
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_0)
{
    uint at_1 = cell_1 + offset_0;

#line 950
    uint _S56;
    if(at_1 >= count_0)
    {

#line 951
        _S56 = at_1 - count_0;

#line 951
    }
    else
    {

#line 951
        _S56 = at_1;

#line 951
    }

#line 951
    return _S56;
}

uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_17)
{
    uint3 counts_0 = kernelContext_17->camera_0->probe_counts_1.xyz;
    uint3 offset_1 = kernelContext_17->camera_0->probe_level_offset_1[level_1].xyz;
    uint _S57 = counts_0.x;
    uint _S58 = counts_0.y;



    return min(kernelContext_17->camera_0->probe_levels_1.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S58 + probe_wrap_0(cell_2.y, offset_1.y, _S58)) * _S57 + probe_wrap_0(cell_2.x, offset_1.x, _S57), max(kernelContext_17->camera_0->probe_counts_1.w, 1U) - 1U);
}


#line 889
float sign_not_zero_0(float value_0)
{

#line 889
    float _S59;

    if(value_0 >= 0.0f)
    {

#line 891
        _S59 = 1.0f;

#line 891
    }
    else
    {

#line 891
        _S59 = -1.0f;

#line 891
    }

#line 891
    return _S59;
}

float2 oct_encode_0(float3 direction_1)
{
    float _S60 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S60) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 897
    float2 p_1;
    if(_S60 < 0.0f)
    {
        float _S61 = p_0.y;

#line 900
        float _S62 = p_0.x;

#line 900
        p_1 = float2((1.0f - abs(_S61)) * sign_not_zero_0(_S62), (1.0f - abs(_S62)) * sign_not_zero_0(_S61));

#line 898
    }
    else
    {

#line 898
        p_1 = p_0;

#line 898
    }

#line 903
    return p_1;
}

float2 probe_moments_0(uint index_3, float3 direction_2, KernelContext_0 thread* kernelContext_18)
{

#line 906
    texture2d_array<float, access::sample> _S63 = kernelContext_18->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S63).get_width(0)),(*((&height_0)) = (_S63).get_height(0)),(*((&layers_0)) = (_S63).get_array_size());

#line 911
    float2 _S64 = float2(0.5f) ;

#line 911
    float2 _S65 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_2) * _S64 + _S64) * float2(16.0f)  + _S65 - _S64;
    float2 _S66 = float2(float(width_0), float(height_0)) - _S65;

#line 915
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S66);
    float2 high_0 = min(low_0 + _S65, _S66);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_3, max(layers_0, 1U) - 1U));

    int _S67 = int(low_0.x);

#line 920
    int _S68 = int(low_0.y);

#line 920
    int4 _S69 = int4(_S67, _S68, layer_0, int(0));
    int _S70 = int(high_0.x);

#line 921
    int4 _S71 = int4(_S70, _S68, layer_0, int(0));
    int _S72 = int(high_0.y);

#line 922
    int4 _S73 = int4(_S67, _S72, layer_0, int(0));
    int4 _S74 = int4(_S70, _S72, layer_0, int(0));
    float2 _S75 = float2(weight_0.x) ;

#line 924
    return mix(mix(((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S69)).xy), uint(((_S69)).z), uint(((_S69)).w))).xy, ((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S71)).xy), uint(((_S71)).z), uint(((_S71)).w))).xy, _S75), mix(((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S73)).xy), uint(((_S73)).z), uint(((_S73)).w))).xy, ((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S74)).xy), uint(((_S74)).z), uint(((_S74)).w))).xy, _S75), float2(weight_0.y) );
}

float probe_chebyshev_0(uint index_4, float3 probe_position_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_19)
{
    float3 to_probe_0 = probe_position_0 - (world_position_3 + normal_2 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 930
    float2 _S76 = probe_moments_0(index_4, - to_probe_0, kernelContext_19);

#line 936
    float _S77 = _S76.x;

#line 936
    float _S78 = max(_S76.y - _S77 * _S77, 0.0f);
    float behind_0 = to_surface_0 - _S77;
    float bound_0 = _S78 / (_S78 + behind_0 * behind_0);

#line 938
    float _S79;
    if(to_surface_0 <= _S77)
    {

#line 939
        _S79 = 1.0f;

#line 939
    }
    else
    {

#line 939
        _S79 = bound_0 * bound_0 * bound_0;

#line 939
    }

#line 939
    return _S79;
}

float probe_weight_0(uint index_5, float3 probe_position_1, float3 world_position_4, float3 normal_3, KernelContext_0 thread* kernelContext_20)
{

#line 942
    float _S80 = probe_chebyshev_0(index_5, probe_position_1, world_position_4, normal_3, kernelContext_20);

    return max(_S80, 0.00009999999747379f);
}


#line 100
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 966
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 982
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_5, float3 normal_4, KernelContext_0 thread* kernelContext_21)
{

#line 983
    uint _S81 = probe_row_0(level_2, cell_3, kernelContext_21);


    GpuProbe_natural_0 stored_0 = kernelContext_21->probes_0[_S81];

#line 986
    float _S82 = probe_weight_0(_S81, origin_1 + float3(cell_3) * spacing_0, world_position_5, normal_4, kernelContext_21);



    thread WeightedProbe_0 corner_0;

#line 990
    float4 _S83 = float4(_S82) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S83;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S83;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S83;
    (&corner_0)->weight_1 = _S82;
    return corner_0;
}


#line 972
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S84 = float4(t_0) ;

#line 975
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S84);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S84);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S84);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1022
float3 probe_level_environment_0(uint level_3, float3 world_position_6, float3 normal_5, float3 direction_3, KernelContext_0 thread* kernelContext_22)
{

#line 1022
    float3 _S85 = float3(1.0f) ;

    float3 _S86 = float3(0.0f, 0.0f, 0.0f);

#line 1024
    float3 last_1 = max(float3(kernelContext_22->camera_0->probe_counts_1.xyz) - _S85, _S86);



    float3 origin_2 = kernelContext_22->camera_0->probe_level_origin_1[level_3].xyz;
    float3 inv_0 = kernelContext_22->camera_0->probe_level_inv_spacing_1[level_3].xyz;
    float3 grid_0 = clamp((world_position_6 - origin_2) * inv_0, _S86, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S87 = uint3(base_0);
    uint3 _S88 = uint3(min(base_0 + _S85, last_1));

#line 1039
    float _S89 = inv_0.x;

#line 1039
    float _S90;

#line 1039
    if(_S89 != 0.0f)
    {

#line 1039
        _S90 = 1.0f / _S89;

#line 1039
    }
    else
    {

#line 1039
        _S90 = 0.0f;

#line 1039
    }
    float _S91 = inv_0.y;

#line 1040
    float _S92;

#line 1040
    if(_S91 != 0.0f)
    {

#line 1040
        _S92 = 1.0f / _S91;

#line 1040
    }
    else
    {

#line 1040
        _S92 = 0.0f;

#line 1040
    }
    float _S93 = inv_0.z;

#line 1041
    float _S94;

#line 1041
    if(_S93 != 0.0f)
    {

#line 1041
        _S94 = 1.0f / _S93;

#line 1041
    }
    else
    {

#line 1041
        _S94 = 0.0f;

#line 1041
    }

#line 1039
    float3 spacing_1 = float3(_S90, _S92, _S94);

#line 1048
    uint _S95 = _S87.x;

#line 1048
    uint _S96 = _S87.y;

#line 1048
    uint _S97 = _S87.z;

#line 1048
    WeightedProbe_0 _S98 = probe_corner_0(level_3, uint3(_S95, _S96, _S97), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);
    uint _S99 = _S88.x;

#line 1049
    WeightedProbe_0 _S100 = probe_corner_0(level_3, uint3(_S99, _S96, _S97), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1049
    float _S101 = f_0.x;

#line 1049
    thread WeightedProbe_0 _S102 = _S98;

#line 1049
    thread WeightedProbe_0 _S103 = _S100;

#line 1049
    WeightedProbe_0 _S104 = lerp_probe_0(&_S102, &_S103, _S101);
    uint _S105 = _S88.y;

#line 1050
    WeightedProbe_0 _S106 = probe_corner_0(level_3, uint3(_S95, _S105, _S97), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1050
    WeightedProbe_0 _S107 = probe_corner_0(level_3, uint3(_S99, _S105, _S97), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1050
    thread WeightedProbe_0 _S108 = _S106;

#line 1050
    thread WeightedProbe_0 _S109 = _S107;

#line 1050
    WeightedProbe_0 _S110 = lerp_probe_0(&_S108, &_S109, _S101);

    uint _S111 = _S88.z;

#line 1052
    WeightedProbe_0 _S112 = probe_corner_0(level_3, uint3(_S95, _S96, _S111), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1052
    WeightedProbe_0 _S113 = probe_corner_0(level_3, uint3(_S99, _S96, _S111), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1052
    thread WeightedProbe_0 _S114 = _S112;

#line 1052
    thread WeightedProbe_0 _S115 = _S113;

#line 1052
    WeightedProbe_0 _S116 = lerp_probe_0(&_S114, &_S115, _S101);

#line 1052
    WeightedProbe_0 _S117 = probe_corner_0(level_3, uint3(_S95, _S105, _S111), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1052
    WeightedProbe_0 _S118 = probe_corner_0(level_3, uint3(_S99, _S105, _S111), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1052
    thread WeightedProbe_0 _S119 = _S117;

#line 1052
    thread WeightedProbe_0 _S120 = _S118;

#line 1052
    WeightedProbe_0 _S121 = lerp_probe_0(&_S119, &_S120, _S101);



    float _S122 = f_0.y;

#line 1056
    thread WeightedProbe_0 _S123 = _S104;

#line 1056
    thread WeightedProbe_0 _S124 = _S110;

#line 1056
    WeightedProbe_0 _S125 = lerp_probe_0(&_S123, &_S124, _S122);

#line 1056
    thread WeightedProbe_0 _S126 = _S116;

#line 1056
    thread WeightedProbe_0 _S127 = _S121;

#line 1056
    WeightedProbe_0 _S128 = lerp_probe_0(&_S126, &_S127, _S122);

    float _S129 = f_0.z;

#line 1058
    thread WeightedProbe_0 _S130 = _S125;

#line 1058
    thread WeightedProbe_0 _S131 = _S128;

#line 1058
    WeightedProbe_0 _S132 = lerp_probe_0(&_S130, &_S131, _S129);

#line 1058
    float3 _S133 = float3(2.09439516067504883f) ;

#line 1064
    return max(float3(dot(_S132.sh_0.sh_r_0.xyz / _S133, direction_3) + _S132.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S132.sh_0.sh_g_0.xyz / _S133, direction_3) + _S132.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S132.sh_0.sh_b_0.xyz / _S133, direction_3) + _S132.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S132.weight_1) , _S86);
}

float3 probe_environment_0(float3 world_position_7, float3 normal_6, float3 direction_4, KernelContext_0 thread* kernelContext_23)
{
    if((kernelContext_23->camera_0->probe_counts_1.w) == 0U)
    {

#line 1070
        return float3(0.0f, 0.0f, 0.0f);
    }

#line 1078
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_7, kernelContext_23->camera_0->probe_level_origin_1[int(0)].xyz, kernelContext_23->camera_0->probe_level_inv_spacing_1[int(0)].xyz, max(float3(kernelContext_23->camera_0->probe_counts_1.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_23->camera_0->probe_levels_1.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1080
    float3 _S134 = probe_level_environment_0(level_4, world_position_7, normal_6, direction_4, kernelContext_23);


    if(share_0 >= 1.0f)
    {

#line 1084
        return _S134;
    }

#line 1084
    float3 _S135 = probe_level_environment_0(level_4 + 1U, world_position_7, normal_6, direction_4, kernelContext_23);

    return _S135 * float3((1.0f - share_0))  + _S134 * float3(share_0) ;
}


#line 794
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

#line 806
    float2 _S136 = float2(1.0f) ;
    float2 _S137 = extent_2 - _S136;

#line 807
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S137);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S138 = int2(low_1);
    int2 _S139 = int2(min(low_1 + _S136, _S137));
    int _S140 = _S138.x;

#line 813
    int _S141 = _S138.y;

#line 813
    int3 _S142 = int3(_S140, _S141, int(0));
    int _S143 = _S139.x;

#line 814
    int3 _S144 = int3(_S143, _S141, int(0));
    float2 _S145 = float2(weight_2.x) ;
    int _S146 = _S139.y;

#line 816
    int3 _S147 = int3(_S140, _S146, int(0));
    int3 _S148 = int3(_S143, _S146, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S142)).xy), uint(((_S142)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S144)).xy), uint(((_S144)).z)))), _S145), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S147)).xy), uint(((_S147)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S148)).xy), uint(((_S148)).z)))), _S145), float2(weight_2.y) );
}

float2 sky_prefilter_at_0(float up_0, float roughness_0, KernelContext_0 thread* kernelContext_24)
{
    return fixed_pair_at_0(kernelContext_24->sky_prefilter_0, float2(up_0, roughness_0));
}

float3 sky_prefiltered_0(float3 direction_5, float roughness_1, KernelContext_0 thread* kernelContext_25)
{
    float up_1 = clamp(direction_5.y, -1.0f, 1.0f);

#line 829
    float2 _S149 = sky_prefilter_at_0(abs(up_1), roughness_1, kernelContext_25);

    bool _S150 = up_1 >= 0.0f;

#line 831
    float3 far_0;

#line 831
    if(_S150)
    {

#line 831
        far_0 = kernelContext_25->camera_0->sky_0[int(0)].xyz;

#line 831
    }
    else
    {

#line 831
        far_0 = kernelContext_25->camera_0->sky_0[int(2)].xyz;

#line 831
    }

#line 831
    float3 opposite_0;
    if(_S150)
    {

#line 832
        opposite_0 = kernelContext_25->camera_0->sky_0[int(2)].xyz;

#line 832
    }
    else
    {

#line 832
        opposite_0 = kernelContext_25->camera_0->sky_0[int(0)].xyz;

#line 832
    }
    float _S151 = _S149.x;

#line 833
    float _S152 = _S149.y;
    return kernelContext_25->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S151 - _S152))  + far_0 * float3(_S151)  + opposite_0 * float3(_S152) ;
}

float3 sky_view_at_0(float up_2, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_26)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_1 = clamp(up_2, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_1));

#line 841
    float _S153;
    if(clamped_1 >= 0.0f)
    {

#line 842
        _S153 = root_0;

#line 842
    }
    else
    {

#line 842
        _S153 = - root_0;

#line 842
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S153, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S154 = uint(x0_0);

#line 856
    float3 _S155 = float3((1.0f - fx_0)) ;
    uint _S156 = uint(min(x0_0 + 1.0f, 95.0f));

#line 857
    float3 _S157 = float3(fx_0) ;


    return ((float4(*(kernelContext_26->sky_view_0+(row0_0 + _S154))) ).xyz * _S155 + (float4(*(kernelContext_26->sky_view_0+(row0_0 + _S156))) ).xyz * _S157) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_26->sky_view_0+(row1_0 + _S154))) ).xyz * _S155 + (float4(*(kernelContext_26->sky_view_0+(row1_0 + _S156))) ).xyz * _S157) * float3(fy_0) ;
}

float3 atmosphere_radiance_0(float3 direction_6, KernelContext_0 thread* kernelContext_27)
{
    float3 sun_0 = kernelContext_27->camera_0->atmosphere_0.xyz;
    float _S158 = direction_6.x;

#line 866
    float _S159 = direction_6.z;

#line 866
    float view_flat_0 = sqrt(_S158 * _S158 + _S159 * _S159);
    float _S160 = sun_0.x;

#line 867
    float _S161 = sun_0.z;

#line 867
    float sun_flat_0 = sqrt(_S160 * _S160 + _S161 * _S161);

#line 867
    bool _S162;

    if(view_flat_0 > 0.0f)
    {

#line 869
        _S162 = sun_flat_0 > 0.0f;

#line 869
    }
    else
    {

#line 869
        _S162 = false;

#line 869
    }

#line 869
    float cosine_1;

#line 869
    if(_S162)
    {

#line 869
        cosine_1 = (_S158 * _S160 + _S159 * _S161) / (view_flat_0 * sun_flat_0);

#line 869
    }
    else
    {

#line 869
        cosine_1 = 1.0f;

#line 869
    }

#line 869
    float3 _S163 = sky_view_at_0(direction_6.y, cosine_1, kernelContext_27);



    return _S163;
}

float3 sky_environment_0(float3 direction_7, float roughness_2, float share_1, KernelContext_0 thread* kernelContext_28)
{

#line 876
    float3 _S164 = sky_prefiltered_0(direction_7, roughness_2, kernelContext_28);


    if((kernelContext_28->camera_0->atmosphere_0.w) <= 0.0f)
    {
        return _S164;
    }



    float3 _S165 = _S164 * float3((1.0f - share_1)) ;

#line 886
    float3 _S166 = atmosphere_radiance_0(direction_7, kernelContext_28);

#line 886
    return _S165 + _S166 * float3(share_1) ;
}


#line 369
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S167 = - d_0;

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
            float _S168 = series_0 * _S167 + FOG_RATIO_KERNEL_0[term_2];

#line 375
            int term_3 = term_2 - int(1);

#line 375
            series_0 = _S168;

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
    float _S169 = a_1 * a_1;

#line 412
    float d_1 = 1.0f + _S169 - 2.0f * a_1 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S169) / (d_1 * sqrt(d_1));
}

float3 volumetric_source_0(float3 view_direction_0, float4 lit_0, KernelContext_0 thread* kernelContext_29)
{



    return kernelContext_29->params_0->fog_color_1.xyz + kernelContext_29->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_29->params_0->sun_direction_0.w, dot(kernelContext_29->params_0->sun_direction_0.xyz, view_direction_0)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 1091
struct WaterAir_0
{
    float survives_0;
    float3 inscatter_0;
};


#line 1105
WaterAir_0 water_froxel_air_0(int2 pixel_7, float depth_4, KernelContext_0 thread* kernelContext_30)
{
    thread WaterAir_0 air_0;
    (&air_0)->survives_0 = 1.0f;
    (&air_0)->inscatter_0 = float3(0.0f, 0.0f, 0.0f);

    uint _S170 = max(kernelContext_30->params_0->grid_x_0, 1U);
    uint _S171 = max(kernelContext_30->params_0->grid_y_0, 1U);
    uint _S172 = max(kernelContext_30->params_0->slices_0, 1U);
    uint tiles_0 = _S170 * _S171;
    uint _S173 = max(kernelContext_30->params_0->tile_pixels_0, 1U);


    int _S174 = pixel_7.x;
    int _S175 = pixel_7.y;

#line 1117
    float2 ndc_3 = float2((float(_S174) + 0.5f) / float(max(kernelContext_30->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S175) + 0.5f) / float(max(kernelContext_30->params_0->viewport_y_0, 1U)) * 2.0f);

#line 1117
    float view_depth_0;



    if(depth_4 > 0.0f)
    {

#line 1121
        float3 _S176 = volumetric_unproject_0(ndc_3, depth_4, kernelContext_30);

#line 1121
        view_depth_0 = dot(kernelContext_30->params_0->depth_row_0, float4(_S176, 1.0f));

#line 1121
    }
    else
    {

#line 1121
        view_depth_0 = 1000.0f;

#line 1121
    }

#line 1126
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 1126
    float slice_start_0 = 0.0f;

#line 1126
    uint slice_0 = 0U;

#line 1126
    float next_start_0 = 0.14677993953227997f;

#line 1144
    for(;;)
    {

#line 1144
        uint _S177 = slice_0 + 1U;

#line 1144
        bool _S178;

#line 1144
        if(_S177 < _S172)
        {

#line 1144
            _S178 = next_start_0 <= view_depth_1;

#line 1144
        }
        else
        {

#line 1144
            _S178 = false;

#line 1144
        }

#line 1144
        if(_S178)
        {
        }
        else
        {

#line 1144
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 1147
        slice_start_0 = next_start_0;

#line 1147
        next_start_0 = next_start_1;

#line 1147
        slice_0 = _S177;

#line 1144
    }

#line 1154
    uint _S179 = uint(max(_S174, int(0))) / _S173;

#line 1154
    uint _S180 = min(_S179, _S170 - 1U);
    uint _S181 = uint(max(_S175, int(0))) / _S173;
    uint froxel_0 = _S180 + min(_S181, _S171 - 1U) * _S170 + slice_0 * tiles_0;
    if(froxel_0 >= (kernelContext_30->params_0->froxel_count_0))
    {
        return air_0;
    }

#line 1159
    float4 _S182 = float4(*(kernelContext_30->volumetrics_0+froxel_0)) ;

#line 1159
    float3 _S183 = volumetric_unproject_0(ndc_3, 1.0f, kernelContext_30);

#line 1168
    float3 along_0 = (_S183 - kernelContext_30->params_0->eye_0.xyz) / float3(max(dot(kernelContext_30->params_0->depth_row_0, float4(_S183, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = kernelContext_30->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = kernelContext_30->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;
    float reference_5 = kernelContext_30->params_0->fog_params_1.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_30->params_0->fog_params_1.x, kernelContext_30->params_0->fog_params_1.y, from_0.y - reference_5, to_0.y - reference_5, length_of_0));

#line 1176
    float3 view_direction_1;

#line 1182
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 1182
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 1182
    }
    else
    {

#line 1182
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 1182
    }

#line 1182
    float3 _S184 = volumetric_source_0(view_direction_1, float4(*(kernelContext_30->lighting_0+froxel_0)) , kernelContext_30);

#line 1189
    float3 partial_radiance_0 = _S184 * float3((1.0f - partial_survives_0)) ;

    float _S185 = _S182.w;

#line 1191
    (&air_0)->survives_0 = _S185 * partial_survives_0;
    (&air_0)->inscatter_0 = _S182.xyz + float3(_S185)  * partial_radiance_0;
    return air_0;
}


#line 398
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 1198
WaterAir_0 water_analytic_air_0(float3 surface_1, KernelContext_0 thread* kernelContext_31)
{

#line 1205
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0(kernelContext_31->frame_0->fog_params_0.x, kernelContext_31->frame_0->fog_params_0.y, kernelContext_31->frame_0->camera_position_0.y - kernelContext_31->frame_0->fog_params_0.z, surface_1.y - kernelContext_31->frame_0->fog_params_0.z, length(kernelContext_31->frame_0->camera_position_0.xyz - surface_1)));

    thread WaterAir_0 air_1;
    (&air_1)->survives_0 = fog_survives_0;
    (&air_1)->inscatter_0 = kernelContext_31->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;
    return air_1;
}


#line 1210
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1210
struct pixelInput_0
{
    float3 world_position_8 [[user(TEXCOORD)]];
    [[flat]] uint body_1 [[user(TEXCOORD_1)]];
};


#line 1271
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S186 [[stage_in]], float4 position_1 [[position]], WaterVertex_natural_0 device* water_vertices_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(1)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], WaterMedium_natural_0 device* media_1 [[buffer(5)]], texture2d<float, access::sample> scene_color_1 [[texture(0)]], VolumetricParams_natural_0 constant* params_1 [[buffer(3)]], WaterParams_0 constant* water_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(2)]], sampler shadow_sampler_1 [[sampler(0)]], SsrParams_natural_0 constant* camera_1 [[buffer(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(3)]], packed_float4 device* sky_view_1 [[buffer(7)]], packed_float4 device* volumetrics_1 [[buffer(8)]], packed_float4 device* lighting_1 [[buffer(9)]])
{

#line 1271
    thread KernelContext_0 kernelContext_32;

#line 1271
    (&kernelContext_32)->water_vertices_0 = water_vertices_1;

#line 1271
    (&kernelContext_32)->frame_0 = frame_1;

#line 1271
    (&kernelContext_32)->scene_depth_0 = scene_depth_1;

#line 1271
    (&kernelContext_32)->media_0 = media_1;

#line 1271
    (&kernelContext_32)->scene_color_0 = scene_color_1;

#line 1271
    (&kernelContext_32)->params_0 = params_1;

#line 1271
    (&kernelContext_32)->water_0 = water_1;

#line 1271
    (&kernelContext_32)->shadow_atlas_0 = shadow_atlas_1;

#line 1271
    (&kernelContext_32)->shadow_sampler_0 = shadow_sampler_1;

#line 1271
    (&kernelContext_32)->camera_0 = camera_1;

#line 1271
    (&kernelContext_32)->probes_0 = probes_1;

#line 1271
    (&kernelContext_32)->probe_visibility_0 = probe_visibility_1;

#line 1271
    (&kernelContext_32)->sky_prefilter_0 = sky_prefilter_1;

#line 1271
    (&kernelContext_32)->sky_view_0 = sky_view_1;

#line 1271
    (&kernelContext_32)->volumetrics_0 = volumetrics_1;

#line 1271
    (&kernelContext_32)->lighting_0 = lighting_1;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    float2 extent_3 = float2(float(width_2), float(height_2));
    int2 last_2 = int2(int(width_2) - int(1), int(height_2) - int(1));
    float2 _S187 = position_1.xy;

#line 1278
    int2 _S188 = int2(_S187);
    float surface_depth_0 = position_1.z;

    WaterMedium_natural_0 medium_0 = media_1[_S186.body_1];

    float3 up_3 = float3(0.0f, 1.0f, 0.0f);


    float3 incident_1 = normalize(_S186.world_position_8 - frame_1->camera_position_0.xyz);
    float _S189 = - incident_1.y;

#line 1287
    float cos_incident_1 = saturate(_S189);

#line 1294
    int3 _S190 = int3(_S188, int(0));

#line 1294
    float4 straight_0 = ((scene_color_1).read(vec<uint,2>(((_S190)).xy), uint(((_S190)).z)));

#line 1294
    float3 _S191 = water_opaque_point_0(_S188, ((scene_depth_1).read(vec<uint,2>(((_S190)).xy), uint(((_S190)).z))), extent_3, _S186.world_position_8, incident_1, &kernelContext_32);

#line 1304
    float3 refracted_0 = water_refract_0(incident_1, up_3, 0.75f);

    float4 landing_clip_0 = (((float4(_S186.world_position_8 + refracted_0 * float3((max(_S186.world_position_8.y - _S191.y, 0.0f) / max(- refracted_0.y, 9.99999997475242708e-07f))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));



    float _S192 = landing_clip_0.w;

#line 1310
    float3 behind_point_0;

#line 1310
    float4 behind_1;

#line 1310
    if(_S192 > 0.0f)
    {
        float2 landing_ndc_0 = landing_clip_0.xy / float2(_S192) ;
        int2 bent_0 = clamp(int2(float2(landing_ndc_0.x * 0.5f + 0.5f, 0.5f - landing_ndc_0.y * 0.5f) * extent_3), int2(int(0), int(0)), last_2);


        int3 _S193 = int3(bent_0, int(0));

#line 1316
        float bent_depth_0 = (((&kernelContext_32)->scene_depth_0).read(vec<uint,2>(((_S193)).xy), uint(((_S193)).z)));

#line 1322
        if(bent_depth_0 < surface_depth_0)
        {
            float4 _S194 = (((&kernelContext_32)->scene_color_0).read(vec<uint,2>(((_S193)).xy), uint(((_S193)).z)));

#line 1324
            float3 _S195 = water_opaque_point_0(bent_0, bent_depth_0, extent_3, _S186.world_position_8, refracted_0, &kernelContext_32);

#line 1324
            behind_point_0 = _S195;

#line 1324
            behind_1 = _S194;

#line 1322
        }
        else
        {

#line 1322
            behind_point_0 = _S191;

#line 1322
            behind_1 = straight_0;

#line 1322
        }

#line 1310
    }
    else
    {

#line 1310
        behind_point_0 = _S191;

#line 1310
        behind_1 = straight_0;

#line 1310
    }

#line 1330
    float thickness_0 = length(behind_point_0 - _S186.world_position_8);

#line 1330
    float4 _S196 = float4(medium_0.scattering_0) ;
    float3 extinction_0 = (float4(medium_0.absorption_0) ).xyz + _S196.xyz;
    float _S197 = extinction_0.x;
    float _S198 = extinction_0.y;
    float _S199 = extinction_0.z;

#line 1332
    float3 transmittance_0 = float3(fog_exp_neg_0(_S197 * thickness_0), fog_exp_neg_0(_S198 * thickness_0), fog_exp_neg_0(_S199 * thickness_0));

#line 1332
    float _S200;

#line 1340
    if(_S197 > 0.0f)
    {

#line 1340
        _S200 = _S196.x / _S197;

#line 1340
    }
    else
    {

#line 1340
        _S200 = 0.0f;

#line 1340
    }

#line 1340
    float _S201;
    if(_S198 > 0.0f)
    {

#line 1341
        _S201 = _S196.y / _S198;

#line 1341
    }
    else
    {

#line 1341
        _S201 = 0.0f;

#line 1341
    }

#line 1341
    float _S202;
    if(_S199 > 0.0f)
    {

#line 1342
        _S202 = _S196.z / _S199;

#line 1342
    }
    else
    {

#line 1342
        _S202 = 0.0f;

#line 1342
    }

#line 1340
    float3 albedo_0 = float3(_S200, _S201, _S202);


    float3 to_sun_0 = (&kernelContext_32)->water_0->sun_direction_1.xyz;


    float _S203 = to_sun_0.y;

#line 1344
    thread uint cascade_3;
    thread float cascade_fade_0;

#line 1345
    float _S204 = sun_visibility_0(_S186.world_position_8, to_sun_0, _S203, up_3, _S187, &cascade_3, &cascade_fade_0, &kernelContext_32);


    float3 _S205 = (&kernelContext_32)->frame_0->ambient_0.xyz;

#line 1348
    float3 _S206 = sky_irradiance_0(up_3, &kernelContext_32);

#line 1348
    float3 _S207 = float3(1.0f) ;

    float3 scattered_0 = albedo_0 * (_S205 + _S206 + (&kernelContext_32)->water_0->sun_color_0.xyz * float3((max(_S203, 0.0f) * _S204)) ) * (_S207 - transmittance_0);

#line 1357
    float3 mirrored_0 = float3(incident_1.x, _S189, incident_1.z);

#line 1357
    float3 _S208 = probe_environment_0(_S186.world_position_8, up_3, mirrored_0, &kernelContext_32);

#line 1357
    float3 _S209 = sky_environment_0(mirrored_0, 0.0f, 1.0f, &kernelContext_32);

    float3 environment_0 = _S208 + _S209;
    float grazing_0 = 1.0f - cos_incident_1;
    float grazing2_0 = grazing_0 * grazing_0;
    float fresnel_0 = 0.01999999955296516f + 0.98000001907348633f * (grazing2_0 * grazing2_0 * grazing_0);

#line 1362
    WaterAir_0 air_2;

#line 1373
    if(((&kernelContext_32)->water_0->froxels_0.x) != 0U)
    {

#line 1373
        WaterAir_0 _S210 = water_froxel_air_0(_S188, surface_depth_0, &kernelContext_32);

#line 1373
        air_2 = _S210;

#line 1373
    }
    else
    {

#line 1373
        WaterAir_0 _S211 = water_analytic_air_0(_S186.world_position_8, &kernelContext_32);

#line 1373
        air_2 = _S211;

#line 1373
    }

#line 1373
    float3 _S212 = float3((1.0f - fresnel_0)) ;

#line 1390
    float fade_1 = saturate(thickness_0 / 0.25f);

#line 1390
    pixelOutput_0 _S213 = { float4(straight_0.xyz * float3((1.0f - fade_1))  + (behind_1.xyz * transmittance_0 * _S212 + (environment_0 * float3(fresnel_0)  + scattered_0 * _S212) * float3(air_2.survives_0)  + air_2.inscatter_0 * (_S207 - transmittance_0 * _S212)) * float3(fade_1) , straight_0.w) };
    return _S213;
}


#line 1391
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_9 [[user(TEXCOORD)]];
    uint body_2 [[user(TEXCOORD_1)]];
};


#line 1247
struct SurfaceOutput_0
{
    float4 position_3;
    float3 world_position_10;
    [[flat]] uint body_3;
};


#line 1247
[[vertex]] vertexMain_Result_0 vertexMain(uint index_6 [[vertex_id]], WaterVertex_natural_0 device* water_vertices_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(1)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], WaterMedium_natural_0 device* media_2 [[buffer(5)]], texture2d<float, access::sample> scene_color_2 [[texture(0)]], VolumetricParams_natural_0 constant* params_2 [[buffer(3)]], WaterParams_0 constant* water_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(2)]], sampler shadow_sampler_2 [[sampler(0)]], SsrParams_natural_0 constant* camera_2 [[buffer(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(3)]], packed_float4 device* sky_view_2 [[buffer(7)]], packed_float4 device* volumetrics_2 [[buffer(8)]], packed_float4 device* lighting_2 [[buffer(9)]])
{

#line 1247
    thread KernelContext_0 kernelContext_33;

#line 1247
    (&kernelContext_33)->water_vertices_0 = water_vertices_2;

#line 1247
    (&kernelContext_33)->frame_0 = frame_2;

#line 1247
    (&kernelContext_33)->scene_depth_0 = scene_depth_2;

#line 1247
    (&kernelContext_33)->media_0 = media_2;

#line 1247
    (&kernelContext_33)->scene_color_0 = scene_color_2;

#line 1247
    (&kernelContext_33)->params_0 = params_2;

#line 1247
    (&kernelContext_33)->water_0 = water_2;

#line 1247
    (&kernelContext_33)->shadow_atlas_0 = shadow_atlas_2;

#line 1247
    (&kernelContext_33)->shadow_sampler_0 = shadow_sampler_2;

#line 1247
    (&kernelContext_33)->camera_0 = camera_2;

#line 1247
    (&kernelContext_33)->probes_0 = probes_2;

#line 1247
    (&kernelContext_33)->probe_visibility_0 = probe_visibility_2;

#line 1247
    (&kernelContext_33)->sky_prefilter_0 = sky_prefilter_2;

#line 1247
    (&kernelContext_33)->sky_view_0 = sky_view_2;

#line 1247
    (&kernelContext_33)->volumetrics_0 = volumetrics_2;

#line 1247
    (&kernelContext_33)->lighting_0 = lighting_2;

#line 1262
    WaterVertex_natural_0 vertex_0 = water_vertices_2[index_6];
    thread SurfaceOutput_0 output_1;
    float3 _S214 = (float4(vertex_0.position_0) ).xyz;

#line 1264
    (&output_1)->world_position_10 = _S214;
    (&output_1)->position_3 = (((float4(_S214, 1.0f)) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_0[int(0)][int(0)], frame_2->view_proj_0.data_0[int(1)][int(0)], frame_2->view_proj_0.data_0[int(2)][int(0)], frame_2->view_proj_0.data_0[int(3)][int(0)], frame_2->view_proj_0.data_0[int(0)][int(1)], frame_2->view_proj_0.data_0[int(1)][int(1)], frame_2->view_proj_0.data_0[int(2)][int(1)], frame_2->view_proj_0.data_0[int(3)][int(1)], frame_2->view_proj_0.data_0[int(0)][int(2)], frame_2->view_proj_0.data_0[int(1)][int(2)], frame_2->view_proj_0.data_0[int(2)][int(2)], frame_2->view_proj_0.data_0[int(3)][int(2)], frame_2->view_proj_0.data_0[int(0)][int(3)], frame_2->view_proj_0.data_0[int(1)][int(3)], frame_2->view_proj_0.data_0[int(2)][int(3)], frame_2->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_1)->body_3 = (uint4(vertex_0.body_0) ).x;

#line 1266
    thread vertexMain_Result_0 _S215;

#line 1266
    (&_S215)->position_2 = output_1.position_3;

#line 1266
    (&_S215)->world_position_9 = output_1.world_position_10;

#line 1266
    (&_S215)->body_2 = output_1.body_3;

#line 1266
    return _S215;
}

