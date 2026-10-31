#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 82 "shaders/ssao_upsample_bentless.slang"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 82
struct SsaoParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    float4 params_0;
};


#line 1095 "core"
struct KernelContext_0
{
    texture2d<float, access::sample> occlusion_0;
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* camera_0;
};


#line 269 "shaders/ssao_upsample_bentless.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 272
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 283
float view_z_0(int2 pixel_1, float depth_0, float2 extent_1, KernelContext_0 thread* kernelContext_1)
{



    float4 view_0 = (((float4(float2((float(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (float(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.z / view_0.w;
}


#line 167
float sampling_radius_0(KernelContext_0 thread* kernelContext_2)
{
    float asked_0 = kernelContext_2->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 258
int2 full_res_pixel_0(int2 pixel_2)
{
    return pixel_2 * int2(int(2)) ;
}


#line 310
float ao_intensity_0(KernelContext_0 thread* kernelContext_3)
{
    float asked_1 = kernelContext_3->camera_0->params_0.z;

#line 312
    float _S2;
    if(asked_1 == 0.0f)
    {

#line 313
        _S2 = 1.0f;

#line 313
    }
    else
    {

#line 313
        _S2 = clamp(asked_1, 0.25f, 4.0f);

#line 313
    }

#line 313
    return _S2;
}


#line 313
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 313
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 327
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 327
    float shown_0;

#line 327
    thread KernelContext_0 kernelContext_4;

#line 327
    (&kernelContext_4)->occlusion_0 = occlusion_1;

#line 327
    (&kernelContext_4)->scene_depth_0 = scene_depth_1;

#line 327
    (&kernelContext_4)->camera_0 = camera_1;

#line 333
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

#line 343
    float _S6 = depth_at_0(_S5, depth_extent_0, &kernelContext_4);

#line 348
    if(_S6 <= 0.0f)
    {

#line 348
        pixelOutput_0 _S7 = { float4(1.0f, float3(0.50196081399917603f, 0.50196081399917603f, 0.50196081399917603f)) };



        return _S7;
    }

#line 352
    float _S8 = view_z_0(_S5, _S6, depth_size_0, &kernelContext_4);

#line 352
    float _S9 = sampling_radius_0(&kernelContext_4);


    float _S10 = _S9 * 2.0f;

#line 363
    int2 nearest_0 = _S5 / int2(int(2)) ;
    int2 offset_0 = _S5 - full_res_pixel_0(nearest_0);
    float2 _S11 = float2(offset_0) / float2(2.0f) ;

#line 371
    int2 _S12 = int2(int(1), int(1));

#line 371
    int2 _S13 = min(offset_0, _S12);

#line 371
    int y_0 = int(0);

#line 371
    float total_0 = 0.0f;

#line 371
    float weight_0 = 0.0f;



    for(;;)
    {

#line 375
        if(y_0 <= (_S13.y))
        {
        }
        else
        {

#line 375
            break;
        }

#line 375
        int x_0 = int(0);

        for(;;)
        {

#line 377
            if(x_0 <= (_S13.x))
            {
            }
            else
            {

#line 377
                break;
            }

#line 384
            int2 tap_0 = clamp(nearest_0 + int2(x_0, y_0), int2(int(0), int(0)), _S4 - _S12);
            int2 texel_0 = full_res_pixel_0(tap_0);

#line 385
            float _S14 = depth_at_0(texel_0, depth_extent_0, &kernelContext_4);

#line 385
            float _S15 = view_z_0(texel_0, _S14, depth_size_0, &kernelContext_4);

            float away_0 = abs(_S15 - _S8);

#line 392
            bool _S16 = x_0 == int(0);

#line 392
            if(_S16)
            {

#line 392
                shown_0 = 1.0f - _S11.x;

#line 392
            }
            else
            {

#line 392
                shown_0 = _S11.x;

#line 392
            }
            bool _S17 = y_0 == int(0);

#line 393
            float _S18;

#line 393
            if(_S17)
            {

#line 393
                _S18 = 1.0f - _S11.y;

#line 393
            }
            else
            {

#line 393
                _S18 = _S11.y;

#line 393
            }
            float _S19 = shown_0 * _S18;

#line 394
            float _S20;
            if(_S14 <= 0.0f)
            {

#line 395
                _S20 = 0.0f;

#line 395
            }
            else
            {

#line 395
                _S20 = saturate(1.0f - away_0 / _S10);

#line 395
            }

#line 395
            float share_0 = _S19 * _S20;

#line 395
            bool _S21;



            if(_S16)
            {

#line 399
                _S21 = _S17;

#line 399
            }
            else
            {

#line 399
                _S21 = false;

#line 399
            }

#line 399
            float share_1;

#line 399
            if(_S21)
            {

#line 399
                share_1 = max(share_0, 0.000244140625f);

#line 399
            }
            else
            {

#line 399
                share_1 = share_0;

#line 399
            }
            int3 _S22 = int3(tap_0, int(0));
            float total_1 = total_0 + (((&kernelContext_4)->occlusion_0).read(vec<uint,2>(((_S22)).xy), uint(((_S22)).z))).x * share_1;
            float weight_1 = weight_0 + share_1;

#line 377
            x_0 = x_0 + int(1);

#line 377
            total_0 = total_1;

#line 377
            weight_0 = weight_1;

#line 377
        }

#line 375
        y_0 = y_0 + int(1);

#line 375
    }

#line 406
    float visibility_0 = total_0 / weight_0;

#line 406
    float _S23 = ao_intensity_0(&kernelContext_4);

#line 414
    if(_S23 == 1.0f)
    {

#line 414
        shown_0 = visibility_0;

#line 414
    }
    else
    {

#line 414
        shown_0 = pow(visibility_0, _S23);

#line 414
    }

#line 414
    pixelOutput_0 _S24 = { float4(shown_0, float3(0.50196081399917603f, 0.50196081399917603f, 0.50196081399917603f)) };
    return _S24;
}


#line 415
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 246
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 246
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 246
    thread KernelContext_0 kernelContext_5;

#line 246
    (&kernelContext_5)->occlusion_0 = occlusion_2;

#line 246
    (&kernelContext_5)->scene_depth_0 = scene_depth_2;

#line 246
    (&kernelContext_5)->camera_0 = camera_2;

#line 319
    thread FullscreenOutput_0 output_1;

    float2 _S25 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 321
    (&output_1)->uv_2 = _S25;
    (&output_1)->position_2 = float4(_S25 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 322
    thread vertexMain_Result_0 _S26;

#line 322
    (&_S26)->position_1 = output_1.position_2;

#line 322
    (&_S26)->uv_1 = output_1.uv_2;

#line 322
    return _S26;
}

