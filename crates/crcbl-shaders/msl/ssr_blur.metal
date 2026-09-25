#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 93 "shaders/ssr_blur.slang"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 93
struct SsrParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
};


#line 1095 "core"
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    texture2d<float, access::sample> scene_color_0;
    texture2d<float, access::sample> reflection_0;
    SsrParams_natural_0 constant* camera_0;
};


#line 192 "shaders/ssr_blur.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 195
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 213
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_1)
{
    return float2((&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 262
float view_z_0(int2 pixel_1, float depth_1, float2 extent_1, KernelContext_0 thread* kernelContext_2)
{

#line 262
    float2 _S2 = unproject_z_0(depth_1, kernelContext_2);

#line 268
    return _S2.x / _S2.y;
}


#line 268
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 268
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 282
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], texture2d<float, access::sample> reflection_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 282
    thread KernelContext_0 kernelContext_3;

#line 282
    (&kernelContext_3)->scene_depth_0 = scene_depth_1;

#line 282
    (&kernelContext_3)->scene_color_0 = scene_color_1;

#line 282
    (&kernelContext_3)->reflection_0 = reflection_1;

#line 282
    (&kernelContext_3)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 290
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_2 = int2(int(width_0), int(height_0));
    float2 size_0 = float2(float(width_0), float(height_0));
    int2 _S4 = int2(position_0.xy);



    int3 _S5 = int3(_S4, int(0));

#line 297
    float4 lit_0 = ((scene_color_1).read(vec<uint,2>(((_S5)).xy), uint(((_S5)).z)));
    float4 centre_0 = ((reflection_1).read(vec<uint,2>(((_S5)).xy), uint(((_S5)).z)));
    float sharpness_0 = centre_0.w;

#line 299
    float _S6 = depth_at_0(_S4, extent_2, &kernelContext_3);

#line 304
    if(_S6 <= 0.0f)
    {

#line 304
        pixelOutput_0 _S7 = { lit_0 };

        return _S7;
    }
    if(sharpness_0 <= 0.0f)
    {

#line 308
        pixelOutput_0 _S8 = { float4(lit_0.xyz + centre_0.xyz, lit_0.w) };

        return _S8;
    }

#line 310
    float _S9 = view_z_0(_S4, _S6, size_0, &kernelContext_3);



    float _S10 = abs(_S9) * 0.01999999955296516f * 8.0f;

    float3 _S11 = float3(0.0f, 0.0f, 0.0f);

#line 316
    int y_0 = int(-1);

#line 316
    float3 total_0 = _S11;

#line 316
    float weight_0 = 0.0f;

    for(;;)
    {

#line 318
        if(y_0 < int(3))
        {
        }
        else
        {

#line 318
            break;
        }

#line 318
        int x_0 = int(-1);

        for(;;)
        {

#line 320
            if(x_0 < int(3))
            {
            }
            else
            {

#line 320
                break;
            }

#line 326
            int2 tap_0 = clamp(_S4 + int2(x_0, y_0), int2(int(0), int(0)), extent_2 - int2(int(1), int(1)));
            int3 _S12 = int3(tap_0, int(0));

#line 327
            float4 tapped_0 = (((&kernelContext_3)->reflection_0).read(vec<uint,2>(((_S12)).xy), uint(((_S12)).z)));

#line 327
            bool _S13;

#line 334
            if(x_0 != int(0))
            {

#line 334
                _S13 = true;

#line 334
            }
            else
            {

#line 334
                _S13 = y_0 != int(0);

#line 334
            }

#line 334
            float share_0;

#line 334
            if(_S13)
            {

#line 334
                float _S14 = depth_at_0(tap_0, extent_2, &kernelContext_3);

#line 334
                float _S15 = view_z_0(tap_0, _S14, size_0, &kernelContext_3);


                float away_0 = abs(_S15 - _S9);

#line 343
                float apart_0 = abs(tapped_0.w - sharpness_0);

#line 348
                if(_S14 <= 0.0f)
                {

#line 348
                    share_0 = 0.0f;

#line 348
                }
                else
                {

#line 348
                    share_0 = saturate(1.0f - away_0 / _S10) * saturate(1.0f - apart_0 / sharpness_0);

#line 348
                }

#line 334
            }
            else
            {

#line 334
                share_0 = 1.0f;

#line 334
            }

#line 351
            float3 total_1 = total_0 + tapped_0.xyz * float3(share_0) ;
            float weight_1 = weight_0 + share_0;

#line 320
            x_0 = x_0 + int(1);

#line 320
            total_0 = total_1;

#line 320
            weight_0 = weight_1;

#line 320
        }

#line 318
        y_0 = y_0 + int(1);

#line 318
    }

#line 318
    pixelOutput_0 _S16 = { float4(lit_0.xyz + mix(centre_0.xyz, total_0 / float3(weight_0) , float3(sqrt(sharpness_0)) ), lit_0.w) };

#line 363
    return _S16;
}


#line 363
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 180
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 180
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], texture2d<float, access::sample> reflection_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 180
    thread KernelContext_0 kernelContext_4;

#line 180
    (&kernelContext_4)->scene_depth_0 = scene_depth_2;

#line 180
    (&kernelContext_4)->scene_color_0 = scene_color_2;

#line 180
    (&kernelContext_4)->reflection_0 = reflection_2;

#line 180
    (&kernelContext_4)->camera_0 = camera_2;

#line 274
    thread FullscreenOutput_0 output_1;

    float2 _S17 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 276
    (&output_1)->uv_2 = _S17;
    (&output_1)->position_2 = float4(_S17 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 277
    thread vertexMain_Result_0 _S18;

#line 277
    (&_S18)->position_1 = output_1.position_2;

#line 277
    (&_S18)->uv_1 = output_1.uv_2;

#line 277
    return _S18;
}

