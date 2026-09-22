#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 126 "shaders/wind.slang"
float windSmoothTriangle_0(float u_0)
{
    float s_0 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}


#line 96
struct WindProbeParams_0
{
    uint count_0;
};


#line 51
struct WindParams_0
{
    float2 baseDirection_0;
    float baseSpeed_0;
    float gustAmplitude_0;
    float gustPhase_0;
    float invGustWavelength_0;
    float2 pad0_0;
    float2 directionUv_0;
    float2 directionUvPerMetre_0;
    float2 intensityUv_0;
    float2 intensityUvPerMetre_0;
};


#line 51
struct KernelContext_0
{
    WindProbeParams_0 constant* probe_0;
    packed_float4 device* velocities_0;
    packed_float4 device* points_0;
    WindParams_0 constant* wind_0;
    texture2d<float, access::sample> windDirectionLayer_0;
    sampler windSampler_0;
    texture2d<float, access::sample> windIntensityLayer_0;
};


#line 137
float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_0)
{


    float2 _S1 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_0->windDirectionLayer_0).sample((kernelContext_0->windSampler_0), (kernelContext_0->wind_0->directionUv_0 + _S1 * kernelContext_0->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_0->windIntensityLayer_0).sample((kernelContext_0->windSampler_0), (kernelContext_0->wind_0->intensityUv_0 + _S1 * kernelContext_0->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;



    float2 base_0 = kernelContext_0->wind_0->baseDirection_0;
    float _S2 = kernelContext_0->wind_0->baseDirection_0.x;

#line 153
    float _S3 = deflection_0.x;

#line 153
    float _S4 = kernelContext_0->wind_0->baseDirection_0.y;

#line 153
    float _S5 = deflection_0.y;

#line 153
    float2 turned_0 = float2(_S2 * _S3 - _S4 * _S5, _S2 * _S5 + _S4 * _S3);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 155
    float2 direction_0;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 157
        direction_0 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 157
    }
    else
    {

#line 157
        direction_0 = base_0;

#line 157
    }

#line 162
    float speed_0 = intensity_0 * kernelContext_0->wind_0->baseSpeed_0 * (1.0f + kernelContext_0->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S1, base_0) * kernelContext_0->wind_0->invGustWavelength_0 + kernelContext_0->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_0.x * speed_0, 0.0f, direction_0.y * speed_0);
}


#line 171
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], WindProbeParams_0 constant* probe_1 [[buffer(1)]], packed_float4 device* velocities_1 [[buffer(3)]], packed_float4 device* points_1 [[buffer(2)]], WindParams_0 constant* wind_1 [[buffer(0)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(0)]], sampler windSampler_1 [[sampler(0)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(1)]])
{

#line 171
    thread KernelContext_0 kernelContext_1;

#line 171
    (&kernelContext_1)->probe_0 = probe_1;

#line 171
    (&kernelContext_1)->velocities_0 = velocities_1;

#line 171
    (&kernelContext_1)->points_0 = points_1;

#line 171
    (&kernelContext_1)->wind_0 = wind_1;

#line 171
    (&kernelContext_1)->windDirectionLayer_0 = windDirectionLayer_1;

#line 171
    (&kernelContext_1)->windSampler_0 = windSampler_1;

#line 171
    (&kernelContext_1)->windIntensityLayer_0 = windIntensityLayer_1;

    uint index_0 = thread_0.x;
    if(index_0 >= (probe_1->count_0))
    {
        return;
    }

#line 176
    packed_float4 device* _S6 = (&kernelContext_1)->velocities_0+index_0;

#line 176
    float3 _S7 = windSample_0((float4(*((&kernelContext_1)->points_0+index_0)) ).xyz, &kernelContext_1);

#line 176
    *_S6 = packed_float4(float4(_S7, 0.0f)) ;



    return;
}

