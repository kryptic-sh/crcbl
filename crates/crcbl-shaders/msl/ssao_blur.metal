#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 193 "shaders/ssao_blur.slang"
int2 full_res_pixel_0(int2 pixel_0)
{
    return pixel_0 * int2(int(2)) ;
}


#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct SsaoParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    float4 params_0;
};


#line 1084
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


#line 218
float view_z_0(int2 pixel_2, float depth_0, float2 extent_1, KernelContext_0 thread* kernelContext_1)
{



    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.z / view_0.w;
}


#line 224
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 224
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 238
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 238
    thread KernelContext_0 kernelContext_2;

#line 238
    (&kernelContext_2)->occlusion_0 = occlusion_1;

#line 238
    (&kernelContext_2)->scene_depth_0 = scene_depth_1;

#line 238
    (&kernelContext_2)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S3 = int2(int(width_0), int(height_0));

#line 249
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));
    int2 _S4 = int2(position_0.xy);
    int2 centre_texel_0 = full_res_pixel_0(_S4);

#line 255
    float _S5 = depth_at_0(centre_texel_0, depth_extent_0, &kernelContext_2);

#line 261
    if(_S5 <= 0.0f)
    {

#line 261
        pixelOutput_0 _S6 = { 1.0f };

        return _S6;
    }

#line 263
    float _S7 = view_z_0(centre_texel_0, _S5, depth_size_0, &kernelContext_2);


    float _S8 = (&kernelContext_2)->camera_0->params_0.x * 2.0f;

#line 266
    int y_0 = int(-1);

#line 266
    float total_0 = 0.0f;

#line 266
    float weight_0 = 0.0f;



    for(;;)
    {

#line 270
        if(y_0 < int(3))
        {
        }
        else
        {

#line 270
            break;
        }

#line 270
        int x_0 = int(-1);

        for(;;)
        {

#line 272
            if(x_0 < int(3))
            {
            }
            else
            {

#line 272
                break;
            }

#line 280
            int2 tap_0 = clamp(_S4 + int2(x_0, y_0), int2(int(0), int(0)), _S3 - int2(int(1), int(1)));

#line 280
            bool _S9;

#line 287
            if(x_0 != int(0))
            {

#line 287
                _S9 = true;

#line 287
            }
            else
            {

#line 287
                _S9 = y_0 != int(0);

#line 287
            }

#line 287
            float share_0;

#line 287
            if(_S9)
            {
                int2 texel_0 = full_res_pixel_0(tap_0);

#line 289
                float _S10 = depth_at_0(texel_0, depth_extent_0, &kernelContext_2);

#line 289
                float _S11 = view_z_0(texel_0, _S10, depth_size_0, &kernelContext_2);

                float away_0 = abs(_S11 - _S7);



                if(_S10 <= 0.0f)
                {

#line 295
                    share_0 = 0.0f;

#line 295
                }
                else
                {

#line 295
                    share_0 = saturate(1.0f - away_0 / _S8);

#line 295
                }

#line 287
            }
            else
            {

#line 287
                share_0 = 1.0f;

#line 287
            }

#line 297
            int3 _S12 = int3(tap_0, int(0));

#line 297
            float total_1 = total_0 + (((&kernelContext_2)->occlusion_0).read(vec<uint,2>(((_S12)).xy), uint(((_S12)).z)).x) * share_0;
            float weight_1 = weight_0 + share_0;

#line 272
            x_0 = x_0 + int(1);

#line 272
            total_0 = total_1;

#line 272
            weight_0 = weight_1;

#line 272
        }

#line 270
        y_0 = y_0 + int(1);

#line 270
    }

#line 270
    pixelOutput_0 _S13 = { total_0 / weight_0 };

#line 302
    return _S13;
}


#line 302
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
    thread KernelContext_0 kernelContext_3;

#line 182
    (&kernelContext_3)->occlusion_0 = occlusion_2;

#line 182
    (&kernelContext_3)->scene_depth_0 = scene_depth_2;

#line 182
    (&kernelContext_3)->camera_0 = camera_2;

#line 230
    thread FullscreenOutput_0 output_1;

    float2 _S14 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 232
    (&output_1)->uv_2 = _S14;
    (&output_1)->position_2 = float4(_S14 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 233
    thread vertexMain_Result_0 _S15;

#line 233
    (&_S15)->position_1 = output_1.position_2;

#line 233
    (&_S15)->uv_1 = output_1.uv_2;

#line 233
    return _S15;
}

