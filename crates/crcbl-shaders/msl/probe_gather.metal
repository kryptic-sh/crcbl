#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 202 "shaders/probe_gather.slang"
float sign_not_zero_0(float value_0)
{

#line 202
    float _S1;

    if(value_0 >= 0.0f)
    {

#line 204
        _S1 = 1.0f;

#line 204
    }
    else
    {

#line 204
        _S1 = -1.0f;

#line 204
    }

#line 204
    return _S1;
}


#line 212
float2 oct_encode_0(float3 direction_0)
{
    float _S2 = direction_0.y;
    float2 p_0 = direction_0.xz / float2(max(abs(direction_0.x) + abs(_S2) + abs(direction_0.z), 9.99999968265522539e-21f)) ;

#line 215
    float2 p_1;
    if(_S2 < 0.0f)
    {
        float _S3 = p_0.y;

#line 218
        float _S4 = p_0.x;

#line 218
        p_1 = float2((1.0f - abs(_S3)) * sign_not_zero_0(_S4), (1.0f - abs(_S4)) * sign_not_zero_0(_S3));

#line 216
    }
    else
    {

#line 216
        p_1 = p_0;

#line 216
    }

#line 221
    return p_1;
}


#line 133
struct GatherParams_0
{
    float4 sun_color_0;
    float texel_area_0;
    uint rsm_side_0;
    uint probes_0;
    uint reserved_0;
};


#line 270
struct Bands_0
{
    float4 r_0;
    float4 g_0;
    float4 b_0;
};


#line 380
struct KernelContext_0
{
    GatherParams_0 constant* params_0;
    packed_float4 device* probe_positions_0;
    texture2d<float, access::sample> rsm_world_0;
    texture2d<float, access::sample> rsm_normal_0;
    texture2d_array<float, access::sample> probe_visibility_0;
    texture2d<float, access::sample> rsm_albedo_0;
    packed_float4 device* probes_1;
    array<Bands_0, int(64)> threadgroup* tile_0;
};


#line 226
float2 probe_moments_0(uint index_0, float3 direction_1, KernelContext_0 thread* kernelContext_0)
{

#line 226
    texture2d_array<float, access::sample> _S5 = kernelContext_0->probe_visibility_0;

    thread uint width_0;
    thread uint height_0;
    thread uint layers_0;
    (*((&width_0)) = (_S5).get_width(0)),(*((&height_0)) = (_S5).get_height(0)),(*((&layers_0)) = (_S5).get_array_size());

#line 231
    float2 _S6 = float2(0.5f) ;

#line 231
    float2 _S7 = float2(1.0f) ;


    float2 scaled_0 = (oct_encode_0(direction_1) * _S6 + _S6) * float2(16.0f)  + _S7 - _S6;
    float2 _S8 = float2(float(width_0), float(height_0)) - _S7;

#line 235
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S8);
    float2 high_0 = min(low_0 + _S7, _S8);
    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );
    int layer_0 = int(min(index_0, max(layers_0, 1U) - 1U));

    int _S9 = int(low_0.x);

#line 240
    int _S10 = int(low_0.y);

#line 240
    int4 _S11 = int4(_S9, _S10, layer_0, int(0));
    int _S12 = int(high_0.x);

#line 241
    int4 _S13 = int4(_S12, _S10, layer_0, int(0));
    int _S14 = int(high_0.y);

#line 242
    int4 _S15 = int4(_S9, _S14, layer_0, int(0));
    int4 _S16 = int4(_S12, _S14, layer_0, int(0));
    float2 _S17 = float2(weight_0.x) ;

#line 244
    return mix(mix(((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z), uint(((_S11)).w))).xy, ((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z), uint(((_S13)).w))).xy, _S17), mix(((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S15)).xy), uint(((_S15)).z), uint(((_S15)).w))).xy, ((kernelContext_0->probe_visibility_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z), uint(((_S16)).w))).xy, _S17), float2(weight_0.y) );
}


#line 253
float probe_chebyshev_0(uint index_1, float3 probe_position_0, float3 world_position_0, float3 normal_0, KernelContext_0 thread* kernelContext_1)
{
    float3 to_probe_0 = probe_position_0 - (world_position_0 + normal_0 * float3(0.05000000074505806f) );
    float to_surface_0 = length(to_probe_0);

#line 256
    float2 _S18 = probe_moments_0(index_1, - to_probe_0, kernelContext_1);

#line 262
    float _S19 = _S18.x;

#line 262
    float _S20 = max(_S18.y - _S19 * _S19, 0.0f);
    float behind_0 = to_surface_0 - _S19;
    float bound_0 = _S20 / (_S20 + behind_0 * behind_0);

#line 264
    float _S21;
    if(to_surface_0 <= _S19)
    {

#line 265
        _S21 = 1.0f;

#line 265
    }
    else
    {

#line 265
        _S21 = bound_0 * bound_0 * bound_0;

#line 265
    }

#line 265
    return _S21;
}


#line 283
void accumulate_0(Bands_0 thread* bands_0, float3 direction_2, float3 radiance_0, float solid_angle_0)
{
    float4 basis_0 = float4(direction_2 * float3((solid_angle_0 * 0.5f)) , solid_angle_0 * 0.25f);

    bands_0->r_0 = bands_0->r_0 + basis_0 * float4(radiance_0.x) ;
    bands_0->g_0 = bands_0->g_0 + basis_0 * float4(radiance_0.y) ;
    bands_0->b_0 = bands_0->b_0 + basis_0 * float4(radiance_0.z) ;
    return;
}


#line 301
[[kernel]] void computeMain(uint3 group_0 [[threadgroup_position_in_grid]], uint3 thread_0 [[thread_position_in_threadgroup]], GatherParams_0 constant* params_1 [[buffer(0)]], packed_float4 device* probe_positions_1 [[buffer(1)]], texture2d<float, access::sample> rsm_world_1 [[texture(3)]], texture2d<float, access::sample> rsm_normal_1 [[texture(2)]], texture2d_array<float, access::sample> probe_visibility_1 [[texture(0)]], texture2d<float, access::sample> rsm_albedo_1 [[texture(1)]], packed_float4 device* probes_2 [[buffer(2)]])
{

#line 301
    thread KernelContext_0 kernelContext_2;

#line 301
    (&kernelContext_2)->params_0 = params_1;

#line 301
    (&kernelContext_2)->probe_positions_0 = probe_positions_1;

#line 301
    (&kernelContext_2)->rsm_world_0 = rsm_world_1;

#line 301
    (&kernelContext_2)->rsm_normal_0 = rsm_normal_1;

#line 301
    (&kernelContext_2)->probe_visibility_0 = probe_visibility_1;

#line 301
    (&kernelContext_2)->rsm_albedo_0 = rsm_albedo_1;

#line 301
    (&kernelContext_2)->probes_1 = probes_2;

#line 301
    threadgroup array<Bands_0, int(64)> tile_1;

#line 301
    (&kernelContext_2)->tile_0 = &tile_1;

#line 307
    uint probe_0 = group_0.x;
    uint lane_0 = thread_0.x;

    thread Bands_0 bands_1;
    float4 _S22 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 311
    (&bands_1)->r_0 = _S22;
    (&bands_1)->g_0 = _S22;
    (&bands_1)->b_0 = _S22;

#line 313
    uint stride_0;

    if(probe_0 < (params_1->probes_0))
    {
        float3 _S23 = (float4(*((&kernelContext_2)->probe_positions_0+probe_0)) ).xyz;
        uint _S24 = max((&kernelContext_2)->params_0->rsm_side_0, 1U);
        uint _S25 = _S24 * _S24;

#line 319
        stride_0 = lane_0;
        for(;;)
        {

#line 320
            if(stride_0 < _S25)
            {
            }
            else
            {

#line 320
                break;
            }
            uint row_0 = stride_0 / _S24;

            int3 at_0 = int3(int(stride_0 - row_0 * _S24), int(row_0), int(0));

            float4 world_0 = (((&kernelContext_2)->rsm_world_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z)));



            if((world_0.w) <= 0.0f)
            {
                stride_0 = stride_0 + 64U;

#line 320
                continue;
            }

#line 334
            float3 sample_position_0 = world_0.xyz;



            float3 sample_normal_0 = normalize((((&kernelContext_2)->rsm_normal_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z))).xyz * float3(2.0f)  - float3(1.0f) );

            float3 offset_0 = sample_position_0 - _S23;
            float distance_squared_0 = dot(offset_0, offset_0);



            if(distance_squared_0 <= 9.999999960041972e-13f)
            {
                stride_0 = stride_0 + 64U;

#line 320
                continue;
            }

#line 350
            float3 direction_3 = offset_0 / float3(sqrt(distance_squared_0)) ;

#line 355
            float facing_0 = dot(sample_normal_0, - direction_3);
            if(facing_0 <= 0.0f)
            {
                stride_0 = stride_0 + 64U;

#line 320
                continue;
            }

#line 320
            float _S26 = probe_chebyshev_0(probe_0, _S23, sample_position_0, sample_normal_0, &kernelContext_2);

#line 365
            if(_S26 <= 0.0f)
            {
                stride_0 = stride_0 + 64U;

#line 320
                continue;
            }

#line 376
            accumulate_0(&bands_1, direction_3, (((&kernelContext_2)->rsm_albedo_0).read(vec<uint,2>(((at_0)).xy), uint(((at_0)).z))).xyz * (&kernelContext_2)->params_0->sun_color_0.xyz * float3(0.31830987334251404f)  * float3(_S26) , min((&kernelContext_2)->params_0->texel_area_0 * facing_0 / distance_squared_0, 6.28318548202514648f));

#line 320
            stride_0 = stride_0 + 64U;

#line 320
        }

#line 315
    }

#line 380
    (*(&kernelContext_2)->tile_0)[lane_0] = bands_1;
    threadgroup_barrier(mem_flags::mem_threadgroup);

#line 381
    stride_0 = 32U;


    for(;;)
    {

#line 384
        if(stride_0 > 0U)
        {
        }
        else
        {

#line 384
            break;
        }
        if(lane_0 < stride_0)
        {
            (&(*(&kernelContext_2)->tile_0)[lane_0])->r_0 = (&(*(&kernelContext_2)->tile_0)[lane_0])->r_0 + (&(*(&kernelContext_2)->tile_0)[lane_0 + stride_0])->r_0;
            (&(*(&kernelContext_2)->tile_0)[lane_0])->g_0 = (&(*(&kernelContext_2)->tile_0)[lane_0])->g_0 + (&(*(&kernelContext_2)->tile_0)[lane_0 + stride_0])->g_0;
            (&(*(&kernelContext_2)->tile_0)[lane_0])->b_0 = (&(*(&kernelContext_2)->tile_0)[lane_0])->b_0 + (&(*(&kernelContext_2)->tile_0)[lane_0 + stride_0])->b_0;

#line 386
        }

#line 392
        threadgroup_barrier(mem_flags::mem_threadgroup);

#line 384
        stride_0 = stride_0 >> 1U;

#line 384
    }

#line 384
    bool _S27;

#line 395
    if(lane_0 == 0U)
    {

#line 395
        _S27 = probe_0 < (params_1->probes_0);

#line 395
    }
    else
    {

#line 395
        _S27 = false;

#line 395
    }

#line 395
    if(_S27)
    {
        uint _S28 = probe_0 * 3U;

#line 397
        *((&kernelContext_2)->probes_1+_S28) = packed_float4((&(*(&kernelContext_2)->tile_0)[int(0)])->r_0) ;

#line 397
        *((&kernelContext_2)->probes_1+(_S28 + 1U)) = packed_float4((&(*(&kernelContext_2)->tile_0)[int(0)])->g_0) ;

#line 397
        *((&kernelContext_2)->probes_1+(_S28 + 2U)) = packed_float4((&(*(&kernelContext_2)->tile_0)[int(0)])->b_0) ;

#line 395
    }

#line 401
    return;
}

