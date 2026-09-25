#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 305 "shaders/probe_gather.slang"
float sign_not_zero_0(float value_0)
{

#line 305
    float _S1;

    if(value_0 >= 0.0f)
    {

#line 307
        _S1 = 1.0f;

#line 307
    }
    else
    {

#line 307
        _S1 = -1.0f;

#line 307
    }

#line 307
    return _S1;
}


#line 315
float2 oct_encode_0(float3 direction_0)
{
    float _S2 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S2) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 318
    float2 p_1;
    if(_S2 < 0.0f)
    {
        float _S3 = p_0.y;

#line 321
        float _S4 = p_0.x;

#line 321
        p_1 = float2((1.0f - abs(_S3)) * sign_not_zero_0(_S4), (1.0f - abs(_S4)) * sign_not_zero_0(_S3));

#line 319
    }
    else
    {

#line 319
        p_1 = p_0;

#line 319
    }

#line 324
    return p_1;
}


#line 195
struct GatherParams_0
{
    float4 sun_color_0;
    float texel_area_0;
    uint rsm_side_0;
    uint probes_0;
    uint producers_0;
};


#line 297
struct PunctualProducer_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 axis_0;
    packed_uint4 tile_0;
};


#line 426
struct Bands_0
{
    float4 r_0;
    float4 g_0;
    float4 b_0;
};


#line 612
struct KernelContext_0
{
    GatherParams_0 constant* params_0;
    packed_float4 device* probe_positions_0;
    texture2d<float, access::sample> rsm_world_0;
    texture2d<float, access::sample> rsm_normal_0;
    texture2d<float, access::sample> rsm_albedo_0;
    texture2d_array<float, access::sample> probe_visibility_0;
    PunctualProducer_natural_0 device* producers_1;
    texture2d<float, access::sample> punctual_world_0;
    texture2d<float, access::sample> punctual_normal_0;
    texture2d<float, access::sample> punctual_albedo_0;
    packed_float4 device* probes_1;
    array<Bands_0, int(64)> threadgroup* tile_1;
};


#line 329
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_0)
{

#line 329
    texture2d_array<float, access::sample> _S5 = kernelContext_0->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S5).get_width(0)),(*((&height_0)) = (_S5).get_height(0)),(*((&layers_0)) = (_S5).get_array_size());

#line 334
    float2 _S6 = float2(0.5f) ;

#line 334
    float2 _S7 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S6 + _S6) * float2(16.0f)  + _S7 - _S6;
    float2 _S8 = float2(float(width_0), float(height_0)) - _S7;

#line 338
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S8);
    float2 high_0 = min(low_0 + _S7, _S8);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S9 = int(low_0.x);

#line 343
    int _S10 = int(low_0.y);

#line 343
    int4 _S11 = int4(_S9, _S10, layer_0, int(0));
    int _S12 = int(high_0.x);

#line 344
    int4 _S13 = int4(_S12, _S10, layer_0, int(0));
    int _S14 = int(high_0.y);

#line 345
    int4 _S15 = int4(_S9, _S14, layer_0, int(0));
    int4 _S16 = int4(_S12, _S14, layer_0, int(0));
    float2 _S17 = float2(weight_0.x) ;

#line 347
    return mix(mix(((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z), uint(((_S11)).w))).xy, ((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z), uint(((_S13)).w))).xy, _S17), mix(((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S15)).xy), uint(((_S15)).z), uint(((_S15)).w))).xy, ((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z), uint(((_S16)).w))).xy, _S17), float2(weight_0.y) );
}


#line 356
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_0, float3 normal_0, KernelContext_0 thread* kernelContext_1)
{
    float3 to_probe_0 = probe_position_0 - (world_position_0 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 359
    float2 _S18 = probe_moments_0(index_1, - to_probe_0, kernelContext_1);

#line 365
    float _S19 = _S18.x;

#line 365
    float _S20 = max(_S18.y - _S19 * _S19, 0.0f);
    float behind_0 = to_surface_0 - _S19;
    float bound_0 = _S20 / (_S20 + behind_0 * behind_0);

#line 367
    float _S21;
    if(to_surface_0 <= _S19)
    {

#line 368
        _S21 = 1.0f;

#line 368
    }
    else
    {

#line 368
        _S21 = bound_0 * bound_0 * bound_0;

#line 368
    }

#line 368
    return _S21;
}


#line 439
void accumulate_0(Bands_0 thread* bands_0, float3 direction_2, float3 radiance_0, float solid_angle_0)
{
    float4 basis_0 = float4(direction_2 * float3((solid_angle_0 * 0.5f)) , solid_angle_0 * 0.25f);

    bands_0->r_0 = bands_0->r_0 + basis_0 * float4(radiance_0.x) ;
    bands_0->g_0 = bands_0->g_0 + basis_0 * float4(radiance_0.y) ;
    bands_0->b_0 = bands_0->b_0 + basis_0 * float4(radiance_0.z) ;
    return;
}


#line 456
void gather_patch_0(Bands_0 thread* bands_1, uint probe_0, float3 probe_position_1, float3 sample_position_0, float3 sample_normal_0, float3 radiance_1, float patch_area_0, KernelContext_0 thread* kernelContext_2)
{


    float3 offset_0 = sample_position_0 - probe_position_1;
    float distance_squared_0 = dot(offset_0, offset_0);



    if(distance_squared_0 <= 9.999999960041972e-13f)
    {
        return;
    }

    float3 direction_3 = offset_0 / float3(sqrt(distance_squared_0)) ;

#line 475
    float facing_0 = dot(sample_normal_0, - direction_3);
    if(facing_0 <= 0.0f)
    {
        return;
    }

#line 478
    float _S22 = probe_chebyshev_0(probe_0, probe_position_1, sample_position_0, sample_normal_0, kernelContext_2);

#line 484
    if(_S22 <= 0.0f)
    {
        return;
    }


    accumulate_0(bands_1, direction_3, radiance_1 * float3(_S22) , min(patch_area_0 * facing_0 / distance_squared_0, 6.28318548202514648f));
    return;
}


#line 412
float producer_tangent_0(const PunctualProducer_natural_0 thread* light_0)
{


    if(((uint4(light_0->tile_0) ).w) != 2U)
    {
        return 1.0f;
    }
    float _S23 = max((float4(light_0->color_0) ).w, 0.00100000004749745f);
    return sqrt(max(1.0f - _S23 * _S23, 0.0f)) / _S23;
}


#line 395
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_0)
{

    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_0 - cos_outer_0, 0.00009999999747379f));
}


#line 379
float range_window_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}



float punctual_falloff_0(float distance_1, float radius_1)
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}


#line 502
[[kernel]] void computeMain(uint3 group_0 [[threadgroup_position_in_grid]], uint3 thread_0 [[thread_position_in_threadgroup]], GatherParams_0 constant* params_1 [[buffer(0)]], packed_float4 device* probe_positions_1 [[buffer(1)]], texture2d<float, access::sample> rsm_world_1 [[texture(3)]], texture2d<float, access::sample> rsm_normal_1 [[texture(2)]], texture2d<float, access::sample> rsm_albedo_1 [[texture(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(0)]], PunctualProducer_natural_0 device* producers_2 [[buffer(3)]], texture2d<float, access::sample> punctual_world_1 [[texture(6)]], texture2d<float, access::sample> punctual_normal_1 [[texture(5)]], texture2d<float, access::sample> punctual_albedo_1 [[texture(4)]], packed_float4 device* probes_2 [[buffer(2)]])
{

#line 502
    thread KernelContext_0 kernelContext_3;

#line 502
    (&kernelContext_3)->params_0 = params_1;

#line 502
    (&kernelContext_3)->probe_positions_0 = probe_positions_1;

#line 502
    (&kernelContext_3)->rsm_world_0 = rsm_world_1;

#line 502
    (&kernelContext_3)->rsm_normal_0 = rsm_normal_1;

#line 502
    (&kernelContext_3)->rsm_albedo_0 = rsm_albedo_1;

#line 502
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_1;

#line 502
    (&kernelContext_3)->producers_1 = producers_2;

#line 502
    (&kernelContext_3)->punctual_world_0 = punctual_world_1;

#line 502
    (&kernelContext_3)->punctual_normal_0 = punctual_normal_1;

#line 502
    (&kernelContext_3)->punctual_albedo_0 = punctual_albedo_1;

#line 502
    (&kernelContext_3)->probes_1 = probes_2;

#line 502
    threadgroup array<Bands_0, int(64)> tile_2;

#line 502
    (&kernelContext_3)->tile_1 = &tile_2;

#line 508
    uint probe_1 = group_0.x;
    uint lane_0 = thread_0.x;

    thread Bands_0 bands_2;
    float4 _S24 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 512
    (&bands_2)->r_0 = _S24;
    (&bands_2)->g_0 = _S24;
    (&bands_2)->b_0 = _S24;

#line 514
    uint stride_0;

    if(probe_1 < (params_1->probes_0))
    {
        float3 _S25 = (float4(*((&kernelContext_3)->probe_positions_0+probe_1)) ).xyz;


        uint _S26 = max((&kernelContext_3)->params_0->rsm_side_0, 1U);
        uint _S27 = _S26 * _S26;

#line 522
        stride_0 = lane_0;
        for(;;)
        {

#line 523
            if(stride_0 < _S27)
            {
            }
            else
            {

#line 523
                break;
            }
            uint row_0 = stride_0 / _S26;

            int3 at_0 = int3(int(stride_0 - row_0 * _S26), int(row_0), int(0));

            float4 world_0 = (((&kernelContext_3)->rsm_world_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));



            if((world_0.w) <= 0.0f)
            {
                stride_0 = stride_0 + 64U;

#line 523
                continue;
            }

#line 523
            gather_patch_0(&bands_2, probe_1, _S25, world_0.xyz, normalize((((&kernelContext_3)->rsm_normal_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z))).xyz * float3(2.0f)  - float3(1.0f) ), (((&kernelContext_3)->rsm_albedo_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z))).xyz * (&kernelContext_3)->params_0->sun_color_0.xyz * float3(0.31830987334251404f) , (&kernelContext_3)->params_0->texel_area_0, &kernelContext_3);

#line 523
            stride_0 = stride_0 + 64U;

#line 523
        }

#line 523
        uint producer_0 = 0U;

#line 555
        for(;;)
        {

#line 555
            if(producer_0 < ((&kernelContext_3)->params_0->producers_0))
            {
            }
            else
            {

#line 555
                break;
            }
            PunctualProducer_natural_0 light_1 = (&kernelContext_3)->producers_1[producer_0];

#line 557
            thread PunctualProducer_natural_0 _S28 = light_1;

#line 557
            uint4 _S29 = uint4((&_S28)->tile_0) ;
            uint _S30 = max(_S29.z, 1U);
            uint _S31 = _S30 * _S30;

#line 559
            _S28 = light_1;

#line 559
            float _S32 = producer_tangent_0(&_S28);

#line 564
            float _S33 = float(_S30);

#line 564
            float _S34 = 2.0f * _S32 / _S33;

#line 564
            stride_0 = lane_0;
            for(;;)
            {

#line 565
                if(stride_0 < _S31)
                {
                }
                else
                {

#line 565
                    break;
                }
                uint row_1 = stride_0 / _S30;
                uint column_0 = stride_0 - row_1 * _S30;
                int3 at_1 = int3(int(_S29.x + column_0), int(_S29.y + row_1), int(0));

                float4 world_1 = (((&kernelContext_3)->punctual_world_0).read(vec<uint,2>(((at_1)).xy), uint(((at_1)).z)));
                if((world_1.w) <= 0.0f)
                {
                    stride_0 = stride_0 + 64U;

#line 565
                    continue;
                }

#line 576
                float3 sample_normal_1 = normalize((((&kernelContext_3)->punctual_normal_0).read(vec<uint,2>(((at_1)).xy), uint(((at_1)).z))).xyz * float3(2.0f)  - float3(1.0f) );

#line 576
                float4 _S35 = float4((&_S28)->position_0) ;

                float3 _S36 = world_1.xyz;

#line 578
                float3 to_light_1 = _S35.xyz - _S36;
                float to_light_distance_0 = length(to_light_1);


                if(to_light_distance_0 <= 9.99999997475242708e-07f)
                {
                    stride_0 = stride_0 + 64U;

#line 565
                    continue;
                }

#line 586
                float3 to_light_2 = to_light_1 / float3(to_light_distance_0) ;

#line 586
                float cone_0;


                if((_S29.w) == 2U)
                {

#line 589
                    float4 _S37 = float4((&_S28)->axis_0) ;

#line 589
                    cone_0 = spot_cone_0(to_light_2, _S37.xyz, (float4((&_S28)->color_0) ).w, _S37.w);

#line 589
                }
                else
                {

#line 589
                    cone_0 = 1.0f;

#line 589
                }

#line 595
                float u_0 = (2.0f * (float(column_0) + 0.5f) / _S33 - 1.0f) * _S32;
                float v_0 = (2.0f * (float(row_1) + 0.5f) / _S33 - 1.0f) * _S32;
                float axial_0 = u_0 * u_0 + v_0 * v_0 + 1.0f;

#line 597
                gather_patch_0(&bands_2, probe_1, _S25, _S36, sample_normal_1, (((&kernelContext_3)->punctual_albedo_0).read(vec<uint,2>(((at_1)).xy), uint(((at_1)).z))).xyz * (float4((&_S28)->color_0) ).xyz * float3(cone_0)  * float3(0.31830987334251404f) , _S34 * _S34 / (axial_0 * sqrt(axial_0)) * to_light_distance_0 * to_light_distance_0 * punctual_falloff_0(to_light_distance_0, _S35.w), &kernelContext_3);

#line 565
                stride_0 = stride_0 + 64U;

#line 565
            }

#line 555
            producer_0 = producer_0 + 1U;

#line 555
        }

#line 516
    }

#line 612
    (*(&kernelContext_3)->tile_1)[lane_0] = bands_2;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 613
    stride_0 = 32U;


    for(;;)
    {

#line 616
        if(stride_0 > 0U)
        {
        }
        else
        {

#line 616
            break;
        }
        if(lane_0 < stride_0)
        {
            (&(*(&kernelContext_3)->tile_1)[lane_0])->r_0 = (&(*(&kernelContext_3)->tile_1)[lane_0])->r_0 + (&(*(&kernelContext_3)->tile_1)[lane_0 + stride_0])->r_0;
            (&(*(&kernelContext_3)->tile_1)[lane_0])->g_0 = (&(*(&kernelContext_3)->tile_1)[lane_0])->g_0 + (&(*(&kernelContext_3)->tile_1)[lane_0 + stride_0])->g_0;
            (&(*(&kernelContext_3)->tile_1)[lane_0])->b_0 = (&(*(&kernelContext_3)->tile_1)[lane_0])->b_0 + (&(*(&kernelContext_3)->tile_1)[lane_0 + stride_0])->b_0;

#line 618
        }

#line 624
        threadgroup_barrier(mem_flags::mem_threadgroup);

#line 616
        stride_0 = stride_0 >> 1U;

#line 616
    }

#line 616
    bool _S38;

#line 627
    if(lane_0 == 0U)
    {

#line 627
        _S38 = probe_1 < (params_1->probes_0);

#line 627
    }
    else
    {

#line 627
        _S38 = false;

#line 627
    }

#line 627
    if(_S38)
    {
        uint _S39 = probe_1 * 3U;

#line 629
        *((&kernelContext_3)->probes_1+_S39) = packed_float4((&(*(&kernelContext_3)->tile_1)[int(0)])->r_0) ;

#line 629
        *((&kernelContext_3)->probes_1+(_S39 + 1U)) = packed_float4((&(*(&kernelContext_3)->tile_1)[int(0)])->g_0) ;

#line 629
        *((&kernelContext_3)->probes_1+(_S39 + 2U)) = packed_float4((&(*(&kernelContext_3)->tile_1)[int(0)])->b_0) ;

#line 627
    }

#line 633
    return;
}

