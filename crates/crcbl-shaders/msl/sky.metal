#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 158 "shaders/sky.slang"
constant array<float3, int(7)> SUN_LIMB_FIT_0 = { { float3(-5.77344098928733729e-06f, 3.9952874431037344e-07f, -3.33302978106075898e-06f), float3(0.00083442457253113f, -0.00004951301889378f, 0.00036370038287714f), float3(-0.02268672175705433f, 0.00105253444053233f, -0.00656718015670776f), float3(0.77790427207946777f, -0.01045561581850052f, 0.04684425517916679f), float3(0.37402597069740295f, 0.98949694633483887f, -0.18627837300300598f), float3(-0.17161113023757935f, 0.02444075979292393f, 1.0402073860168457f), float3(0.04154470935463905f, -0.00448589120060205f, 0.10543691366910934f) } };

#line 122
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 78
struct SkyParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    array<float4, int(3)> sky_0;
    float4 atmosphere_0;
    float4 sun_disc_0;
};


#line 140
struct KernelContext_0
{
    SkyParams_natural_0 constant* camera_0;
    packed_float4 device* sky_view_0;
};


#line 217
float3 sky_view_at_0(float up_0, float azimuth_cosine_0, KernelContext_0 thread* kernelContext_0)
{
    float u_0 = sqrt(max(0.0f, (1.0f - clamp(azimuth_cosine_0, -1.0f, 1.0f)) * 0.5f));
    float clamped_0 = clamp(up_0, -1.0f, 1.0f);
    float root_0 = sqrt(abs(clamped_0));

#line 221
    float _S1;
    if(clamped_0 >= 0.0f)
    {

#line 222
        _S1 = root_0;

#line 222
    }
    else
    {

#line 222
        _S1 = - root_0;

#line 222
    }

    float across_0 = clamp(u_0, 0.0f, 1.0f) * 96.0f - 0.5f;
    float x0_0 = clamp(floor(across_0), 0.0f, 95.0f);

    float fx_0 = clamp(across_0 - x0_0, 0.0f, 1.0f);

    float down_0 = clamp(0.5f + 0.5f * _S1, 0.0f, 1.0f) * 64.0f - 0.5f;
    float y0_0 = clamp(floor(down_0), 0.0f, 63.0f);

    float fy_0 = clamp(down_0 - y0_0, 0.0f, 1.0f);

    uint row0_0 = uint(y0_0) * 96U;
    uint row1_0 = uint(min(y0_0 + 1.0f, 63.0f)) * 96U;
    uint _S2 = uint(x0_0);

#line 236
    float3 _S3 = float3((1.0f - fx_0)) ;
    uint _S4 = uint(min(x0_0 + 1.0f, 95.0f));

#line 237
    float3 _S5 = float3(fx_0) ;


    return ((float4(*(kernelContext_0->sky_view_0+(row0_0 + _S2))) ).xyz * _S3 + (float4(*(kernelContext_0->sky_view_0+(row0_0 + _S4))) ).xyz * _S5) * float3((1.0f - fy_0))  + ((float4(*(kernelContext_0->sky_view_0+(row1_0 + _S2))) ).xyz * _S3 + (float4(*(kernelContext_0->sky_view_0+(row1_0 + _S4))) ).xyz * _S5) * float3(fy_0) ;
}


#line 250
float3 atmosphere_radiance_0(float3 direction_0, KernelContext_0 thread* kernelContext_1)
{
    float3 sun_0 = kernelContext_1->camera_0->atmosphere_0.xyz;
    float _S6 = direction_0.x;

#line 253
    float _S7 = direction_0.z;

#line 253
    float view_flat_0 = sqrt(_S6 * _S6 + _S7 * _S7);
    float _S8 = sun_0.x;

#line 254
    float _S9 = sun_0.z;

#line 254
    float sun_flat_0 = sqrt(_S8 * _S8 + _S9 * _S9);

#line 254
    bool _S10;

    if(view_flat_0 > 0.0f)
    {

#line 256
        _S10 = sun_flat_0 > 0.0f;

#line 256
    }
    else
    {

#line 256
        _S10 = false;

#line 256
    }

#line 256
    float cosine_0;

#line 256
    if(_S10)
    {

#line 256
        cosine_0 = (_S6 * _S8 + _S7 * _S9) / (view_flat_0 * sun_flat_0);

#line 256
    }
    else
    {

#line 256
        cosine_0 = 1.0f;

#line 256
    }

#line 256
    float3 _S11 = sky_view_at_0(direction_0.y, cosine_0, kernelContext_1);



    return _S11;
}


#line 280
float3 sun_disc_1(float3 direction_1, KernelContext_0 thread* kernelContext_2)
{
    float3 apart_0 = direction_1 - kernelContext_2->camera_0->atmosphere_0.xyz;



    float radius_squared_0 = 0.5f * dot(apart_0, apart_0) / kernelContext_2->camera_0->sun_disc_0.w;
    if(radius_squared_0 >= 1.0f)
    {
        return float3(0.0f, 0.0f, 0.0f);
    }



    float _S12 = sqrt(sqrt(sqrt(sqrt(1.0f - radius_squared_0))));

#line 294
    float3 limb_0 = float3(0.04154470935463905f, -0.00448589120060205f, 0.10543691366910934f);

#line 294
    int power_0 = int(5);

    for(;;)
    {

#line 296
        if(power_0 >= int(0))
        {
        }
        else
        {

#line 296
            break;
        }
        float3 _S13 = limb_0 * float3(_S12)  + SUN_LIMB_FIT_0[power_0];

#line 296
        int power_1 = power_0 - int(1);

#line 296
        limb_0 = _S13;

#line 296
        power_0 = power_1;

#line 296
    }

#line 302
    return kernelContext_2->camera_0->sun_disc_0.xyz * max(limb_0, float3(0.0f, 0.0f, 0.0f));
}


#line 200
float3 sky_radiance_0(float3 direction_2, KernelContext_0 thread* kernelContext_3)
{
    float up_1 = clamp(direction_2.y, -1.0f, 1.0f);

#line 202
    float3 far_0;
    if(up_1 >= 0.0f)
    {

#line 203
        far_0 = kernelContext_3->camera_0->sky_0[int(0)].xyz;

#line 203
    }
    else
    {

#line 203
        far_0 = kernelContext_3->camera_0->sky_0[int(2)].xyz;

#line 203
    }
    float u_1 = abs(up_1);
    float blend_0 = u_1 * u_1 * (3.0f - 2.0f * u_1);
    return kernelContext_3->camera_0->sky_0[int(1)].xyz * float3((1.0f - blend_0))  + far_0 * float3(blend_0) ;
}


#line 206
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 190
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 319
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S14 [[stage_in]], float4 position_0 [[position]], SkyParams_natural_0 constant* camera_1 [[buffer(0)]], packed_float4 device* sky_view_1 [[buffer(1)]])
{

#line 319
    thread KernelContext_0 kernelContext_4;

#line 319
    (&kernelContext_4)->camera_0 = camera_1;

#line 319
    (&kernelContext_4)->sky_view_0 = sky_view_1;

#line 327
    float2 ndc_0 = float2(_S14.uv_0.x * 2.0f - 1.0f, 1.0f - _S14.uv_0.y * 2.0f);

    float4 near_plane_0 = (((float4(ndc_0, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_proj_0.data_0[int(0)][int(0)], camera_1->inv_proj_0.data_0[int(1)][int(0)], camera_1->inv_proj_0.data_0[int(2)][int(0)], camera_1->inv_proj_0.data_0[int(3)][int(0)], camera_1->inv_proj_0.data_0[int(0)][int(1)], camera_1->inv_proj_0.data_0[int(1)][int(1)], camera_1->inv_proj_0.data_0[int(2)][int(1)], camera_1->inv_proj_0.data_0[int(3)][int(1)], camera_1->inv_proj_0.data_0[int(0)][int(2)], camera_1->inv_proj_0.data_0[int(1)][int(2)], camera_1->inv_proj_0.data_0[int(2)][int(2)], camera_1->inv_proj_0.data_0[int(3)][int(2)], camera_1->inv_proj_0.data_0[int(0)][int(3)], camera_1->inv_proj_0.data_0[int(1)][int(3)], camera_1->inv_proj_0.data_0[int(2)][int(3)], camera_1->inv_proj_0.data_0[int(3)][int(3)]))));
    float4 beyond_0 = (((float4(ndc_0, 0.5f, 1.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_proj_0.data_0[int(0)][int(0)], camera_1->inv_proj_0.data_0[int(1)][int(0)], camera_1->inv_proj_0.data_0[int(2)][int(0)], camera_1->inv_proj_0.data_0[int(3)][int(0)], camera_1->inv_proj_0.data_0[int(0)][int(1)], camera_1->inv_proj_0.data_0[int(1)][int(1)], camera_1->inv_proj_0.data_0[int(2)][int(1)], camera_1->inv_proj_0.data_0[int(3)][int(1)], camera_1->inv_proj_0.data_0[int(0)][int(2)], camera_1->inv_proj_0.data_0[int(1)][int(2)], camera_1->inv_proj_0.data_0[int(2)][int(2)], camera_1->inv_proj_0.data_0[int(3)][int(2)], camera_1->inv_proj_0.data_0[int(0)][int(3)], camera_1->inv_proj_0.data_0[int(1)][int(3)], camera_1->inv_proj_0.data_0[int(2)][int(3)], camera_1->inv_proj_0.data_0[int(3)][int(3)]))));

    float3 direction_3 = normalize((((float4(beyond_0.xyz / float3(beyond_0.w)  - near_plane_0.xyz / float3(near_plane_0.w) , 0.0f)) * (matrix<float,int(4),int(4)> (camera_1->inv_view_0.data_0[int(0)][int(0)], camera_1->inv_view_0.data_0[int(1)][int(0)], camera_1->inv_view_0.data_0[int(2)][int(0)], camera_1->inv_view_0.data_0[int(3)][int(0)], camera_1->inv_view_0.data_0[int(0)][int(1)], camera_1->inv_view_0.data_0[int(1)][int(1)], camera_1->inv_view_0.data_0[int(2)][int(1)], camera_1->inv_view_0.data_0[int(3)][int(1)], camera_1->inv_view_0.data_0[int(0)][int(2)], camera_1->inv_view_0.data_0[int(1)][int(2)], camera_1->inv_view_0.data_0[int(2)][int(2)], camera_1->inv_view_0.data_0[int(3)][int(2)], camera_1->inv_view_0.data_0[int(0)][int(3)], camera_1->inv_view_0.data_0[int(1)][int(3)], camera_1->inv_view_0.data_0[int(2)][int(3)], camera_1->inv_view_0.data_0[int(3)][int(3)])))).xyz);

#line 332
    float3 radiance_0;

#line 341
    if((camera_1->atmosphere_0.w) > 0.0f)
    {

#line 341
        float3 _S15 = atmosphere_radiance_0(direction_3, &kernelContext_4);

#line 341
        float3 _S16 = sun_disc_1(direction_3, &kernelContext_4);

#line 341
        radiance_0 = _S15 + _S16;

#line 341
    }
    else
    {

#line 341
        float3 _S17 = sky_radiance_0(direction_3, &kernelContext_4);

#line 341
        radiance_0 = _S17;

#line 341
    }

#line 341
    pixelOutput_0 _S18 = { float4(min(radiance_0, float3(65504.0f, 65504.0f, 65504.0f)), 1.0f) };

#line 346
    return _S18;
}


#line 346
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 190
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 190
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], SkyParams_natural_0 constant* camera_2 [[buffer(0)]], packed_float4 device* sky_view_2 [[buffer(1)]])
{

#line 190
    thread KernelContext_0 kernelContext_5;

#line 190
    (&kernelContext_5)->camera_0 = camera_2;

#line 190
    (&kernelContext_5)->sky_view_0 = sky_view_2;

#line 308
    thread FullscreenOutput_0 output_1;

#line 313
    float2 _S19 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 313
    (&output_1)->uv_2 = _S19;
    (&output_1)->position_2 = float4(_S19 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 314
    thread vertexMain_Result_0 _S20;

#line 314
    (&_S20)->position_1 = output_1.position_2;

#line 314
    (&_S20)->uv_1 = output_1.uv_2;

#line 314
    return _S20;
}

