#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct SsrParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
};


#line 1084
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    texture2d<float, access::sample> scene_color_0;
    texture2d<float, access::sample> reflection_0;
    SsrParams_natural_0 constant* camera_0;
};


#line 180 "shaders/ssr_blur.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 183
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 194
float view_z_0(int2 pixel_1, float depth_0, float2 extent_1, KernelContext_0 thread* kernelContext_1)
{



    float4 view_0 = (((float4(float2((float(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (float(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.z / view_0.w;
}


#line 200
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 200
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 214
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], texture2d<float, access::sample> reflection_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 214
    thread KernelContext_0 kernelContext_2;

#line 214
    (&kernelContext_2)->scene_depth_0 = scene_depth_1;

#line 214
    (&kernelContext_2)->scene_color_0 = scene_color_1;

#line 214
    (&kernelContext_2)->reflection_0 = reflection_1;

#line 214
    (&kernelContext_2)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 222
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_2 = int2(int(width_0), int(height_0));
    float2 size_0 = float2(float(width_0), float(height_0));
    int2 _S3 = int2(position_0.xy);



    int3 _S4 = int3(_S3, int(0));

#line 229
    float4 lit_0 = ((scene_color_1).read(vec<uint,2>(((_S4)).xy), uint(((_S4)).z)));
    float4 centre_0 = ((reflection_1).read(vec<uint,2>(((_S4)).xy), uint(((_S4)).z)));
    float sharpness_0 = centre_0.w;

#line 231
    float _S5 = depth_at_0(_S3, extent_2, &kernelContext_2);

#line 236
    if(_S5 <= 0.0f)
    {

#line 236
        pixelOutput_0 _S6 = { lit_0 };

        return _S6;
    }
    if(sharpness_0 <= 0.0f)
    {

#line 240
        pixelOutput_0 _S7 = { float4(lit_0.xyz + centre_0.xyz, lit_0.w) };

        return _S7;
    }

#line 242
    float _S8 = view_z_0(_S3, _S5, size_0, &kernelContext_2);



    float _S9 = abs(_S8) * 0.01999999955296516f * 8.0f;

    float3 _S10 = float3(0.0f, 0.0f, 0.0f);

#line 248
    int y_0 = int(-1);

#line 248
    float3 total_0 = _S10;

#line 248
    float weight_0 = 0.0f;

    for(;;)
    {

#line 250
        if(y_0 < int(3))
        {
        }
        else
        {

#line 250
            break;
        }

#line 250
        int x_0 = int(-1);

        for(;;)
        {

#line 252
            if(x_0 < int(3))
            {
            }
            else
            {

#line 252
                break;
            }

#line 258
            int2 tap_0 = clamp(_S3 + int2(x_0, y_0), int2(int(0), int(0)), extent_2 - int2(int(1), int(1)));
            int3 _S11 = int3(tap_0, int(0));

#line 259
            float4 tapped_0 = (((&kernelContext_2)->reflection_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z)));

#line 259
            bool _S12;

#line 266
            if(x_0 != int(0))
            {

#line 266
                _S12 = true;

#line 266
            }
            else
            {

#line 266
                _S12 = y_0 != int(0);

#line 266
            }

#line 266
            float share_0;

#line 266
            if(_S12)
            {

#line 266
                float _S13 = depth_at_0(tap_0, extent_2, &kernelContext_2);

#line 266
                float _S14 = view_z_0(tap_0, _S13, size_0, &kernelContext_2);


                float away_0 = abs(_S14 - _S8);

#line 275
                float apart_0 = abs(tapped_0.w - sharpness_0);

#line 280
                if(_S13 <= 0.0f)
                {

#line 280
                    share_0 = 0.0f;

#line 280
                }
                else
                {

#line 280
                    share_0 = saturate(1.0f - away_0 / _S9) * saturate(1.0f - apart_0 / sharpness_0);

#line 280
                }

#line 266
            }
            else
            {

#line 266
                share_0 = 1.0f;

#line 266
            }

#line 283
            float3 total_1 = total_0 + tapped_0.xyz * float3(share_0) ;
            float weight_1 = weight_0 + share_0;

#line 252
            x_0 = x_0 + int(1);

#line 252
            total_0 = total_1;

#line 252
            weight_0 = weight_1;

#line 252
        }

#line 250
        y_0 = y_0 + int(1);

#line 250
    }

#line 250
    pixelOutput_0 _S15 = { float4(lit_0.xyz + mix(centre_0.xyz, total_0 / float3(weight_0) , float3(sharpness_0) ), lit_0.w) };

#line 292
    return _S15;
}


#line 292
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 168
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 168
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], texture2d<float, access::sample> reflection_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 168
    thread KernelContext_0 kernelContext_3;

#line 168
    (&kernelContext_3)->scene_depth_0 = scene_depth_2;

#line 168
    (&kernelContext_3)->scene_color_0 = scene_color_2;

#line 168
    (&kernelContext_3)->reflection_0 = reflection_2;

#line 168
    (&kernelContext_3)->camera_0 = camera_2;

#line 206
    thread FullscreenOutput_0 output_1;

    float2 _S16 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 208
    (&output_1)->uv_2 = _S16;
    (&output_1)->position_2 = float4(_S16 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 209
    thread vertexMain_Result_0 _S17;

#line 209
    (&_S17)->position_1 = output_1.position_2;

#line 209
    (&_S17)->uv_1 = output_1.uv_2;

#line 209
    return _S17;
}

