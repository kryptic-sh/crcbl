#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 241 "shaders/ssao_blur.slang"
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


#line 252 "shaders/ssao_blur.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 255
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 365
float3 encode_bent_0(float3 summed_0, float weight_0)
{

#line 365
    float3 mean_0;

    if(weight_0 > 0.0f)
    {

#line 367
        mean_0 = summed_0 / float3(weight_0) ;

#line 367
    }
    else
    {

#line 367
        mean_0 = float3(0.0f, 0.0f, 0.0f);

#line 367
    }

#line 367
    float3 direction_0;

    if((length(mean_0)) < 0.5f)
    {

#line 369
        direction_0 = float3(0.0f, 0.0f, 0.0f);

#line 369
    }
    else
    {

#line 369
        direction_0 = normalize(mean_0);

#line 369
    }

#line 369
    float3 _S2 = float3(0.5f) ;

    return direction_0 * _S2 + _S2;
}


#line 273
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_1)
{
    return float2((&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 322
float view_z_0(int2 pixel_2, float depth_1, float2 extent_1, KernelContext_0 thread* kernelContext_2)
{

#line 322
    float2 _S3 = unproject_z_0(depth_1, kernelContext_2);

#line 328
    return _S3.x / _S3.y;
}


#line 346
float3 decode_bent_0(float4 texel_0)
{
    float3 decoded_0 = texel_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 348
    float3 _S4;
    if((length(decoded_0)) < 0.5f)
    {

#line 349
        _S4 = float3(0.0f, 0.0f, 0.0f);

#line 349
    }
    else
    {

#line 349
        _S4 = normalize(decoded_0);

#line 349
    }

#line 349
    return _S4;
}


#line 349
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 349
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 385
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S5 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 385
    thread KernelContext_0 kernelContext_3;

#line 385
    (&kernelContext_3)->occlusion_0 = occlusion_1;

#line 385
    (&kernelContext_3)->scene_depth_0 = scene_depth_1;

#line 385
    (&kernelContext_3)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S6 = int2(int(width_0), int(height_0));

#line 396
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));
    int2 _S7 = int2(position_0.xy);
    int2 centre_texel_0 = full_res_pixel_0(_S7);

#line 402
    float _S8 = depth_at_0(centre_texel_0, depth_extent_0, &kernelContext_3);

#line 408
    if(_S8 <= 0.0f)
    {

#line 408
        pixelOutput_0 _S9 = { float4(1.0f, encode_bent_0(float3(0.0f, 0.0f, 0.0f), 0.0f)) };



        return _S9;
    }

#line 412
    float _S10 = view_z_0(centre_texel_0, _S8, depth_size_0, &kernelContext_3);


    float _S11 = (&kernelContext_3)->camera_0->params_0.x * 2.0f;

#line 420
    float3 _S12 = float3(0.0f, 0.0f, 0.0f);

#line 420
    int y_0 = int(-1);

#line 420
    float total_0 = 0.0f;

#line 420
    float3 bent_0 = _S12;

#line 420
    float bent_weight_0 = 0.0f;

#line 420
    float weight_1 = 0.0f;

#line 426
    for(;;)
    {

#line 426
        if(y_0 < int(3))
        {
        }
        else
        {

#line 426
            break;
        }

#line 426
        int x_0 = int(-1);

        for(;;)
        {

#line 428
            if(x_0 < int(3))
            {
            }
            else
            {

#line 428
                break;
            }

#line 436
            int2 tap_0 = clamp(_S7 + int2(x_0, y_0), int2(int(0), int(0)), _S6 - int2(int(1), int(1)));

#line 436
            bool _S13;

#line 443
            if(x_0 != int(0))
            {

#line 443
                _S13 = true;

#line 443
            }
            else
            {

#line 443
                _S13 = y_0 != int(0);

#line 443
            }

#line 443
            float share_0;

#line 443
            if(_S13)
            {
                int2 texel_1 = full_res_pixel_0(tap_0);

#line 445
                float _S14 = depth_at_0(texel_1, depth_extent_0, &kernelContext_3);

#line 445
                float _S15 = view_z_0(texel_1, _S14, depth_size_0, &kernelContext_3);

                float away_0 = abs(_S15 - _S10);



                if(_S14 <= 0.0f)
                {

#line 451
                    share_0 = 0.0f;

#line 451
                }
                else
                {

#line 451
                    share_0 = saturate(1.0f - away_0 / _S11);

#line 451
                }

#line 443
            }
            else
            {

#line 443
                share_0 = 1.0f;

#line 443
            }

#line 453
            int3 _S16 = int3(tap_0, int(0));

#line 453
            float4 sample_0 = (((&kernelContext_3)->occlusion_0).read(vec<uint,2>(((_S16)).xy), uint(((_S16)).z)));
            float3 direction_1 = decode_bent_0(sample_0);

#line 460
            float total_1 = total_0 + sample_0.x * share_0;
            float3 bent_1 = bent_0 + direction_1 * float3(share_0) ;
            float bent_weight_1 = bent_weight_0 + dot(direction_1, direction_1) * share_0;
            float weight_2 = weight_1 + share_0;

#line 428
            x_0 = x_0 + int(1);

#line 428
            total_0 = total_1;

#line 428
            bent_0 = bent_1;

#line 428
            bent_weight_0 = bent_weight_1;

#line 428
            weight_1 = weight_2;

#line 428
        }

#line 426
        y_0 = y_0 + int(1);

#line 426
    }

#line 426
    pixelOutput_0 _S17 = { float4(total_0 / weight_1, encode_bent_0(bent_0, bent_weight_0)) };

#line 468
    return _S17;
}


#line 468
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 230
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 230
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 230
    thread KernelContext_0 kernelContext_4;

#line 230
    (&kernelContext_4)->occlusion_0 = occlusion_2;

#line 230
    (&kernelContext_4)->scene_depth_0 = scene_depth_2;

#line 230
    (&kernelContext_4)->camera_0 = camera_2;

#line 377
    thread FullscreenOutput_0 output_1;

    float2 _S18 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 379
    (&output_1)->uv_2 = _S18;
    (&output_1)->position_2 = float4(_S18 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 380
    thread vertexMain_Result_0 _S19;

#line 380
    (&_S19)->position_1 = output_1.position_2;

#line 380
    (&_S19)->uv_1 = output_1.uv_2;

#line 380
    return _S19;
}

