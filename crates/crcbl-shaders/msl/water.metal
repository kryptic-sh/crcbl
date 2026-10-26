#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 261 "shaders/water.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f } };

#line 277
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 296
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

#line 314
constant array<float2, int(16)> SHADOW_SEARCH_OFFSET_0 = { { float2(1.41421604156494141f, 0.0f), float2(-1.80617594718933105f, 1.65460801124572754f), float2(0.27646398544311523f, -3.15016794204711914f), float2(2.27656793594360352f, 2.96938395500183105f), float2(-4.17778396606445312f, -0.73899197578430176f), float2(3.95756006240844727f, -2.51747989654541016f), float2(-1.32372796535491943f, 4.92420005798339844f), float2(-2.52448797225952148f, -4.86075210571289062f), float2(5.47713613510131836f, 2.00024008750915527f), float2(-5.69804811477661133f, 2.35207200050354004f), float2(2.74683189392089844f, -5.86983203887939453f), float2(2.02983999252319336f, 6.47145605087280273f), float2(-6.11796808242797852f, -3.54548811912536621f), float2(7.17707204818725586f, -1.57785594463348389f), float2(-4.38005590438842773f, 6.23017597198486328f), float2(-1.01189601421356201f, -7.80872011184692383f) } };

#line 297
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) } };

#line 307
constant array<uint, int(16)> SHADOW_DITHER_0 = { { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U } };

#line 258
constant array<float, int(8)> FOG_KERNEL_0 = { { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f } };

#line 1243
float2 water_ndc_0(int2 pixel_0, float2 extent_0)
{
    return float2((float(pixel_0.x) + 0.5f) / extent_0.x * 2.0f - 1.0f, 1.0f - (float(pixel_0.y) + 0.5f) / extent_0.y * 2.0f);
}


#line 200
struct WaterVertex_natural_0
{
    packed_float4 position_0;
    packed_uint4 body_0;
};


#line 189
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 189
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 3335 "core.meta.slang"
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


#line 204
struct WaterMedium_natural_0
{
    packed_float4 absorption_0;
    packed_float4 scattering_0;
};


#line 195
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
    float4 aerial_params_0;
};


#line 154
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


#line 225
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 245
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


#line 404
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 404
float3 volumetric_unproject_1(float2 ndc_1, float depth_1, KernelContext_0 thread* kernelContext_1)
{
    float4 world_1 = (((float4(ndc_1, depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_1->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_1.xyz / float3(world_1.w) ;
}


#line 1252
float3 water_opaque_point_0(int2 pixel_1, float depth_2, float2 extent_1, float3 surface_0, float3 direction_0, KernelContext_0 thread* kernelContext_2)
{

    if(depth_2 <= 0.0f)
    {
        return surface_0 + direction_0 * float3(1000.0f) ;
    }

#line 1257
    float3 _S1 = volumetric_unproject_1(water_ndc_0(pixel_1, extent_1), depth_2, kernelContext_2);

    return _S1;
}


#line 1233
float3 water_refract_0(float3 incident_0, float3 normal_0, float eta_0)
{
    float cos_incident_0 = - dot(incident_0, normal_0);

    return incident_0 * float3(eta_0)  + normal_0 * float3((eta_0 * cos_incident_0 - sqrt(max(1.0f - eta_0 * eta_0 * (1.0f - cos_incident_0 * cos_incident_0), 0.0f)))) ;
}


#line 350
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S2 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 358
    float kernel_0 = 0.0001984127011383f;

#line 358
    int term_0 = int(6);

    for(;;)
    {

#line 360
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 360
            break;
        }
        float _S3 = kernel_0 * _S2 + FOG_KERNEL_0[term_0];

#line 360
        int term_1 = term_0 - int(1);

#line 360
        kernel_0 = _S3;

#line 360
        term_0 = term_1;

#line 360
    }

#line 367
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 457
float4 atlas_rect_0(uint tile_0, KernelContext_0 thread* kernelContext_3)
{
    return kernelContext_3->frame_0->shadow_atlas_rect_0[tile_0];
}


#line 457
float4 atlas_rect_1(uint tile_1, KernelContext_0 thread* kernelContext_4)
{
    return kernelContext_4->frame_0->shadow_atlas_rect_0[tile_1];
}


#line 472
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 467
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_5)
{
    return rect_1.x / kernelContext_5->frame_0->shadow_params_0.x;
}


#line 434
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_0)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_0));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 446
uint shadow_filter_mode_0(float2 pixel_2, KernelContext_0 thread* kernelContext_6)
{

#line 446
    uint _S4;

    if(uint(pixel_2.x) < (kernelContext_6->frame_0->shadow_filter_0.z))
    {

#line 448
        _S4 = kernelContext_6->frame_0->shadow_filter_0.x;

#line 448
    }
    else
    {

#line 448
        _S4 = kernelContext_6->frame_0->shadow_filter_0.y;

#line 448
    }

#line 448
    return _S4;
}


#line 462
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_7)
{
    return kernelContext_7->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 462
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_8)
{
    return kernelContext_8->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 452
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 477
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_9)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S5 = spoke_0.x;

#line 482
    float _S6 = rotation_0.x;

#line 482
    float _S7 = spoke_0.y;

#line 482
    float _S8 = rotation_0.y;


    float _S9 = ((kernelContext_9->shadow_atlas_0).sample_compare((kernelContext_9->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S5 * _S6 - _S7 * _S8, _S5 * _S8 + _S7 * _S6) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 485
    return _S9;
}


#line 477
float tile_tap_1(float4 rect_6, float2 texel_step_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_10)
{

    float2 tile_min_1 = float2(0.5f, 0.5f) * texel_step_1;

    float _S10 = spoke_1.x;

#line 482
    float _S11 = rotation_1.x;

#line 482
    float _S12 = spoke_1.y;

#line 482
    float _S13 = rotation_1.y;


    float _S14 = ((kernelContext_10->shadow_atlas_0).sample_compare((kernelContext_10->shadow_sampler_0), (atlas_uv_0(rect_6, clamp(tile_uv_2 + float2(_S10 * _S11 - _S12 * _S13, _S10 * _S13 + _S12 * _S11) * texel_step_1, tile_min_1, float2(1.0f)  - tile_min_1))), (reference_1), level((0.0f))));

#line 485
    return _S14;
}


#line 548
float tile_box_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, KernelContext_0 thread* kernelContext_11)
{

#line 548
    float4 _S15 = atlas_rect_1(tile_2, kernelContext_11);


    if(atlas_rect_is_empty_0(_S15))
    {
        return 1.0f;
    }

#line 553
    float2 _S16 = atlas_step_1(_S15, kernelContext_11);

#line 553
    int y_0 = int(-1);

#line 553
    float visibility_0 = 0.0f;

#line 558
    for(;;)
    {

#line 558
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 558
            break;
        }

#line 558
        int x_1 = int(-1);

        for(;;)
        {

#line 560
            if(x_1 <= int(1))
            {
            }
            else
            {

#line 560
                break;
            }

#line 560
            float _S17 = tile_tap_1(_S15, _S16, tile_uv_3, float2(float(x_1), float(y_0)), float2(1.0f, 0.0f), reference_2, kernelContext_11);

            float visibility_1 = visibility_0 + _S17;

#line 560
            x_1 = x_1 + int(1);

#line 560
            visibility_0 = visibility_1;

#line 560
        }

#line 558
        y_0 = y_0 + int(1);

#line 558
    }

#line 566
    return visibility_0 / 9.0f;
}


#line 440
float2 shadow_rotation_0(float2 pixel_3)
{
    uint2 cell_0 = uint2(pixel_3) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 569
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_2, KernelContext_0 thread* kernelContext_12)
{
    float2 texel_0 = kernelContext_12->frame_0->shadow_params_0.xy;

#line 571
    float4 _S18 = atlas_rect_0(cascade_0, kernelContext_12);

#line 571
    float2 _S19 = atlas_step_0(_S18, kernelContext_12);


    float2 _S20 = float2(0.5f, 0.5f) * _S19;


    float2 _S21 = float2(1.0f, 1.0f);

#line 577
    float2 _S22 = _S21 / texel_0;

#line 577
    uint index_0 = 0U;

#line 577
    float sum_0 = 0.0f;

#line 577
    float found_0 = 0.0f;



    for(;;)
    {

#line 581
        if(index_0 < 16U)
        {
        }
        else
        {

#line 581
            break;
        }

        float _S23 = SHADOW_SEARCH_OFFSET_0[index_0].x;

#line 584
        float _S24 = rotation_2.x;

#line 584
        float _S25 = SHADOW_SEARCH_OFFSET_0[index_0].y;

#line 584
        float _S26 = rotation_2.y;

#line 592
        int3 _S27 = int3(int2(min(atlas_uv_0(_S18, clamp(tile_uv_4 + float2(_S23 * _S24 - _S25 * _S26, _S23 * _S26 + _S25 * _S24) * _S19, _S20, float2(1.0f)  - _S20)) * _S22, _S22 - _S21)), int(0));

#line 592
        float depth_3 = ((kernelContext_12->shadow_atlas_0).read(vec<uint,2>(((_S27)).xy), uint(((_S27)).z)));
        if(depth_3 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 596
            sum_0 = sum_0 + depth_3;

#line 596
            found_0 = found_1;

#line 593
        }

#line 581
        index_0 = index_0 + 1U;

#line 581
    }

#line 600
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 611
    float _S28 = 2.0f * kernelContext_12->frame_0->cascade_far_0[cascade_0];

#line 611
    float separation_0 = (sum_0 / found_0 - reference_3) * (_S28 + 40.0f);

#line 611
    float _S29 = tile_texels_0(_S18, kernelContext_12);

    return clamp(separation_0 * 0.01999999955296516f / (_S28 / _S29), 2.0f, 8.0f);
}


#line 488
float tile_pcf_0(uint tile_3, float2 tile_uv_5, float reference_4, float2 pixel_4, float radius_0, KernelContext_0 thread* kernelContext_13)
{
    float2 rotation_3 = shadow_rotation_0(pixel_4);

#line 490
    float4 _S30 = atlas_rect_0(tile_3, kernelContext_13);

    if(atlas_rect_is_empty_0(_S30))
    {
        return 1.0f;
    }

#line 494
    float2 _S31 = atlas_step_0(_S30, kernelContext_13);



    thread array<float, int(5)> probe_taps_0;

#line 498
    uint spot_0 = 0U;

#line 498
    float probe_0 = 0.0f;

    for(;;)
    {

#line 500
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 500
            break;
        }

#line 500
        float _S32 = tile_tap_0(_S30, _S31, tile_uv_5, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_0) , rotation_3, reference_4, kernelContext_13);

        probe_taps_0[spot_0] = _S32;


        float probe_1 = probe_0 + _S32;

#line 500
        spot_0 = spot_0 + 1U;

#line 500
        probe_0 = probe_1;

#line 500
    }

#line 511
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 517
    uint index_1 = 1U;

#line 517
    float visibility_2 = probe_taps_0[int(0)];

#line 525
    for(;;)
    {

#line 525
        if(index_1 < 23U)
        {
        }
        else
        {

#line 525
            break;
        }

#line 525
        float _S33 = tile_tap_0(_S30, _S31, tile_uv_5, SHADOW_DISC_0[index_1] * float2(radius_0) , rotation_3, reference_4, kernelContext_13);

        float visibility_3 = visibility_2 + _S33;

#line 525
        index_1 = index_1 + 1U;

#line 525
        visibility_2 = visibility_3;

#line 525
    }

#line 530
    float visibility_4 = visibility_2 + probe_taps_0[int(1)];

#line 530
    float2 _S34 = float2(radius_0) ;

#line 530
    float _S35 = tile_tap_0(_S30, _S31, tile_uv_5, float2(0.43503299355506897f, 0.75919097661972046f) * _S34, rotation_3, reference_4, kernelContext_13);


    float visibility_5 = visibility_4 + _S35 + probe_taps_0[int(2)];

#line 533
    float _S36 = tile_tap_0(_S30, _S31, tile_uv_5, float2(0.82610201835632324f, -0.38168001174926758f) * _S34, rotation_3, reference_4, kernelContext_13);


    float visibility_6 = visibility_5 + _S36 + probe_taps_0[int(3)];

#line 536
    float _S37 = tile_tap_0(_S30, _S31, tile_uv_5, float2(-0.31940698623657227f, -0.88803398609161377f) * _S34, rotation_3, reference_4, kernelContext_13);


    float visibility_7 = visibility_6 + _S37 + probe_taps_0[int(4)];

#line 539
    index_1 = 30U;

#line 539
    visibility_2 = visibility_7;
    for(;;)
    {

#line 540
        if(index_1 < 32U)
        {
        }
        else
        {

#line 540
            break;
        }

#line 540
        float _S38 = tile_tap_0(_S30, _S31, tile_uv_5, SHADOW_DISC_0[index_1] * _S34, rotation_3, reference_4, kernelContext_13);

        float visibility_8 = visibility_2 + _S38;

#line 540
        index_1 = index_1 + 1U;

#line 540
        visibility_2 = visibility_8;

#line 540
    }

#line 545
    return visibility_2 / 32.0f;
}


#line 618
float cascade_visibility_0(uint cascade_1, float3 world_position_0, float3 to_light_1, float3 geometric_normal_1, float2 pixel_5, KernelContext_0 thread* kernelContext_14)
{

#line 619
    float4 _S39 = atlas_rect_0(cascade_1, kernelContext_14);

#line 653
    if(atlas_rect_is_empty_0(_S39))
    {


        return 1.0f;
    }
    float _S40 = 2.0f * kernelContext_14->frame_0->cascade_far_0[cascade_1];

#line 659
    float _S41 = tile_texels_0(_S39, kernelContext_14);

#line 659
    float texel_world_0 = _S40 / _S41;

#line 666
    float4 clip_0 = (((float4(world_position_0 + geometric_normal_1 * float3((texel_world_0 * kernelContext_14->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_1)))  + to_light_1 * float3((texel_world_0 * kernelContext_14->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_14->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_2 = clip_0.xyz / float3(clip_0.w) ;

#line 670
    bool _S42;
    if(any((abs(ndc_2.xy)) > (float2(1.0f) )))
    {

#line 671
        _S42 = true;

#line 671
    }
    else
    {

#line 671
        _S42 = (ndc_2.z) <= 0.0f;

#line 671
    }

#line 671
    if(_S42)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_2.x * 0.5f + 0.5f, 0.5f - ndc_2.y * 0.5f);

#line 681
    uint _S43 = shadow_filter_mode_0(pixel_5, kernelContext_14);

#line 698
    if(_S43 == 2U)
    {

#line 698
        float _S44 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_2.z, kernelContext_14);

        return _S44;
    }

#line 700
    float radius_1;

#line 708
    if(_S43 != 1U)
    {

#line 708
        float _S45 = sun_penumbra_texels_0(cascade_1, tile_uv_6, ndc_2.z, shadow_rotation_0(pixel_5), kernelContext_14);

#line 708
        radius_1 = _S45;

#line 708
    }
    else
    {

#line 708
        radius_1 = 2.0f;

#line 708
    }

#line 708
    float _S46 = tile_pcf_0(cascade_1, tile_uv_6, ndc_2.z, pixel_5, radius_1, kernelContext_14);

#line 713
    return _S46;
}

float sun_visibility_0(float3 world_position_1, float3 to_light_2, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_6, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_15)
{
    uint cascade_2;

#line 718
    bool covered_0;

#line 727
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 739
    float eye_distance_0 = length(world_position_1 - kernelContext_15->frame_0->camera_position_0.xyz);

#line 739
    uint index_2 = 0U;

#line 747
    for(;;)
    {

#line 747
        if(index_2 < 2U)
        {
        }
        else
        {

#line 747
            covered_0 = false;

#line 747
            cascade_2 = 1U;

#line 747
            break;
        }
        if(eye_distance_0 < kernelContext_15->frame_0->cascade_far_0[index_2])
        {

#line 749
            covered_0 = true;

#line 749
            cascade_2 = index_2;



            break;
        }

#line 747
        index_2 = index_2 + 1U;

#line 747
    }

#line 756
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 756
    }

#line 756
    float blend_0;

#line 768
    if((cascade_2 + 1U) < 2U)
    {

        float band_0 = kernelContext_15->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;

#line 771
        blend_0 = saturate((eye_distance_0 - (kernelContext_15->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);

#line 768
    }
    else
    {

#line 768
        blend_0 = 0.0f;

#line 768
    }

#line 776
    *fade_0 = blend_0;

#line 776
    uint lookups_0;

#line 790
    if(blend_0 > 0.0f)
    {

#line 790
        lookups_0 = 2U;

#line 790
    }
    else
    {

#line 790
        lookups_0 = 1U;

#line 790
    }

#line 790
    float visibility_9 = 1.0f;

#line 790
    float next_0 = 1.0f;

#line 790
    uint step_0 = 0U;



    for(;;)
    {

#line 794
        if(step_0 < lookups_0)
        {
        }
        else
        {

#line 794
            break;
        }

#line 794
        float _S47 = cascade_visibility_0(cascade_2 + step_0, world_position_1, to_light_2, geometric_normal_2, pixel_6, kernelContext_15);


        if(step_0 == 0U)
        {

#line 797
            visibility_9 = _S47;

#line 797
        }
        else
        {

#line 797
            next_0 = _S47;

#line 797
        }

#line 794
        step_0 = step_0 + 1U;

#line 794
    }

#line 806
    if(lookups_0 == 1U)
    {

#line 806
        blend_0 = visibility_9;

#line 806
    }
    else
    {

#line 806
        blend_0 = mix(visibility_9, next_0, blend_0);

#line 806
    }

#line 806
    return blend_0;
}


#line 425
float3 sky_irradiance_0(float3 normal_1, KernelContext_0 thread* kernelContext_16)
{
    float4 basis_0 = float4(normal_1, 1.0f);
    return max(float3(dot(kernelContext_16->frame_0->sky_sh_r_0, basis_0), dot(kernelContext_16->frame_0->sky_sh_g_0, basis_0), dot(kernelContext_16->frame_0->sky_sh_b_0, basis_0)), float3(0.0f, 0.0f, 0.0f));
}


#line 1013
float probe_level_reach_0(float3 world_position_2, float3 origin_0, float3 inv_spacing_0, float3 last_0)
{

#line 1013
    float reach_0 = 0.0f;

#line 1013
    uint axis_0 = 0U;


    for(;;)
    {

#line 1016
        if(axis_0 < 3U)
        {
        }
        else
        {

#line 1016
            break;
        }

#line 1016
        uint _S48 = axis_0;

#line 1016
        bool _S49;

        if((last_0[axis_0]) == 0.0f)
        {

#line 1018
            _S49 = true;

#line 1018
        }
        else
        {

#line 1018
            _S49 = (inv_spacing_0[axis_0]) == 0.0f;

#line 1018
        }

#line 1018
        if(_S49)
        {

#line 1019
            axis_0 = axis_0 + 1U;

#line 1016
            continue;
        }

#line 1016
        reach_0 = max(reach_0, abs(2.0f * ((world_position_2[axis_0] - origin_0[axis_0]) * inv_spacing_0[axis_0]) / last_0[_S48] - 1.0f));

#line 1016
        axis_0 = axis_0 + 1U;

#line 1016
    }

#line 1023
    return reach_0;
}

float2 probe_level_of_0(float reach_1, uint levels_0)
{

#line 1026
    uint level_0 = 0U;

    for(;;)
    {

#line 1028
        uint _S50 = level_0 + 1U;

#line 1028
        if(_S50 < levels_0)
        {
        }
        else
        {

#line 1028
            break;
        }
        float _S51 = float(level_0);

#line 1030
        float at_0 = reach_1 * exp2(- _S51);
        if(at_0 < 1.0f)
        {

#line 1032
            return float2(_S51, saturate((1.0f - at_0) / 0.25f));
        }

#line 1028
        level_0 = _S50;

#line 1028
    }

#line 1034
    return float2(float(levels_0 - 1U), 1.0f);
}


#line 963
uint probe_wrap_0(uint cell_1, uint offset_0, uint count_0)
{
    uint at_1 = cell_1 + offset_0;

#line 965
    uint _S52;
    if(at_1 >= count_0)
    {

#line 966
        _S52 = at_1 - count_0;

#line 966
    }
    else
    {

#line 966
        _S52 = at_1;

#line 966
    }

#line 966
    return _S52;
}

uint probe_row_0(uint level_1, uint3 cell_2, KernelContext_0 thread* kernelContext_17)
{
    uint3 counts_0 = kernelContext_17->camera_0->probe_counts_1.xyz;
    uint3 offset_1 = kernelContext_17->camera_0->probe_level_offset_1[level_1].xyz;
    uint _S53 = counts_0.x;
    uint _S54 = counts_0.y;



    return min(kernelContext_17->camera_0->probe_levels_1.y * level_1 + (probe_wrap_0(cell_2.z, offset_1.z, counts_0.z) * _S54 + probe_wrap_0(cell_2.y, offset_1.y, _S54)) * _S53 + probe_wrap_0(cell_2.x, offset_1.x, _S53), max(kernelContext_17->camera_0->probe_counts_1.w, 1U) - 1U);
}


#line 904
float sign_not_zero_0(float value_0)
{

#line 904
    float _S55;

    if(value_0 >= 0.0f)
    {

#line 906
        _S55 = 1.0f;

#line 906
    }
    else
    {

#line 906
        _S55 = -1.0f;

#line 906
    }

#line 906
    return _S55;
}

float2 oct_encode_0(float3 direction_1)
{
    float _S56 = direction_1.y;
    float2 p_0 = direction_1.xz / float2(max(abs(direction_1.x) + abs(_S56) + abs(direction_1.z), 9.99999968265522539e-21f)) ;

#line 912
    float2 p_1;
    if(_S56 < 0.0f)
    {
        float _S57 = p_0.y;

#line 915
        float _S58 = p_0.x;

#line 915
        p_1 = float2((1.0f - abs(_S57)) * sign_not_zero_0(_S58), (1.0f - abs(_S58)) * sign_not_zero_0(_S57));

#line 913
    }
    else
    {

#line 913
        p_1 = p_0;

#line 913
    }

#line 918
    return p_1;
}

float2 probe_moments_0(uint index_3, float3 direction_2, KernelContext_0 thread* kernelContext_18)
{

#line 921
    texture2d_array<float, access::sample> _S59 = kernelContext_18->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S59).get_width(0)),(*((&height_0)) = (_S59).get_height(0)),(*((&layers_0)) = (_S59).get_array_size());

#line 926
    float2 _S60 = float2(0.5f) ;

#line 926
    float2 _S61 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_2) * _S60 + _S60) * float2(16.0f)  + _S61 - _S60;
    float2 _S62 = float2(float(width_0), float(height_0)) - _S61;

#line 930
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S62);
    float2 high_0 = min(low_0 + _S61, _S62);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_3, max(layers_0, 1U) - 1U));

    int _S63 = int(low_0.x);

#line 935
    int _S64 = int(low_0.y);

#line 935
    int4 _S65 = int4(_S63, _S64, layer_0, int(0));
    int _S66 = int(high_0.x);

#line 936
    int4 _S67 = int4(_S66, _S64, layer_0, int(0));
    int _S68 = int(high_0.y);

#line 937
    int4 _S69 = int4(_S63, _S68, layer_0, int(0));
    int4 _S70 = int4(_S66, _S68, layer_0, int(0));
    float2 _S71 = float2(weight_0.x) ;

#line 939
    return mix(mix(((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S65)).xy), uint(((_S65)).z), uint(((_S65)).w))).xy, ((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S67)).xy), uint(((_S67)).z), uint(((_S67)).w))).xy, _S71), mix(((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S69)).xy), uint(((_S69)).z), uint(((_S69)).w))).xy, ((kernelContext_18->probe_visibility_0).read(vec<uint,2>(((_S70)).xy), uint(((_S70)).z), uint(((_S70)).w))).xy, _S71), float2(weight_0.y) );
}

float probe_chebyshev_0(uint index_4, float3 probe_position_0, float3 world_position_3, float3 normal_2, KernelContext_0 thread* kernelContext_19)
{
    float3 to_probe_0 = probe_position_0 - (world_position_3 + normal_2 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 945
    float2 _S72 = probe_moments_0(index_4, - to_probe_0, kernelContext_19);

#line 951
    float _S73 = _S72.x;

#line 951
    float _S74 = max(_S72.y - _S73 * _S73, 0.0f);
    float behind_0 = to_surface_0 - _S73;
    float bound_0 = _S74 / (_S74 + behind_0 * behind_0);

#line 953
    float _S75;
    if(to_surface_0 <= _S73)
    {

#line 954
        _S75 = 1.0f;

#line 954
    }
    else
    {

#line 954
        _S75 = bound_0 * bound_0 * bound_0;

#line 954
    }

#line 954
    return _S75;
}

float probe_weight_0(uint index_5, float3 probe_position_1, float3 world_position_4, float3 normal_3, KernelContext_0 thread* kernelContext_20)
{

#line 957
    float _S76 = probe_chebyshev_0(index_5, probe_position_1, world_position_4, normal_3, kernelContext_20);

    return max(_S76, 0.00009999999747379f);
}


#line 100
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 981
struct WeightedProbe_0
{
    GpuProbe_0 sh_0;
    float weight_1;
};


#line 997
WeightedProbe_0 probe_corner_0(uint level_2, uint3 cell_3, float3 origin_1, float3 spacing_0, float3 world_position_5, float3 normal_4, KernelContext_0 thread* kernelContext_21)
{

#line 998
    uint _S77 = probe_row_0(level_2, cell_3, kernelContext_21);


    GpuProbe_natural_0 stored_0 = kernelContext_21->probes_0[_S77];

#line 1001
    float _S78 = probe_weight_0(_S77, origin_1 + float3(cell_3) * spacing_0, world_position_5, normal_4, kernelContext_21);



    thread WeightedProbe_0 corner_0;

#line 1005
    float4 _S79 = float4(_S78) ;
    (&(&corner_0)->sh_0)->sh_r_0 = float4(stored_0.sh_r_0)  * _S79;
    (&(&corner_0)->sh_0)->sh_g_0 = float4(stored_0.sh_g_0)  * _S79;
    (&(&corner_0)->sh_0)->sh_b_0 = float4(stored_0.sh_b_0)  * _S79;
    (&corner_0)->weight_1 = _S78;
    return corner_0;
}


#line 987
WeightedProbe_0 lerp_probe_0(const WeightedProbe_0 thread* a_0, const WeightedProbe_0 thread* b_0, float t_0)
{
    thread WeightedProbe_0 blended_0;
    float4 _S80 = float4(t_0) ;

#line 990
    (&(&blended_0)->sh_0)->sh_r_0 = mix((&a_0->sh_0)->sh_r_0, (&b_0->sh_0)->sh_r_0, _S80);
    (&(&blended_0)->sh_0)->sh_g_0 = mix((&a_0->sh_0)->sh_g_0, (&b_0->sh_0)->sh_g_0, _S80);
    (&(&blended_0)->sh_0)->sh_b_0 = mix((&a_0->sh_0)->sh_b_0, (&b_0->sh_0)->sh_b_0, _S80);
    (&blended_0)->weight_1 = mix(a_0->weight_1, b_0->weight_1, t_0);
    return blended_0;
}


#line 1037
float3 probe_level_environment_0(uint level_3, float3 world_position_6, float3 normal_5, float3 direction_3, KernelContext_0 thread* kernelContext_22)
{

#line 1037
    float3 _S81 = float3(1.0f) ;

    float3 _S82 = float3(0.0f, 0.0f, 0.0f);

#line 1039
    float3 last_1 = max(float3(kernelContext_22->camera_0->probe_counts_1.xyz) - _S81, _S82);



    float3 origin_2 = kernelContext_22->camera_0->probe_level_origin_1[level_3].xyz;
    float3 inv_0 = kernelContext_22->camera_0->probe_level_inv_spacing_1[level_3].xyz;
    float3 grid_0 = clamp((world_position_6 - origin_2) * inv_0, _S82, last_1);
    float3 base_0 = floor(grid_0);
    float3 f_0 = grid_0 - base_0;
    uint3 _S83 = uint3(base_0);
    uint3 _S84 = uint3(min(base_0 + _S81, last_1));

#line 1054
    float _S85 = inv_0.x;

#line 1054
    float _S86;

#line 1054
    if(_S85 != 0.0f)
    {

#line 1054
        _S86 = 1.0f / _S85;

#line 1054
    }
    else
    {

#line 1054
        _S86 = 0.0f;

#line 1054
    }
    float _S87 = inv_0.y;

#line 1055
    float _S88;

#line 1055
    if(_S87 != 0.0f)
    {

#line 1055
        _S88 = 1.0f / _S87;

#line 1055
    }
    else
    {

#line 1055
        _S88 = 0.0f;

#line 1055
    }
    float _S89 = inv_0.z;

#line 1056
    float _S90;

#line 1056
    if(_S89 != 0.0f)
    {

#line 1056
        _S90 = 1.0f / _S89;

#line 1056
    }
    else
    {

#line 1056
        _S90 = 0.0f;

#line 1056
    }

#line 1054
    float3 spacing_1 = float3(_S86, _S88, _S90);

#line 1063
    uint _S91 = _S83.x;

#line 1063
    uint _S92 = _S83.y;

#line 1063
    uint _S93 = _S83.z;

#line 1063
    WeightedProbe_0 _S94 = probe_corner_0(level_3, uint3(_S91, _S92, _S93), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);
    uint _S95 = _S84.x;

#line 1064
    WeightedProbe_0 _S96 = probe_corner_0(level_3, uint3(_S95, _S92, _S93), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1064
    float _S97 = f_0.x;

#line 1064
    thread WeightedProbe_0 _S98 = _S94;

#line 1064
    thread WeightedProbe_0 _S99 = _S96;

#line 1064
    WeightedProbe_0 _S100 = lerp_probe_0(&_S98, &_S99, _S97);
    uint _S101 = _S84.y;

#line 1065
    WeightedProbe_0 _S102 = probe_corner_0(level_3, uint3(_S91, _S101, _S93), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1065
    WeightedProbe_0 _S103 = probe_corner_0(level_3, uint3(_S95, _S101, _S93), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1065
    thread WeightedProbe_0 _S104 = _S102;

#line 1065
    thread WeightedProbe_0 _S105 = _S103;

#line 1065
    WeightedProbe_0 _S106 = lerp_probe_0(&_S104, &_S105, _S97);

    uint _S107 = _S84.z;

#line 1067
    WeightedProbe_0 _S108 = probe_corner_0(level_3, uint3(_S91, _S92, _S107), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1067
    WeightedProbe_0 _S109 = probe_corner_0(level_3, uint3(_S95, _S92, _S107), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1067
    thread WeightedProbe_0 _S110 = _S108;

#line 1067
    thread WeightedProbe_0 _S111 = _S109;

#line 1067
    WeightedProbe_0 _S112 = lerp_probe_0(&_S110, &_S111, _S97);

#line 1067
    WeightedProbe_0 _S113 = probe_corner_0(level_3, uint3(_S91, _S101, _S107), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1067
    WeightedProbe_0 _S114 = probe_corner_0(level_3, uint3(_S95, _S101, _S107), origin_2, spacing_1, world_position_6, normal_5, kernelContext_22);

#line 1067
    thread WeightedProbe_0 _S115 = _S113;

#line 1067
    thread WeightedProbe_0 _S116 = _S114;

#line 1067
    WeightedProbe_0 _S117 = lerp_probe_0(&_S115, &_S116, _S97);



    float _S118 = f_0.y;

#line 1071
    thread WeightedProbe_0 _S119 = _S100;

#line 1071
    thread WeightedProbe_0 _S120 = _S106;

#line 1071
    WeightedProbe_0 _S121 = lerp_probe_0(&_S119, &_S120, _S118);

#line 1071
    thread WeightedProbe_0 _S122 = _S112;

#line 1071
    thread WeightedProbe_0 _S123 = _S117;

#line 1071
    WeightedProbe_0 _S124 = lerp_probe_0(&_S122, &_S123, _S118);

    float _S125 = f_0.z;

#line 1073
    thread WeightedProbe_0 _S126 = _S121;

#line 1073
    thread WeightedProbe_0 _S127 = _S124;

#line 1073
    WeightedProbe_0 _S128 = lerp_probe_0(&_S126, &_S127, _S125);

#line 1073
    float3 _S129 = float3(2.09439516067504883f) ;

#line 1079
    return max(float3(dot(_S128.sh_0.sh_r_0.xyz / _S129, direction_3) + _S128.sh_0.sh_r_0.w / 3.14159274101257324f, dot(_S128.sh_0.sh_g_0.xyz / _S129, direction_3) + _S128.sh_0.sh_g_0.w / 3.14159274101257324f, dot(_S128.sh_0.sh_b_0.xyz / _S129, direction_3) + _S128.sh_0.sh_b_0.w / 3.14159274101257324f) / float3(_S128.weight_1) , _S82);
}

float3 probe_environment_0(float3 world_position_7, float3 normal_6, float3 direction_4, KernelContext_0 thread* kernelContext_23)
{
    if((kernelContext_23->camera_0->probe_counts_1.w) == 0U)
    {

#line 1085
        return float3(0.0f, 0.0f, 0.0f);
    }

#line 1093
    float2 pick_0 = probe_level_of_0(probe_level_reach_0(world_position_7, kernelContext_23->camera_0->probe_level_origin_1[int(0)].xyz, kernelContext_23->camera_0->probe_level_inv_spacing_1[int(0)].xyz, max(float3(kernelContext_23->camera_0->probe_counts_1.xyz) - float3(1.0f) , float3(0.0f, 0.0f, 0.0f))), clamp(kernelContext_23->camera_0->probe_levels_1.x, 1U, 4U));
    uint level_4 = uint(pick_0.x);
    float share_0 = pick_0.y;

#line 1095
    float3 _S130 = probe_level_environment_0(level_4, world_position_7, normal_6, direction_4, kernelContext_23);


    if(share_0 >= 1.0f)
    {

#line 1099
        return _S130;
    }

#line 1099
    float3 _S131 = probe_level_environment_0(level_4 + 1U, world_position_7, normal_6, direction_4, kernelContext_23);

    return _S131 * float3((1.0f - share_0))  + _S130 * float3(share_0) ;
}


#line 809
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

#line 821
    float2 _S132 = float2(1.0f) ;
    float2 _S133 = extent_2 - _S132;

#line 822
    float2 low_1 = clamp(floor(scaled_1), float2(0.0f, 0.0f), _S133);

    float2 weight_2 = clamp(scaled_1 - low_1, float2(0.0f) , float2(1.0f) );

    int2 _S134 = int2(low_1);
    int2 _S135 = int2(min(low_1 + _S132, _S133));
    int _S136 = _S134.x;

#line 828
    int _S137 = _S134.y;

#line 828
    int3 _S138 = int3(_S136, _S137, int(0));
    int _S139 = _S135.x;

#line 829
    int3 _S140 = int3(_S139, _S137, int(0));
    float2 _S141 = float2(weight_2.x) ;
    int _S142 = _S135.y;

#line 831
    int3 _S143 = int3(_S136, _S142, int(0));
    int3 _S144 = int3(_S139, _S142, int(0));

    return mix(mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S138)).xy), uint(((_S138)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S140)).xy), uint(((_S140)).z)))), _S141), mix(decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S143)).xy), uint(((_S143)).z)))), decode_fixed_pair_0(((table_0).read(vec<uint,2>(((_S144)).xy), uint(((_S144)).z)))), _S141), float2(weight_2.y) );
}

float2 sky_prefilter_at_0(float up_0, float roughness_0, KernelContext_0 thread* kernelContext_24)
{
    return fixed_pair_at_0(kernelContext_24->sky_prefilter_0, float2(up_0, roughness_0));
}

float3 sky_prefiltered_0(float3 direction_5, float roughness_1, KernelContext_0 thread* kernelContext_25)
{
    float up_1 = clamp(direction_5.y, -1.0f, 1.0f);

#line 844
    float2 _S145 = sky_prefilter_at_0(abs(up_1), roughness_1, kernelContext_25);

    bool _S146 = up_1 >= 0.0f;

#line 846
    float3 far_0;

#line 846
    if(_S146)
    {

#line 846
        far_0 = kernelContext_25->camera_0->sky_0[int(0)].xyz;

#line 846
    }
    else
    {

#line 846
        far_0 = kernelContext_25->camera_0->sky_0[int(2)].xyz;

#line 846
    }

#line 846
    float3 opposite_0;
    if(_S146)
    {

#line 847
        opposite_0 = kernelContext_25->camera_0->sky_0[int(2)].xyz;

#line 847
    }
    else
    {

#line 847
        opposite_0 = kernelContext_25->camera_0->sky_0[int(0)].xyz;

#line 847
    }
    float _S147 = _S145.x;

#line 848
    float _S148 = _S145.y;
    return kernelContext_25->camera_0->sky_0[int(1)].xyz * float3((1.0f - _S147 - _S148))  + far_0 * float3(_S147)  + opposite_0 * float3(_S148) ;
}

float3 sky_view_at_0(float up_2, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_26)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_1 = clamp(up_2, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_1));

#line 856
    float _S149;
    if(clamped_1 >= 0.0f)
    {

#line 857
        _S149 = root_0;

#line 857
    }
    else
    {

#line 857
        _S149 = - root_0;

#line 857
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S149, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S150 = uint(x0_0);

#line 871
    float3 _S151 = float3((1.0f - fx_0)) ;
    uint _S152 = uint(min(x0_0 + 1.0f, 95.0f));

#line 872
    float3 _S153 = float3(fx_0) ;


    return ((float4(*(kernelContext_26->sky_view_0+(row0_0 + _S150))) ).xyz * _S151 + (float4(*(kernelContext_26->sky_view_0+(row0_0 + _S152))) ).xyz * _S153) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_26->sky_view_0+(row1_0 + _S150))) ).xyz * _S151 + (float4(*(kernelContext_26->sky_view_0+(row1_0 + _S152))) ).xyz * _S153) * float3(fy_0) ;
}

float3 atmosphere_radiance_0(float3 direction_6, KernelContext_0 thread* kernelContext_27)
{
    float3 sun_0 = kernelContext_27->camera_0->atmosphere_0.xyz;
    float _S154 = direction_6.x;

#line 881
    float _S155 = direction_6.z;

#line 881
    float view_flat_0 = sqrt(_S154 * _S154 + _S155 * _S155);
    float _S156 = sun_0.x;

#line 882
    float _S157 = sun_0.z;

#line 882
    float sun_flat_0 = sqrt(_S156 * _S156 + _S157 * _S157);

#line 882
    bool _S158;

    if(view_flat_0 > 0.0f)
    {

#line 884
        _S158 = sun_flat_0 > 0.0f;

#line 884
    }
    else
    {

#line 884
        _S158 = false;

#line 884
    }

#line 884
    float cosine_1;

#line 884
    if(_S158)
    {

#line 884
        cosine_1 = (_S154 * _S156 + _S155 * _S157) / (view_flat_0 * sun_flat_0);

#line 884
    }
    else
    {

#line 884
        cosine_1 = 1.0f;

#line 884
    }

#line 884
    float3 _S159 = sky_view_at_0(direction_6.y, cosine_1, kernelContext_27);



    return _S159;
}

float3 sky_environment_0(float3 direction_7, float roughness_2, float share_1, KernelContext_0 thread* kernelContext_28)
{

#line 891
    float3 _S160 = sky_prefiltered_0(direction_7, roughness_2, kernelContext_28);


    if((kernelContext_28->camera_0->atmosphere_0.w) <= 0.0f)
    {
        return _S160;
    }



    float3 _S161 = _S160 * float3((1.0f - share_1)) ;

#line 901
    float3 _S162 = atmosphere_radiance_0(direction_7, kernelContext_28);

#line 901
    return _S161 + _S162 * float3(share_1) ;
}


#line 370
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S163 = - d_0;

#line 374
        float series_0 = 0.00833333376795053f;

#line 374
        int term_2 = int(3);

        for(;;)
        {

#line 376
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 376
                break;
            }
            float _S164 = series_0 * _S163 + FOG_RATIO_KERNEL_0[term_2];

#line 376
            int term_3 = term_2 - int(1);

#line 376
            series_0 = _S164;

#line 376
            term_2 = term_3;

#line 376
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

#line 396
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 410
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_1 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S165 = a_1 * a_1;

#line 413
    float d_1 = 1.0f + _S165 - 2.0f * a_1 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S165) / (d_1 * sqrt(d_1));
}

float3 volumetric_source_0(float3 view_direction_0, float4 lit_0, KernelContext_0 thread* kernelContext_29)
{



    return kernelContext_29->params_0->fog_color_1.xyz + kernelContext_29->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_29->params_0->sun_direction_0.w, dot(kernelContext_29->params_0->sun_direction_0.xyz, view_direction_0)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 1106
struct WaterAir_0
{
    float survives_0;
    float3 inscatter_0;
};


#line 1120
WaterAir_0 water_froxel_air_0(int2 pixel_7, float depth_4, KernelContext_0 thread* kernelContext_30)
{
    thread WaterAir_0 air_0;
    (&air_0)->survives_0 = 1.0f;
    (&air_0)->inscatter_0 = float3(0.0f, 0.0f, 0.0f);

    uint _S166 = max(kernelContext_30->params_0->grid_x_0, 1U);
    uint _S167 = max(kernelContext_30->params_0->grid_y_0, 1U);
    uint _S168 = max(kernelContext_30->params_0->slices_0, 1U);
    uint tiles_0 = _S166 * _S167;
    uint _S169 = max(kernelContext_30->params_0->tile_pixels_0, 1U);


    int _S170 = pixel_7.x;
    int _S171 = pixel_7.y;

#line 1132
    float2 ndc_3 = float2((float(_S170) + 0.5f) / float(max(kernelContext_30->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S171) + 0.5f) / float(max(kernelContext_30->params_0->viewport_y_0, 1U)) * 2.0f);

#line 1132
    float view_depth_0;



    if(depth_4 > 0.0f)
    {

#line 1136
        float3 _S172 = volumetric_unproject_0(ndc_3, depth_4, kernelContext_30);

#line 1136
        view_depth_0 = dot(kernelContext_30->params_0->depth_row_0, float4(_S172, 1.0f));

#line 1136
    }
    else
    {

#line 1136
        view_depth_0 = 1000.0f;

#line 1136
    }

#line 1141
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 1141
    float slice_start_0 = 0.0f;

#line 1141
    uint slice_0 = 0U;

#line 1141
    float next_start_0 = 0.14677993953227997f;

#line 1159
    for(;;)
    {

#line 1159
        uint _S173 = slice_0 + 1U;

#line 1159
        bool _S174;

#line 1159
        if(_S173 < _S168)
        {

#line 1159
            _S174 = next_start_0 <= view_depth_1;

#line 1159
        }
        else
        {

#line 1159
            _S174 = false;

#line 1159
        }

#line 1159
        if(_S174)
        {
        }
        else
        {

#line 1159
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 1162
        slice_start_0 = next_start_0;

#line 1162
        next_start_0 = next_start_1;

#line 1162
        slice_0 = _S173;

#line 1159
    }

#line 1169
    uint _S175 = uint(max(_S170, int(0))) / _S169;

#line 1169
    uint _S176 = min(_S175, _S166 - 1U);
    uint _S177 = uint(max(_S171, int(0))) / _S169;
    uint froxel_0 = _S176 + min(_S177, _S167 - 1U) * _S166 + slice_0 * tiles_0;
    if(froxel_0 >= (kernelContext_30->params_0->froxel_count_0))
    {
        return air_0;
    }

#line 1174
    float4 _S178 = float4(*(kernelContext_30->volumetrics_0+froxel_0)) ;

#line 1174
    float3 _S179 = volumetric_unproject_0(ndc_3, 1.0f, kernelContext_30);

#line 1183
    float3 along_0 = (_S179 - kernelContext_30->params_0->eye_0.xyz) / float3(max(dot(kernelContext_30->params_0->depth_row_0, float4(_S179, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = kernelContext_30->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = kernelContext_30->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;
    float reference_5 = kernelContext_30->params_0->fog_params_1.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_30->params_0->fog_params_1.x, kernelContext_30->params_0->fog_params_1.y, from_0.y - reference_5, to_0.y - reference_5, length_of_0));

#line 1191
    float3 view_direction_1;

#line 1197
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 1197
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 1197
    }
    else
    {

#line 1197
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 1197
    }

#line 1197
    float3 _S180 = volumetric_source_0(view_direction_1, float4(*(kernelContext_30->lighting_0+froxel_0)) , kernelContext_30);

#line 1204
    float3 partial_radiance_0 = _S180 * float3((1.0f - partial_survives_0)) ;

    float _S181 = _S178.w;

#line 1206
    (&air_0)->survives_0 = _S181 * partial_survives_0;
    (&air_0)->inscatter_0 = _S178.xyz + float3(_S181)  * partial_radiance_0;
    return air_0;
}


#line 399
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 1213
WaterAir_0 water_analytic_air_0(float3 surface_1, KernelContext_0 thread* kernelContext_31)
{

#line 1220
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0(kernelContext_31->frame_0->fog_params_0.x, kernelContext_31->frame_0->fog_params_0.y, kernelContext_31->frame_0->camera_position_0.y - kernelContext_31->frame_0->fog_params_0.z, surface_1.y - kernelContext_31->frame_0->fog_params_0.z, length(kernelContext_31->frame_0->camera_position_0.xyz - surface_1)));

    thread WaterAir_0 air_1;
    (&air_1)->survives_0 = fog_survives_0;
    (&air_1)->inscatter_0 = kernelContext_31->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) ;
    return air_1;
}


#line 1225
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1225
struct pixelInput_0
{
    float3 world_position_8 [[user(TEXCOORD)]];
    [[flat]] uint body_1 [[user(TEXCOORD_1)]];
};


#line 1286
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S182 [[stage_in]], float4 position_1 [[position]], WaterVertex_natural_0 device* water_vertices_1 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(1)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], WaterMedium_natural_0 device* media_1 [[buffer(5)]], texture2d<float, access::sample> scene_color_1 [[texture(0)]], VolumetricParams_natural_0 constant* params_1 [[buffer(3)]], WaterParams_0 constant* water_1 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(2)]], sampler shadow_sampler_1 [[sampler(0)]], SsrParams_natural_0 constant* camera_1 [[buffer(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_1 [[texture(3)]], packed_float4 device* sky_view_1 [[buffer(7)]], packed_float4 device* volumetrics_1 [[buffer(8)]], packed_float4 device* lighting_1 [[buffer(9)]])
{

#line 1286
    thread KernelContext_0 kernelContext_32;

#line 1286
    (&kernelContext_32)->water_vertices_0 = water_vertices_1;

#line 1286
    (&kernelContext_32)->frame_0 = frame_1;

#line 1286
    (&kernelContext_32)->scene_depth_0 = scene_depth_1;

#line 1286
    (&kernelContext_32)->media_0 = media_1;

#line 1286
    (&kernelContext_32)->scene_color_0 = scene_color_1;

#line 1286
    (&kernelContext_32)->params_0 = params_1;

#line 1286
    (&kernelContext_32)->water_0 = water_1;

#line 1286
    (&kernelContext_32)->shadow_atlas_0 = shadow_atlas_1;

#line 1286
    (&kernelContext_32)->shadow_sampler_0 = shadow_sampler_1;

#line 1286
    (&kernelContext_32)->camera_0 = camera_1;

#line 1286
    (&kernelContext_32)->probes_0 = probes_1;

#line 1286
    (&kernelContext_32)->probe_visibility_0 = probe_visibility_1;

#line 1286
    (&kernelContext_32)->sky_prefilter_0 = sky_prefilter_1;

#line 1286
    (&kernelContext_32)->sky_view_0 = sky_view_1;

#line 1286
    (&kernelContext_32)->volumetrics_0 = volumetrics_1;

#line 1286
    (&kernelContext_32)->lighting_0 = lighting_1;

    thread uint width_2;
    thread uint height_2;
    (*((&width_2)) = (scene_depth_1).get_width(0)),(*((&height_2)) = (scene_depth_1).get_height(0));
    float2 extent_3 = float2(float(width_2), float(height_2));
    int2 last_2 = int2(int(width_2) - int(1), int(height_2) - int(1));
    float2 _S183 = position_1.xy;

#line 1293
    int2 _S184 = int2(_S183);
    float surface_depth_0 = position_1.z;

    WaterMedium_natural_0 medium_0 = media_1[_S182.body_1];

    float3 up_3 = float3(0.0f, 1.0f, 0.0f);


    float3 incident_1 = normalize(_S182.world_position_8 - frame_1->camera_position_0.xyz);
    float _S185 = - incident_1.y;

#line 1302
    float cos_incident_1 = saturate(_S185);

#line 1309
    int3 _S186 = int3(_S184, int(0));

#line 1309
    float4 straight_0 = ((scene_color_1).read(vec<uint,2>(((_S186)).xy), uint(((_S186)).z)));

#line 1309
    float3 _S187 = water_opaque_point_0(_S184, ((scene_depth_1).read(vec<uint,2>(((_S186)).xy), uint(((_S186)).z))), extent_3, _S182.world_position_8, incident_1, &kernelContext_32);

#line 1319
    float3 refracted_0 = water_refract_0(incident_1, up_3, 0.75f);

    float4 landing_clip_0 = (((float4(_S182.world_position_8 + refracted_0 * float3((max(_S182.world_position_8.y - _S187.y, 0.0f) / max(- refracted_0.y, 9.99999997475242708e-07f))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));



    float _S188 = landing_clip_0.w;

#line 1325
    float3 behind_point_0;

#line 1325
    float4 behind_1;

#line 1325
    if(_S188 > 0.0f)
    {
        float2 landing_ndc_0 = landing_clip_0.xy / float2(_S188) ;
        int2 bent_0 = clamp(int2(float2(landing_ndc_0.x * 0.5f + 0.5f, 0.5f - landing_ndc_0.y * 0.5f) * extent_3), int2(int(0), int(0)), last_2);


        int3 _S189 = int3(bent_0, int(0));

#line 1331
        float bent_depth_0 = (((&kernelContext_32)->scene_depth_0).read(vec<uint,2>(((_S189)).xy), uint(((_S189)).z)));

#line 1337
        if(bent_depth_0 < surface_depth_0)
        {
            float4 _S190 = (((&kernelContext_32)->scene_color_0).read(vec<uint,2>(((_S189)).xy), uint(((_S189)).z)));

#line 1339
            float3 _S191 = water_opaque_point_0(bent_0, bent_depth_0, extent_3, _S182.world_position_8, refracted_0, &kernelContext_32);

#line 1339
            behind_point_0 = _S191;

#line 1339
            behind_1 = _S190;

#line 1337
        }
        else
        {

#line 1337
            behind_point_0 = _S187;

#line 1337
            behind_1 = straight_0;

#line 1337
        }

#line 1325
    }
    else
    {

#line 1325
        behind_point_0 = _S187;

#line 1325
        behind_1 = straight_0;

#line 1325
    }

#line 1345
    float thickness_0 = length(behind_point_0 - _S182.world_position_8);

#line 1345
    float4 _S192 = float4(medium_0.scattering_0) ;
    float3 extinction_0 = (float4(medium_0.absorption_0) ).xyz + _S192.xyz;
    float _S193 = extinction_0.x;
    float _S194 = extinction_0.y;
    float _S195 = extinction_0.z;

#line 1347
    float3 transmittance_0 = float3(fog_exp_neg_0(_S193 * thickness_0), fog_exp_neg_0(_S194 * thickness_0), fog_exp_neg_0(_S195 * thickness_0));

#line 1347
    float _S196;

#line 1355
    if(_S193 > 0.0f)
    {

#line 1355
        _S196 = _S192.x / _S193;

#line 1355
    }
    else
    {

#line 1355
        _S196 = 0.0f;

#line 1355
    }

#line 1355
    float _S197;
    if(_S194 > 0.0f)
    {

#line 1356
        _S197 = _S192.y / _S194;

#line 1356
    }
    else
    {

#line 1356
        _S197 = 0.0f;

#line 1356
    }

#line 1356
    float _S198;
    if(_S195 > 0.0f)
    {

#line 1357
        _S198 = _S192.z / _S195;

#line 1357
    }
    else
    {

#line 1357
        _S198 = 0.0f;

#line 1357
    }

#line 1355
    float3 albedo_0 = float3(_S196, _S197, _S198);


    float3 to_sun_0 = (&kernelContext_32)->water_0->sun_direction_1.xyz;


    float _S199 = to_sun_0.y;

#line 1359
    thread uint cascade_3;
    thread float cascade_fade_0;

#line 1360
    float _S200 = sun_visibility_0(_S182.world_position_8, to_sun_0, _S199, up_3, _S183, &cascade_3, &cascade_fade_0, &kernelContext_32);


    float3 _S201 = (&kernelContext_32)->frame_0->ambient_0.xyz;

#line 1363
    float3 _S202 = sky_irradiance_0(up_3, &kernelContext_32);

#line 1363
    float3 _S203 = float3(1.0f) ;

    float3 scattered_0 = albedo_0 * (_S201 + _S202 + (&kernelContext_32)->water_0->sun_color_0.xyz * float3((max(_S199, 0.0f) * _S200)) ) * (_S203 - transmittance_0);

#line 1372
    float3 mirrored_0 = float3(incident_1.x, _S185, incident_1.z);

#line 1372
    float3 _S204 = probe_environment_0(_S182.world_position_8, up_3, mirrored_0, &kernelContext_32);

#line 1372
    float3 _S205 = sky_environment_0(mirrored_0, 0.0f, 1.0f, &kernelContext_32);

    float3 environment_0 = _S204 + _S205;
    float grazing_0 = 1.0f - cos_incident_1;
    float grazing2_0 = grazing_0 * grazing_0;
    float fresnel_0 = 0.01999999955296516f + 0.98000001907348633f * (grazing2_0 * grazing2_0 * grazing_0);

#line 1377
    WaterAir_0 air_2;

#line 1388
    if(((&kernelContext_32)->water_0->froxels_0.x) != 0U)
    {

#line 1388
        WaterAir_0 _S206 = water_froxel_air_0(_S184, surface_depth_0, &kernelContext_32);

#line 1388
        air_2 = _S206;

#line 1388
    }
    else
    {

#line 1388
        WaterAir_0 _S207 = water_analytic_air_0(_S182.world_position_8, &kernelContext_32);

#line 1388
        air_2 = _S207;

#line 1388
    }

#line 1388
    float3 _S208 = float3((1.0f - fresnel_0)) ;

#line 1405
    float fade_1 = saturate(thickness_0 / 0.25f);

#line 1405
    pixelOutput_0 _S209 = { float4(straight_0.xyz * float3((1.0f - fade_1))  + (behind_1.xyz * transmittance_0 * _S208 + (environment_0 * float3(fresnel_0)  + scattered_0 * _S208) * float3(air_2.survives_0)  + air_2.inscatter_0 * (_S203 - transmittance_0 * _S208)) * float3(fade_1) , max(straight_0.w, fade_1)) };

#line 1410
    return _S209;
}


#line 1410
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_9 [[user(TEXCOORD)]];
    uint body_2 [[user(TEXCOORD_1)]];
};


#line 1262
struct SurfaceOutput_0
{
    float4 position_3;
    float3 world_position_10;
    [[flat]] uint body_3;
};


#line 1262
[[vertex]] vertexMain_Result_0 vertexMain(uint index_6 [[vertex_id]], WaterVertex_natural_0 device* water_vertices_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(1)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], WaterMedium_natural_0 device* media_2 [[buffer(5)]], texture2d<float, access::sample> scene_color_2 [[texture(0)]], VolumetricParams_natural_0 constant* params_2 [[buffer(3)]], WaterParams_0 constant* water_2 [[buffer(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(2)]], sampler shadow_sampler_2 [[sampler(0)]], SsrParams_natural_0 constant* camera_2 [[buffer(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(6)]], texture2d_array<float, access::sample> probe_visibility_2 [[texture(4)]], texture2d<float, access::sample> sky_prefilter_2 [[texture(3)]], packed_float4 device* sky_view_2 [[buffer(7)]], packed_float4 device* volumetrics_2 [[buffer(8)]], packed_float4 device* lighting_2 [[buffer(9)]])
{

#line 1262
    thread KernelContext_0 kernelContext_33;

#line 1262
    (&kernelContext_33)->water_vertices_0 = water_vertices_2;

#line 1262
    (&kernelContext_33)->frame_0 = frame_2;

#line 1262
    (&kernelContext_33)->scene_depth_0 = scene_depth_2;

#line 1262
    (&kernelContext_33)->media_0 = media_2;

#line 1262
    (&kernelContext_33)->scene_color_0 = scene_color_2;

#line 1262
    (&kernelContext_33)->params_0 = params_2;

#line 1262
    (&kernelContext_33)->water_0 = water_2;

#line 1262
    (&kernelContext_33)->shadow_atlas_0 = shadow_atlas_2;

#line 1262
    (&kernelContext_33)->shadow_sampler_0 = shadow_sampler_2;

#line 1262
    (&kernelContext_33)->camera_0 = camera_2;

#line 1262
    (&kernelContext_33)->probes_0 = probes_2;

#line 1262
    (&kernelContext_33)->probe_visibility_0 = probe_visibility_2;

#line 1262
    (&kernelContext_33)->sky_prefilter_0 = sky_prefilter_2;

#line 1262
    (&kernelContext_33)->sky_view_0 = sky_view_2;

#line 1262
    (&kernelContext_33)->volumetrics_0 = volumetrics_2;

#line 1262
    (&kernelContext_33)->lighting_0 = lighting_2;

#line 1277
    WaterVertex_natural_0 vertex_0 = water_vertices_2[index_6];
    thread SurfaceOutput_0 output_1;
    float3 _S210 = (float4(vertex_0.position_0) ).xyz;

#line 1279
    (&output_1)->world_position_10 = _S210;
    (&output_1)->position_3 = (((float4(_S210, 1.0f)) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_0[int(0)][int(0)], frame_2->view_proj_0.data_0[int(1)][int(0)], frame_2->view_proj_0.data_0[int(2)][int(0)], frame_2->view_proj_0.data_0[int(3)][int(0)], frame_2->view_proj_0.data_0[int(0)][int(1)], frame_2->view_proj_0.data_0[int(1)][int(1)], frame_2->view_proj_0.data_0[int(2)][int(1)], frame_2->view_proj_0.data_0[int(3)][int(1)], frame_2->view_proj_0.data_0[int(0)][int(2)], frame_2->view_proj_0.data_0[int(1)][int(2)], frame_2->view_proj_0.data_0[int(2)][int(2)], frame_2->view_proj_0.data_0[int(3)][int(2)], frame_2->view_proj_0.data_0[int(0)][int(3)], frame_2->view_proj_0.data_0[int(1)][int(3)], frame_2->view_proj_0.data_0[int(2)][int(3)], frame_2->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_1)->body_3 = (uint4(vertex_0.body_0) ).x;

#line 1281
    thread vertexMain_Result_0 _S211;

#line 1281
    (&_S211)->position_2 = output_1.position_3;

#line 1281
    (&_S211)->world_position_9 = output_1.world_position_10;

#line 1281
    (&_S211)->body_2 = output_1.body_3;

#line 1281
    return _S211;
}

