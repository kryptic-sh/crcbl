#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 193 "shaders/ssao_blur.slang"
int2 full_res_pixel_0(int2 pixel_0)
{
    return pixel_0 * int2(int(2)) ;
}


#line 2580 "core.meta.slang"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 2580
struct SsaoParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    float4 params_0;
};


#line 1084 "core"
struct KernelContext_0
{
    texture2d<float, access::sample> occlusion_0;
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* camera_0;
};


#line 204 "shaders/ssao_blur.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 207
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 225
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_1)
{
    return float2((&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 274
float view_z_0(int2 pixel_2, float depth_1, float2 extent_1, KernelContext_0 thread* kernelContext_2)
{

#line 274
    float2 _S2 = unproject_z_0(depth_1, kernelContext_2);

#line 280
    return _S2.x / _S2.y;
}


#line 280
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 280
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 294
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 294
    thread KernelContext_0 kernelContext_3;

#line 294
    (&kernelContext_3)->occlusion_0 = occlusion_1;

#line 294
    (&kernelContext_3)->scene_depth_0 = scene_depth_1;

#line 294
    (&kernelContext_3)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S4 = int2(int(width_0), int(height_0));

#line 305
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));
    int2 _S5 = int2(position_0.xy);
    int2 centre_texel_0 = full_res_pixel_0(_S5);

#line 311
    float _S6 = depth_at_0(centre_texel_0, depth_extent_0, &kernelContext_3);

#line 317
    if(_S6 <= 0.0f)
    {

#line 317
        pixelOutput_0 _S7 = { 1.0f };

        return _S7;
    }

#line 319
    float _S8 = view_z_0(centre_texel_0, _S6, depth_size_0, &kernelContext_3);


    float _S9 = (&kernelContext_3)->camera_0->params_0.x * 2.0f;

#line 322
    int y_0 = int(-1);

#line 322
    float total_0 = 0.0f;

#line 322
    float weight_0 = 0.0f;



    for(;;)
    {

#line 326
        if(y_0 < int(3))
        {
        }
        else
        {

#line 326
            break;
        }

#line 326
        int x_0 = int(-1);

        for(;;)
        {

#line 328
            if(x_0 < int(3))
            {
            }
            else
            {

#line 328
                break;
            }

#line 336
            int2 tap_0 = clamp(_S5 + int2(x_0, y_0), int2(int(0), int(0)), _S4 - int2(int(1), int(1)));

#line 336
            bool _S10;

#line 343
            if(x_0 != int(0))
            {

#line 343
                _S10 = true;

#line 343
            }
            else
            {

#line 343
                _S10 = y_0 != int(0);

#line 343
            }

#line 343
            float share_0;

#line 343
            if(_S10)
            {
                int2 texel_0 = full_res_pixel_0(tap_0);

#line 345
                float _S11 = depth_at_0(texel_0, depth_extent_0, &kernelContext_3);

#line 345
                float _S12 = view_z_0(texel_0, _S11, depth_size_0, &kernelContext_3);

                float away_0 = abs(_S12 - _S8);



                if(_S11 <= 0.0f)
                {

#line 351
                    share_0 = 0.0f;

#line 351
                }
                else
                {

#line 351
                    share_0 = saturate(1.0f - away_0 / _S9);

#line 351
                }

#line 343
            }
            else
            {

#line 343
                share_0 = 1.0f;

#line 343
            }

#line 353
            int3 _S13 = int3(tap_0, int(0));

#line 353
            float total_1 = total_0 + (((&kernelContext_3)->occlusion_0).read(vec<uint,2>(((_S13)).xy), uint(((_S13)).z)).x) * share_0;
            float weight_1 = weight_0 + share_0;

#line 328
            x_0 = x_0 + int(1);

#line 328
            total_0 = total_1;

#line 328
            weight_0 = weight_1;

#line 328
        }

#line 326
        y_0 = y_0 + int(1);

#line 326
    }

#line 326
    pixelOutput_0 _S14 = { total_0 / weight_0 };

#line 358
    return _S14;
}


#line 358
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 182
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 182
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 182
    thread KernelContext_0 kernelContext_4;

#line 182
    (&kernelContext_4)->occlusion_0 = occlusion_2;

#line 182
    (&kernelContext_4)->scene_depth_0 = scene_depth_2;

#line 182
    (&kernelContext_4)->camera_0 = camera_2;

#line 286
    thread FullscreenOutput_0 output_1;

    float2 _S15 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 288
    (&output_1)->uv_2 = _S15;
    (&output_1)->position_2 = float4(_S15 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 289
    thread vertexMain_Result_0 _S16;

#line 289
    (&_S16)->position_1 = output_1.position_2;

#line 289
    (&_S16)->uv_1 = output_1.uv_2;

#line 289
    return _S16;
}

