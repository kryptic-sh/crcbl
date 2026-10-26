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
    texture2d<float, access::sample> reflection_0;
    texture2d<float, access::sample> scene_color_0;
    SsrParams_natural_0 constant* camera_0;
};


#line 202 "shaders/ssr_blur.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 205
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 223
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_1)
{
    return float2((&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 272
float view_z_0(int2 pixel_1, float depth_1, float2 extent_1, KernelContext_0 thread* kernelContext_2)
{

#line 272
    float2 _S2 = unproject_z_0(depth_1, kernelContext_2);

#line 278
    return _S2.x / _S2.y;
}


#line 278
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 278
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 292
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> reflection_1 [[texture(2)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 292
    thread KernelContext_0 kernelContext_3;

#line 292
    (&kernelContext_3)->scene_depth_0 = scene_depth_1;

#line 292
    (&kernelContext_3)->reflection_0 = reflection_1;

#line 292
    (&kernelContext_3)->scene_color_0 = scene_color_1;

#line 292
    (&kernelContext_3)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 300
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_2 = int2(int(width_0), int(height_0));
    float2 size_0 = float2(float(width_0), float(height_0));
    int2 _S4 = int2(position_0.xy);
    thread uint reflection_width_0;
    thread uint reflection_height_0;
    (*((&reflection_width_0)) = (reflection_1).get_width(0)),(*((&reflection_height_0)) = (reflection_1).get_height(0));
    int2 reflection_extent_0 = int2(int(reflection_width_0), int(reflection_height_0));

#line 307
    int2 _S5 = int2(int(2)) ;

#line 313
    int2 _S6 = int2(int(1), int(1));

#line 313
    int2 _S7 = (_S4 * _S5 + _S6) * reflection_extent_0 / (extent_2 * _S5);
    int2 _S8 = int2(int(0), int(0));
    int2 _S9 = reflection_extent_0 - _S6;

#line 312
    int2 reflection_pixel_0 = clamp(_S7, _S8, _S9);

#line 312
    int _S10;

#line 317
    if(any(reflection_extent_0 != extent_2))
    {

#line 317
        _S10 = int(2);

#line 317
    }
    else
    {

#line 317
        _S10 = int(3);

#line 317
    }



    int3 _S11 = int3(_S4, int(0));

#line 321
    float4 lit_0 = (((&kernelContext_3)->scene_color_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z)));
    int3 _S12 = int3(reflection_pixel_0, int(0));

#line 322
    float4 centre_0 = (((&kernelContext_3)->reflection_0).read(vec<uint,2>(((_S12)).xy), uint(((_S12)).z)));
    float sharpness_0 = centre_0.w;

#line 323
    float _S13 = depth_at_0(_S4, extent_2, &kernelContext_3);

#line 328
    if(_S13 <= 0.0f)
    {

#line 328
        pixelOutput_0 _S14 = { lit_0 };

        return _S14;
    }
    if(sharpness_0 <= 0.0f)
    {

#line 332
        pixelOutput_0 _S15 = { float4(lit_0.xyz + centre_0.xyz, lit_0.w) };

        return _S15;
    }

#line 334
    float _S16 = view_z_0(_S4, _S13, size_0, &kernelContext_3);



    float _S17 = abs(_S16) * 0.01999999955296516f * 8.0f;

    float3 _S18 = float3(0.0f, 0.0f, 0.0f);

#line 340
    int y_0 = int(-1);

#line 340
    float3 total_0 = _S18;

#line 340
    float weight_0 = 0.0f;

    for(;;)
    {

#line 342
        if(y_0 < _S10)
        {
        }
        else
        {

#line 342
            break;
        }

#line 342
        int x_0 = int(-1);

        for(;;)
        {

#line 344
            if(x_0 < _S10)
            {
            }
            else
            {

#line 344
                break;
            }

#line 350
            int2 tap_0 = clamp(reflection_pixel_0 + int2(x_0, y_0), _S8, _S9);

            int3 _S19 = int3(tap_0, int(0));

#line 352
            float4 tapped_0 = (((&kernelContext_3)->reflection_0).read(vec<uint,2>(((_S19)).xy), uint(((_S19)).z)));

#line 352
            bool _S20;

#line 359
            if(x_0 != int(0))
            {

#line 359
                _S20 = true;

#line 359
            }
            else
            {

#line 359
                _S20 = y_0 != int(0);

#line 359
            }

#line 359
            float share_0;

#line 359
            if(_S20)
            {

#line 369
                int2 _S21 = (tap_0 * _S5 + _S6) * extent_2 / (reflection_extent_0 * _S5);

#line 368
                int2 represented_0 = clamp(_S21, _S8, extent_2 - _S6);

#line 368
                float _S22 = depth_at_0(represented_0, extent_2, &kernelContext_3);

#line 368
                float _S23 = view_z_0(represented_0, _S22, size_0, &kernelContext_3);

#line 373
                float away_0 = abs(_S23 - _S16);

#line 379
                float apart_0 = abs(tapped_0.w - sharpness_0);

#line 384
                if(_S22 <= 0.0f)
                {

#line 384
                    share_0 = 0.0f;

#line 384
                }
                else
                {

#line 384
                    share_0 = saturate(1.0f - away_0 / _S17) * saturate(1.0f - apart_0 / sharpness_0);

#line 384
                }

#line 359
            }
            else
            {

#line 359
                share_0 = 1.0f;

#line 359
            }

#line 387
            float3 total_1 = total_0 + tapped_0.xyz * float3(share_0) ;
            float weight_1 = weight_0 + share_0;

#line 344
            x_0 = x_0 + int(1);

#line 344
            total_0 = total_1;

#line 344
            weight_0 = weight_1;

#line 344
        }

#line 342
        y_0 = y_0 + int(1);

#line 342
    }

#line 342
    pixelOutput_0 _S24 = { float4(lit_0.xyz + mix(centre_0.xyz, total_0 / float3(weight_0) , float3(sqrt(sharpness_0)) ), lit_0.w) };

#line 399
    return _S24;
}


#line 399
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
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> reflection_2 [[texture(2)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 190
    thread KernelContext_0 kernelContext_4;

#line 190
    (&kernelContext_4)->scene_depth_0 = scene_depth_2;

#line 190
    (&kernelContext_4)->reflection_0 = reflection_2;

#line 190
    (&kernelContext_4)->scene_color_0 = scene_color_2;

#line 190
    (&kernelContext_4)->camera_0 = camera_2;

#line 284
    thread FullscreenOutput_0 output_1;

    float2 _S25 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 286
    (&output_1)->uv_2 = _S25;
    (&output_1)->position_2 = float4(_S25 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 287
    thread vertexMain_Result_0 _S26;

#line 287
    (&_S26)->position_1 = output_1.position_2;

#line 287
    (&_S26)->uv_1 = output_1.uv_2;

#line 287
    return _S26;
}

