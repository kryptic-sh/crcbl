#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 293 "shaders/probe_gather.slang"
float sign_not_zero_0(float value_0)
{

#line 293
    float _S1;

    if(value_0 >= 0.0f)
    {

#line 295
        _S1 = 1.0f;

#line 295
    }
    else
    {

#line 295
        _S1 = -1.0f;

#line 295
    }

#line 295
    return _S1;
}


#line 303
float2 oct_encode_0(float3 direction_0)
{
    float _S2 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S2) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 306
    float2 p_1;
    if(_S2 < 0.0f)
    {
        float _S3 = p_0.y;

#line 309
        float _S4 = p_0.x;

#line 309
        p_1 = float2((1.0f - abs(_S3)) * sign_not_zero_0(_S4), (1.0f - abs(_S4)) * sign_not_zero_0(_S3));

#line 307
    }
    else
    {

#line 307
        p_1 = p_0;

#line 307
    }

#line 312
    return p_1;
}


#line 183
struct GatherParams_0
{
    float4 sun_color_0;
    float texel_area_0;
    uint rsm_side_0;
    uint probes_0;
    uint producers_0;
};


#line 183
struct PunctualProducer_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 axis_0;
    packed_uint4 tile_0;
};


#line 414
struct Bands_0
{
    float4 r_0;
    float4 g_0;
    float4 b_0;
};


#line 600
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


#line 317
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_0)
{

#line 317
    texture2d_array<float, access::sample> _S5 = kernelContext_0->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S5).get_width(0)),(*((&height_0)) = (_S5).get_height(0)),(*((&layers_0)) = (_S5).get_array_size());

#line 322
    float2 _S6 = float2(0.5f) ;

#line 322
    float2 _S7 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S6 + _S6) * float2(16.0f)  + _S7 - _S6;
    float2 _S8 = float2(float(width_0), float(height_0)) - _S7;

#line 326
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S8);
    float2 high_0 = min(low_0 + _S7, _S8);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S9 = int(low_0.x);

#line 331
    int _S10 = int(low_0.y);

#line 331
    int4 _S11 = int4(_S9, _S10, layer_0, int(0));
    int _S12 = int(high_0.x);

#line 332
    int4 _S13 = int4(_S12, _S10, layer_0, int(0));
    int _S14 = int(high_0.y);

#line 333
    int4 _S15 = int4(_S9, _S14, layer_0, int(0));
    int4 _S16 = int4(_S12, _S14, layer_0, int(0));
    float2 _S17 = float2(weight_0.x) ;

#line 335
    return mix(mix(((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z), uint(((_S11)).w))).xy, ((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z), uint(((_S13)).w))).xy, _S17), mix(((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S15)).xy), uint(((_S15)).z), uint(((_S15)).w))).xy, ((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z), uint(((_S16)).w))).xy, _S17), float2(weight_0.y) );
}


#line 344
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_0, float3 normal_0, KernelContext_0 thread* kernelContext_1)
{
    float3 to_probe_0 = probe_position_0 - (world_position_0 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 347
    float2 _S18 = probe_moments_0(index_1, - to_probe_0, kernelContext_1);

#line 353
    float _S19 = _S18.x;

#line 353
    float _S20 = max(_S18.y - _S19 * _S19, 0.0f);
    float behind_0 = to_surface_0 - _S19;
    float bound_0 = _S20 / (_S20 + behind_0 * behind_0);

#line 355
    float _S21;
    if(to_surface_0 <= _S19)
    {

#line 356
        _S21 = 1.0f;

#line 356
    }
    else
    {

#line 356
        _S21 = bound_0 * bound_0 * bound_0;

#line 356
    }

#line 356
    return _S21;
}


#line 427
void accumulate_0(Bands_0 thread* bands_0, float3 direction_2, float3 radiance_0, float solid_angle_0)
{
    float4 basis_0 = float4(direction_2 * float3((solid_angle_0 * 0.5f)) , solid_angle_0 * 0.25f);

    bands_0->r_0 = bands_0->r_0 + basis_0 * float4(radiance_0.x) ;
    bands_0->g_0 = bands_0->g_0 + basis_0 * float4(radiance_0.y) ;
    bands_0->b_0 = bands_0->b_0 + basis_0 * float4(radiance_0.z) ;
    return;
}


#line 444
void gather_patch_0(Bands_0 thread* bands_1, uint probe_0, float3 probe_position_1, float3 sample_position_0, float3 sample_normal_0, float3 radiance_1, float patch_area_0, KernelContext_0 thread* kernelContext_2)
{


    float3 offset_0 = sample_position_0 - probe_position_1;
    float distance_squared_0 = dot(offset_0, offset_0);



    if(distance_squared_0 <= 9.999999960041972e-13f)
    {
        return;
    }

    float3 direction_3 = offset_0 / float3(sqrt(distance_squared_0)) ;

#line 463
    float facing_0 = dot(sample_normal_0, - direction_3);
    if(facing_0 <= 0.0f)
    {
        return;
    }

#line 466
    float _S22 = probe_chebyshev_0(probe_0, probe_position_1, sample_position_0, sample_normal_0, kernelContext_2);

#line 472
    if(_S22 <= 0.0f)
    {
        return;
    }


    accumulate_0(bands_1, direction_3, radiance_1 * float3(_S22) , min(patch_area_0 * facing_0 / distance_squared_0, 6.28318548202514648f));
    return;
}


#line 400
float producer_tangent_0(const PunctualProducer_natural_0 thread* light_0)
{


    if(((uint4(light_0->tile_0) ).w) != 2U)
    {
        return 1.0f;
    }
    float _S23 = max((float4(light_0->color_0) ).w, 0.00100000004749745f);
    return sqrt(max(1.0f - _S23 * _S23, 0.0f)) / _S23;
}


#line 383
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_0)
{

    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_0 - cos_outer_0, 0.00009999999747379f));
}


#line 367
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


#line 490
[[kernel]] void computeMain(uint3 group_0 [[threadgroup_position_in_grid]], uint3 thread_0 [[thread_position_in_threadgroup]], GatherParams_0 constant* params_1 [[buffer(0)]], packed_float4 device* probe_positions_1 [[buffer(1)]], texture2d<float, access::sample> rsm_world_1 [[texture(3)]], texture2d<float, access::sample> rsm_normal_1 [[texture(2)]], texture2d<float, access::sample> rsm_albedo_1 [[texture(1)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(0)]], PunctualProducer_natural_0 device* producers_2 [[buffer(3)]], texture2d<float, access::sample> punctual_world_1 [[texture(6)]], texture2d<float, access::sample> punctual_normal_1 [[texture(5)]], texture2d<float, access::sample> punctual_albedo_1 [[texture(4)]], packed_float4 device* probes_2 [[buffer(2)]])
{

#line 490
    thread KernelContext_0 kernelContext_3;

#line 490
    (&kernelContext_3)->params_0 = params_1;

#line 490
    (&kernelContext_3)->probe_positions_0 = probe_positions_1;

#line 490
    (&kernelContext_3)->rsm_world_0 = rsm_world_1;

#line 490
    (&kernelContext_3)->rsm_normal_0 = rsm_normal_1;

#line 490
    (&kernelContext_3)->rsm_albedo_0 = rsm_albedo_1;

#line 490
    (&kernelContext_3)->probe_visibility_0 = probe_visibility_1;

#line 490
    (&kernelContext_3)->producers_1 = producers_2;

#line 490
    (&kernelContext_3)->punctual_world_0 = punctual_world_1;

#line 490
    (&kernelContext_3)->punctual_normal_0 = punctual_normal_1;

#line 490
    (&kernelContext_3)->punctual_albedo_0 = punctual_albedo_1;

#line 490
    (&kernelContext_3)->probes_1 = probes_2;

#line 490
    threadgroup array<Bands_0, int(64)> tile_2;

#line 490
    (&kernelContext_3)->tile_1 = &tile_2;

#line 496
    uint probe_1 = group_0.x;
    uint lane_0 = thread_0.x;

    thread Bands_0 bands_2;
    float4 _S24 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 500
    (&bands_2)->r_0 = _S24;
    (&bands_2)->g_0 = _S24;
    (&bands_2)->b_0 = _S24;

#line 502
    uint stride_0;

    if(probe_1 < (params_1->probes_0))
    {
        float3 _S25 = (float4(*((&kernelContext_3)->probe_positions_0+probe_1)) ).xyz;


        uint _S26 = max((&kernelContext_3)->params_0->rsm_side_0, 1U);
        uint _S27 = _S26 * _S26;

#line 510
        stride_0 = lane_0;
        for(;;)
        {

#line 511
            if(stride_0 < _S27)
            {
            }
            else
            {

#line 511
                break;
            }
            uint row_0 = stride_0 / _S26;

            int3 at_0 = int3(int(stride_0 - row_0 * _S26), int(row_0), int(0));

            float4 world_0 = (((&kernelContext_3)->rsm_world_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));



            if((world_0.w) <= 0.0f)
            {
                stride_0 = stride_0 + 64U;

#line 511
                continue;
            }

#line 511
            gather_patch_0(&bands_2, probe_1, _S25, world_0.xyz, normalize((((&kernelContext_3)->rsm_normal_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z))).xyz * float3(2.0f)  - float3(1.0f) ), (((&kernelContext_3)->rsm_albedo_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z))).xyz * (&kernelContext_3)->params_0->sun_color_0.xyz * float3(0.31830987334251404f) , (&kernelContext_3)->params_0->texel_area_0, &kernelContext_3);

#line 511
            stride_0 = stride_0 + 64U;

#line 511
        }

#line 511
        uint producer_0 = 0U;

#line 543
        for(;;)
        {

#line 543
            if(producer_0 < ((&kernelContext_3)->params_0->producers_0))
            {
            }
            else
            {

#line 543
                break;
            }
            PunctualProducer_natural_0 light_1 = (&kernelContext_3)->producers_1[producer_0];

#line 545
            thread PunctualProducer_natural_0 _S28 = light_1;

#line 545
            uint4 _S29 = uint4((&_S28)->tile_0) ;
            uint _S30 = max(_S29.z, 1U);
            uint _S31 = _S30 * _S30;

#line 547
            _S28 = light_1;

#line 547
            float _S32 = producer_tangent_0(&_S28);

#line 552
            float _S33 = float(_S30);

#line 552
            float _S34 = 2.0f * _S32 / _S33;

#line 552
            stride_0 = lane_0;
            for(;;)
            {

#line 553
                if(stride_0 < _S31)
                {
                }
                else
                {

#line 553
                    break;
                }
                uint row_1 = stride_0 / _S30;
                uint column_0 = stride_0 - row_1 * _S30;
                int3 at_1 = int3(int(_S29.x + column_0), int(_S29.y + row_1), int(0));

                float4 world_1 = (((&kernelContext_3)->punctual_world_0).read(vec<uint,2>(((at_1)).xy), uint(((at_1)).z)));
                if((world_1.w) <= 0.0f)
                {
                    stride_0 = stride_0 + 64U;

#line 553
                    continue;
                }

#line 564
                float3 sample_normal_1 = normalize((((&kernelContext_3)->punctual_normal_0).read(vec<uint,2>(((at_1)).xy), uint(((at_1)).z))).xyz * float3(2.0f)  - float3(1.0f) );

#line 564
                float4 _S35 = float4((&_S28)->position_0) ;

                float3 _S36 = world_1.xyz;

#line 566
                float3 to_light_1 = _S35.xyz - _S36;
                float to_light_distance_0 = length(to_light_1);


                if(to_light_distance_0 <= 9.99999997475242708e-07f)
                {
                    stride_0 = stride_0 + 64U;

#line 553
                    continue;
                }

#line 574
                float3 to_light_2 = to_light_1 / float3(to_light_distance_0) ;

#line 574
                float cone_0;


                if((_S29.w) == 2U)
                {

#line 577
                    float4 _S37 = float4((&_S28)->axis_0) ;

#line 577
                    cone_0 = spot_cone_0(to_light_2, _S37.xyz, (float4((&_S28)->color_0) ).w, _S37.w);

#line 577
                }
                else
                {

#line 577
                    cone_0 = 1.0f;

#line 577
                }

#line 583
                float u_0 = (2.0f * (float(column_0) + 0.5f) / _S33 - 1.0f) * _S32;
                float v_0 = (2.0f * (float(row_1) + 0.5f) / _S33 - 1.0f) * _S32;
                float axial_0 = u_0 * u_0 + v_0 * v_0 + 1.0f;

#line 585
                gather_patch_0(&bands_2, probe_1, _S25, _S36, sample_normal_1, (((&kernelContext_3)->punctual_albedo_0).read(vec<uint,2>(((at_1)).xy), uint(((at_1)).z))).xyz * (float4((&_S28)->color_0) ).xyz * float3(cone_0)  * float3(0.31830987334251404f) , _S34 * _S34 / (axial_0 * sqrt(axial_0)) * to_light_distance_0 * to_light_distance_0 * punctual_falloff_0(to_light_distance_0, _S35.w), &kernelContext_3);

#line 553
                stride_0 = stride_0 + 64U;

#line 553
            }

#line 543
            producer_0 = producer_0 + 1U;

#line 543
        }

#line 504
    }

#line 600
    (*(&kernelContext_3)->tile_1)[lane_0] = bands_2;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 601
    stride_0 = 32U;


    for(;;)
    {

#line 604
        if(stride_0 > 0U)
        {
        }
        else
        {

#line 604
            break;
        }
        if(lane_0 < stride_0)
        {
            (&(*(&kernelContext_3)->tile_1)[lane_0])->r_0 = (&(*(&kernelContext_3)->tile_1)[lane_0])->r_0 + (&(*(&kernelContext_3)->tile_1)[lane_0 + stride_0])->r_0;
            (&(*(&kernelContext_3)->tile_1)[lane_0])->g_0 = (&(*(&kernelContext_3)->tile_1)[lane_0])->g_0 + (&(*(&kernelContext_3)->tile_1)[lane_0 + stride_0])->g_0;
            (&(*(&kernelContext_3)->tile_1)[lane_0])->b_0 = (&(*(&kernelContext_3)->tile_1)[lane_0])->b_0 + (&(*(&kernelContext_3)->tile_1)[lane_0 + stride_0])->b_0;

#line 606
        }

#line 612
        threadgroup_barrier(mem_flags::mem_threadgroup);

#line 604
        stride_0 = stride_0 >> 1U;

#line 604
    }

#line 604
    bool _S38;

#line 615
    if(lane_0 == 0U)
    {

#line 615
        _S38 = probe_1 < (params_1->probes_0);

#line 615
    }
    else
    {

#line 615
        _S38 = false;

#line 615
    }

#line 615
    if(_S38)
    {
        uint _S39 = probe_1 * 3U;

#line 617
        *((&kernelContext_3)->probes_1+_S39) = packed_float4((&(*(&kernelContext_3)->tile_1)[int(0)])->r_0) ;

#line 617
        *((&kernelContext_3)->probes_1+(_S39 + 1U)) = packed_float4((&(*(&kernelContext_3)->tile_1)[int(0)])->g_0) ;

#line 617
        *((&kernelContext_3)->probes_1+(_S39 + 2U)) = packed_float4((&(*(&kernelContext_3)->tile_1)[int(0)])->b_0) ;

#line 615
    }

#line 621
    return;
}

