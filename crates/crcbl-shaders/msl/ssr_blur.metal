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


#line 179 "shaders/ssr_blur.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 182
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 193
float view_z_0(int2 pixel_1, float depth_0, float2 extent_1, KernelContext_0 thread* kernelContext_1)
{



    float4 view_0 = (((float4(float2((float(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (float(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.z / view_0.w;
}


#line 199
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 199
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 213
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], texture2d<float, access::sample> reflection_1 [[texture(2)]], SsrParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 213
    thread KernelContext_0 kernelContext_2;

#line 213
    (&kernelContext_2)->scene_depth_0 = scene_depth_1;

#line 213
    (&kernelContext_2)->scene_color_0 = scene_color_1;

#line 213
    (&kernelContext_2)->reflection_0 = reflection_1;

#line 213
    (&kernelContext_2)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;

#line 221
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_2 = int2(int(width_0), int(height_0));
    float2 size_0 = float2(float(width_0), float(height_0));
    int2 _S3 = int2(position_0.xy);



    int3 _S4 = int3(_S3, int(0));

#line 228
    float4 lit_0 = ((scene_color_1).read(vec<uint,2>(((_S4)).xy), uint(((_S4)).z)));
    float sharpness_0 = ((reflection_1).read(vec<uint,2>(((_S4)).xy), uint(((_S4)).z))).w;

#line 229
    float _S5 = depth_at_0(_S3, extent_2, &kernelContext_2);

#line 229
    bool _S6;

#line 237
    if(_S5 <= 0.0f)
    {

#line 237
        _S6 = true;

#line 237
    }
    else
    {

#line 237
        _S6 = sharpness_0 <= 0.0f;

#line 237
    }

#line 237
    if(_S6)
    {

#line 237
        pixelOutput_0 _S7 = { lit_0 };

        return _S7;
    }

#line 239
    float _S8 = view_z_0(_S3, _S5, size_0, &kernelContext_2);



    float _S9 = abs(_S8) * 0.01999999955296516f * 8.0f;

    float3 _S10 = float3(0.0f, 0.0f, 0.0f);

#line 245
    int y_0 = int(-1);

#line 245
    float3 total_0 = _S10;

#line 245
    float weight_0 = 0.0f;

    for(;;)
    {

#line 247
        if(y_0 < int(3))
        {
        }
        else
        {

#line 247
            break;
        }

#line 247
        int x_0 = int(-1);

        for(;;)
        {

#line 249
            if(x_0 < int(3))
            {
            }
            else
            {

#line 249
                break;
            }

#line 255
            int2 tap_0 = clamp(_S3 + int2(x_0, y_0), int2(int(0), int(0)), extent_2 - int2(int(1), int(1)));
            int3 _S11 = int3(tap_0, int(0));

#line 256
            float4 tapped_0 = (((&kernelContext_2)->reflection_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z)));

#line 263
            if(x_0 != int(0))
            {

#line 263
                _S6 = true;

#line 263
            }
            else
            {

#line 263
                _S6 = y_0 != int(0);

#line 263
            }

#line 263
            float share_0;

#line 263
            if(_S6)
            {

#line 263
                float _S12 = depth_at_0(tap_0, extent_2, &kernelContext_2);

#line 263
                float _S13 = view_z_0(tap_0, _S12, size_0, &kernelContext_2);


                float away_0 = abs(_S13 - _S8);

#line 272
                float apart_0 = abs(tapped_0.w - sharpness_0);

#line 277
                if(_S12 <= 0.0f)
                {

#line 277
                    share_0 = 0.0f;

#line 277
                }
                else
                {

#line 277
                    share_0 = saturate(1.0f - away_0 / _S9) * saturate(1.0f - apart_0 / sharpness_0);

#line 277
                }

#line 263
            }
            else
            {

#line 263
                share_0 = 1.0f;

#line 263
            }

#line 280
            float3 total_1 = total_0 + tapped_0.xyz * float3(share_0) ;
            float weight_1 = weight_0 + share_0;

#line 249
            x_0 = x_0 + int(1);

#line 249
            total_0 = total_1;

#line 249
            weight_0 = weight_1;

#line 249
        }

#line 247
        y_0 = y_0 + int(1);

#line 247
    }

#line 247
    pixelOutput_0 _S14 = { float4(lit_0.xyz + total_0 / float3(weight_0) , lit_0.w) };

#line 289
    return _S14;
}


#line 289
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 167
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 167
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], texture2d<float, access::sample> reflection_2 [[texture(2)]], SsrParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 167
    thread KernelContext_0 kernelContext_3;

#line 167
    (&kernelContext_3)->scene_depth_0 = scene_depth_2;

#line 167
    (&kernelContext_3)->scene_color_0 = scene_color_2;

#line 167
    (&kernelContext_3)->reflection_0 = reflection_2;

#line 167
    (&kernelContext_3)->camera_0 = camera_2;

#line 205
    thread FullscreenOutput_0 output_1;

    float2 _S15 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 207
    (&output_1)->uv_2 = _S15;
    (&output_1)->position_2 = float4(_S15 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 208
    thread vertexMain_Result_0 _S16;

#line 208
    (&_S16)->position_1 = output_1.position_2;

#line 208
    (&_S16)->uv_1 = output_1.uv_2;

#line 208
    return _S16;
}

