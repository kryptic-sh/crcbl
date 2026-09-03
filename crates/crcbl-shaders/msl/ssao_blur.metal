#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 280 "shaders/ssao_blur.slang"
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
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_0;
    float4 params_0;
};


#line 1084
struct KernelContext_0
{
    texture2d<float, access::sample> occlusion_0;
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* camera_0;
};


#line 291 "shaders/ssao_blur.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 294
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 404
float3 encode_bent_0(float3 summed_0, float weight_0)
{

#line 404
    float3 mean_0;

    if(weight_0 > 0.0f)
    {

#line 406
        mean_0 = summed_0 / float3(weight_0) ;

#line 406
    }
    else
    {

#line 406
        mean_0 = float3(0.0f, 0.0f, 0.0f);

#line 406
    }

#line 406
    float3 direction_0;

    if((length(mean_0)) < 0.5f)
    {

#line 408
        direction_0 = float3(0.0f, 0.0f, 0.0f);

#line 408
    }
    else
    {

#line 408
        direction_0 = normalize(mean_0);

#line 408
    }

#line 408
    float3 _S2 = float3(0.5f) ;

    return direction_0 * _S2 + _S2;
}


#line 312
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_1)
{
    return float2((&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 361
float view_z_0(int2 pixel_2, float depth_1, float2 extent_1, KernelContext_0 thread* kernelContext_2)
{

#line 361
    float2 _S3 = unproject_z_0(depth_1, kernelContext_2);

#line 367
    return _S3.x / _S3.y;
}


#line 247
float sampling_radius_0(KernelContext_0 thread* kernelContext_3)
{
    float asked_0 = kernelContext_3->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 385
float3 decode_bent_0(float4 texel_0)
{
    float3 decoded_0 = texel_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 387
    float3 _S4;
    if((length(decoded_0)) < 0.5f)
    {

#line 388
        _S4 = float3(0.0f, 0.0f, 0.0f);

#line 388
    }
    else
    {

#line 388
        _S4 = normalize(decoded_0);

#line 388
    }

#line 388
    return _S4;
}


#line 388
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 388
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 424
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S5 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 424
    thread KernelContext_0 kernelContext_4;

#line 424
    (&kernelContext_4)->occlusion_0 = occlusion_1;

#line 424
    (&kernelContext_4)->scene_depth_0 = scene_depth_1;

#line 424
    (&kernelContext_4)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S6 = int2(int(width_0), int(height_0));

#line 435
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));
    int2 _S7 = int2(position_0.xy);
    int2 centre_texel_0 = full_res_pixel_0(_S7);

#line 441
    float _S8 = depth_at_0(centre_texel_0, depth_extent_0, &kernelContext_4);

#line 447
    if(_S8 <= 0.0f)
    {

#line 447
        pixelOutput_0 _S9 = { float4(1.0f, encode_bent_0(float3(0.0f, 0.0f, 0.0f), 0.0f)) };



        return _S9;
    }

#line 451
    float _S10 = view_z_0(centre_texel_0, _S8, depth_size_0, &kernelContext_4);

#line 451
    float _S11 = sampling_radius_0(&kernelContext_4);


    float _S12 = _S11 * 2.0f;

#line 459
    float3 _S13 = float3(0.0f, 0.0f, 0.0f);

#line 459
    int y_0 = int(-1);

#line 459
    float total_0 = 0.0f;

#line 459
    float3 bent_0 = _S13;

#line 459
    float bent_weight_0 = 0.0f;

#line 459
    float weight_1 = 0.0f;

#line 465
    for(;;)
    {

#line 465
        if(y_0 < int(3))
        {
        }
        else
        {

#line 465
            break;
        }

#line 465
        int x_0 = int(-1);

        for(;;)
        {

#line 467
            if(x_0 < int(3))
            {
            }
            else
            {

#line 467
                break;
            }

#line 475
            int2 tap_0 = clamp(_S7 + int2(x_0, y_0), int2(int(0), int(0)), _S6 - int2(int(1), int(1)));

#line 475
            bool _S14;

#line 482
            if(x_0 != int(0))
            {

#line 482
                _S14 = true;

#line 482
            }
            else
            {

#line 482
                _S14 = y_0 != int(0);

#line 482
            }

#line 482
            float share_0;

#line 482
            if(_S14)
            {
                int2 texel_1 = full_res_pixel_0(tap_0);

#line 484
                float _S15 = depth_at_0(texel_1, depth_extent_0, &kernelContext_4);

#line 484
                float _S16 = view_z_0(texel_1, _S15, depth_size_0, &kernelContext_4);

                float away_0 = abs(_S16 - _S10);



                if(_S15 <= 0.0f)
                {

#line 490
                    share_0 = 0.0f;

#line 490
                }
                else
                {

#line 490
                    share_0 = saturate(1.0f - away_0 / _S12);

#line 490
                }

#line 482
            }
            else
            {

#line 482
                share_0 = 1.0f;

#line 482
            }

#line 492
            int3 _S17 = int3(tap_0, int(0));

#line 492
            float4 sample_0 = (((&kernelContext_4)->occlusion_0).read(vec<uint,2>(((_S17)).xy), uint(((_S17)).z)));
            float3 direction_1 = decode_bent_0(sample_0);

#line 499
            float total_1 = total_0 + sample_0.x * share_0;
            float3 bent_1 = bent_0 + direction_1 * float3(share_0) ;
            float bent_weight_1 = bent_weight_0 + dot(direction_1, direction_1) * share_0;
            float weight_2 = weight_1 + share_0;

#line 467
            x_0 = x_0 + int(1);

#line 467
            total_0 = total_1;

#line 467
            bent_0 = bent_1;

#line 467
            bent_weight_0 = bent_weight_1;

#line 467
            weight_1 = weight_2;

#line 467
        }

#line 465
        y_0 = y_0 + int(1);

#line 465
    }

#line 465
    pixelOutput_0 _S18 = { float4(total_0 / weight_1, encode_bent_0(bent_0, bent_weight_0)) };

#line 507
    return _S18;
}


#line 507
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 269
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 269
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 269
    thread KernelContext_0 kernelContext_5;

#line 269
    (&kernelContext_5)->occlusion_0 = occlusion_2;

#line 269
    (&kernelContext_5)->scene_depth_0 = scene_depth_2;

#line 269
    (&kernelContext_5)->camera_0 = camera_2;

#line 416
    thread FullscreenOutput_0 output_1;

    float2 _S19 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 418
    (&output_1)->uv_2 = _S19;
    (&output_1)->position_2 = float4(_S19 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 419
    thread vertexMain_Result_0 _S20;

#line 419
    (&_S20)->position_1 = output_1.position_2;

#line 419
    (&_S20)->uv_1 = output_1.uv_2;

#line 419
    return _S20;
}

