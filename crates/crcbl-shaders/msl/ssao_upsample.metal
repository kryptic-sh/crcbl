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


#line 213 "shaders/ssao_upsample.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 216
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 227
float view_z_0(int2 pixel_1, float depth_0, float2 extent_1, KernelContext_0 thread* kernelContext_1)
{



    float4 view_0 = (((float4(float2((float(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (float(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.z / view_0.w;
}


#line 202
int2 full_res_pixel_0(int2 pixel_2)
{
    return pixel_2 * int2(int(2)) ;
}


#line 254
float ao_intensity_0(KernelContext_0 thread* kernelContext_2)
{
    float asked_0 = kernelContext_2->camera_0->params_0.z;

#line 256
    float _S2;
    if(asked_0 == 0.0f)
    {

#line 257
        _S2 = 1.0f;

#line 257
    }
    else
    {

#line 257
        _S2 = clamp(asked_0, 0.25f, 4.0f);

#line 257
    }

#line 257
    return _S2;
}


#line 257
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 257
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 271
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 271
    thread KernelContext_0 kernelContext_3;

#line 271
    (&kernelContext_3)->occlusion_0 = occlusion_1;

#line 271
    (&kernelContext_3)->scene_depth_0 = scene_depth_1;

#line 271
    (&kernelContext_3)->camera_0 = camera_1;

#line 277
    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S4 = int2(int(width_0), int(height_0));
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));

    int2 _S5 = int2(position_0.xy);

#line 287
    float _S6 = depth_at_0(_S5, depth_extent_0, &kernelContext_3);

#line 292
    if(_S6 <= 0.0f)
    {

#line 292
        pixelOutput_0 _S7 = { 1.0f };

        return _S7;
    }

#line 294
    float _S8 = view_z_0(_S5, _S6, depth_size_0, &kernelContext_3);


    float _S9 = (&kernelContext_3)->camera_0->params_0.x * 2.0f;

#line 305
    int2 nearest_0 = _S5 / int2(int(2)) ;
    int2 offset_0 = _S5 - full_res_pixel_0(nearest_0);
    float2 _S10 = float2(offset_0) / float2(2.0f) ;

#line 313
    int2 _S11 = int2(int(1), int(1));

#line 313
    int2 _S12 = min(offset_0, _S11);

#line 313
    int y_0 = int(0);

#line 313
    float total_0 = 0.0f;

#line 313
    float weight_0 = 0.0f;



    for(;;)
    {

#line 317
        if(y_0 <= (_S12.y))
        {
        }
        else
        {

#line 317
            break;
        }

#line 317
        int x_0 = int(0);

        for(;;)
        {

#line 319
            if(x_0 <= (_S12.x))
            {
            }
            else
            {

#line 319
                break;
            }

#line 326
            int2 tap_0 = clamp(nearest_0 + int2(x_0, y_0), int2(int(0), int(0)), _S4 - _S11);
            int2 texel_0 = full_res_pixel_0(tap_0);

#line 327
            float _S13 = depth_at_0(texel_0, depth_extent_0, &kernelContext_3);

#line 327
            float _S14 = view_z_0(texel_0, _S13, depth_size_0, &kernelContext_3);

            float away_0 = abs(_S14 - _S8);

#line 334
            bool _S15 = x_0 == int(0);

#line 334
            float _S16;

#line 334
            if(_S15)
            {

#line 334
                _S16 = 1.0f - _S10.x;

#line 334
            }
            else
            {

#line 334
                _S16 = _S10.x;

#line 334
            }
            bool _S17 = y_0 == int(0);

#line 335
            float _S18;

#line 335
            if(_S17)
            {

#line 335
                _S18 = 1.0f - _S10.y;

#line 335
            }
            else
            {

#line 335
                _S18 = _S10.y;

#line 335
            }
            float _S19 = _S16 * _S18;

#line 336
            float _S20;
            if(_S13 <= 0.0f)
            {

#line 337
                _S20 = 0.0f;

#line 337
            }
            else
            {

#line 337
                _S20 = saturate(1.0f - away_0 / _S9);

#line 337
            }

#line 337
            float share_0 = _S19 * _S20;

#line 337
            bool _S21;



            if(_S15)
            {

#line 341
                _S21 = _S17;

#line 341
            }
            else
            {

#line 341
                _S21 = false;

#line 341
            }

#line 341
            float share_1;

#line 341
            if(_S21)
            {

#line 341
                share_1 = max(share_0, 0.000244140625f);

#line 341
            }
            else
            {

#line 341
                share_1 = share_0;

#line 341
            }
            int3 _S22 = int3(tap_0, int(0));

#line 342
            float total_1 = total_0 + (((&kernelContext_3)->occlusion_0).read(vec<uint,2>(((_S22)).xy), uint(((_S22)).z)).x) * share_1;
            float weight_1 = weight_0 + share_1;

#line 319
            x_0 = x_0 + int(1);

#line 319
            total_0 = total_1;

#line 319
            weight_0 = weight_1;

#line 319
        }

#line 317
        y_0 = y_0 + int(1);

#line 317
    }

#line 347
    float visibility_0 = total_0 / weight_0;

#line 347
    float _S23 = ao_intensity_0(&kernelContext_3);

#line 355
    if(_S23 == 1.0f)
    {

#line 355
        total_0 = visibility_0;

#line 355
    }
    else
    {

#line 355
        total_0 = pow(visibility_0, _S23);

#line 355
    }

#line 355
    pixelOutput_0 _S24 = { total_0 };

#line 355
    return _S24;
}


#line 355
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
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 190
    thread KernelContext_0 kernelContext_4;

#line 190
    (&kernelContext_4)->occlusion_0 = occlusion_2;

#line 190
    (&kernelContext_4)->scene_depth_0 = scene_depth_2;

#line 190
    (&kernelContext_4)->camera_0 = camera_2;

#line 263
    thread FullscreenOutput_0 output_1;

    float2 _S25 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 265
    (&output_1)->uv_2 = _S25;
    (&output_1)->position_2 = float4(_S25 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 266
    thread vertexMain_Result_0 _S26;

#line 266
    (&_S26)->position_1 = output_1.position_2;

#line 266
    (&_S26)->uv_1 = output_1.uv_2;

#line 266
    return _S26;
}

