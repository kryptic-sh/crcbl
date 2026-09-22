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


#line 296 "shaders/ssao_upsample.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 299
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 381
float3 encode_bent_0(float3 summed_0, float weight_0)
{

#line 381
    float3 mean_0;

    if(weight_0 > 0.0f)
    {

#line 383
        mean_0 = summed_0 / float3(weight_0) ;

#line 383
    }
    else
    {

#line 383
        mean_0 = float3(0.0f, 0.0f, 0.0f);

#line 383
    }

#line 383
    float3 direction_0;

    if((length(mean_0)) < 0.5f)
    {

#line 385
        direction_0 = float3(0.0f, 0.0f, 0.0f);

#line 385
    }
    else
    {

#line 385
        direction_0 = normalize(mean_0);

#line 385
    }

#line 385
    float3 _S2 = float3(0.5f) ;

    return direction_0 * _S2 + _S2;
}


#line 310
float view_z_0(int2 pixel_1, float depth_0, float2 extent_1, KernelContext_0 thread* kernelContext_1)
{



    float4 view_0 = (((float4(float2((float(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (float(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_1->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_1->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.z / view_0.w;
}


#line 199
float sampling_radius_0(KernelContext_0 thread* kernelContext_2)
{
    float asked_0 = kernelContext_2->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 285
int2 full_res_pixel_0(int2 pixel_2)
{
    return pixel_2 * int2(int(2)) ;
}


#line 360
float3 decode_bent_0(float4 texel_0)
{
    float3 decoded_0 = texel_0.yzw * float3(2.0f)  - float3(1.0f) ;

#line 362
    float3 _S3;
    if((length(decoded_0)) < 0.5f)
    {

#line 363
        _S3 = float3(0.0f, 0.0f, 0.0f);

#line 363
    }
    else
    {

#line 363
        _S3 = normalize(decoded_0);

#line 363
    }

#line 363
    return _S3;
}


#line 337
float ao_intensity_0(KernelContext_0 thread* kernelContext_3)
{
    float asked_1 = kernelContext_3->camera_0->params_0.z;

#line 339
    float _S4;
    if(asked_1 == 0.0f)
    {

#line 340
        _S4 = 1.0f;

#line 340
    }
    else
    {

#line 340
        _S4 = clamp(asked_1, 0.25f, 4.0f);

#line 340
    }

#line 340
    return _S4;
}


#line 340
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 340
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 401
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S5 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 401
    float shown_0;

#line 401
    thread KernelContext_0 kernelContext_4;

#line 401
    (&kernelContext_4)->occlusion_0 = occlusion_1;

#line 401
    (&kernelContext_4)->scene_depth_0 = scene_depth_1;

#line 401
    (&kernelContext_4)->camera_0 = camera_1;

#line 407
    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S6 = int2(int(width_0), int(height_0));
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));

    int2 _S7 = int2(position_0.xy);

#line 417
    float _S8 = depth_at_0(_S7, depth_extent_0, &kernelContext_4);

#line 422
    if(_S8 <= 0.0f)
    {

#line 422
        pixelOutput_0 _S9 = { float4(1.0f, encode_bent_0(float3(0.0f, 0.0f, 0.0f), 0.0f)) };



        return _S9;
    }

#line 426
    float _S10 = view_z_0(_S7, _S8, depth_size_0, &kernelContext_4);

#line 426
    float _S11 = sampling_radius_0(&kernelContext_4);


    float _S12 = _S11 * 2.0f;

#line 437
    int2 nearest_0 = _S7 / int2(int(2)) ;
    int2 offset_0 = _S7 - full_res_pixel_0(nearest_0);
    float2 _S13 = float2(offset_0) / float2(2.0f) ;

#line 445
    int2 _S14 = int2(int(1), int(1));

#line 445
    int2 _S15 = min(offset_0, _S14);

#line 451
    float3 _S16 = float3(0.0f, 0.0f, 0.0f);

#line 451
    int y_0 = int(0);

#line 451
    float total_0 = 0.0f;

#line 451
    float3 bent_0 = _S16;

#line 451
    float bent_weight_0 = 0.0f;

#line 451
    float weight_1 = 0.0f;

    for(;;)
    {

#line 453
        if(y_0 <= (_S15.y))
        {
        }
        else
        {

#line 453
            break;
        }

#line 453
        int x_0 = int(0);

        for(;;)
        {

#line 455
            if(x_0 <= (_S15.x))
            {
            }
            else
            {

#line 455
                break;
            }

#line 462
            int2 tap_0 = clamp(nearest_0 + int2(x_0, y_0), int2(int(0), int(0)), _S6 - _S14);
            int2 texel_1 = full_res_pixel_0(tap_0);

#line 463
            float _S17 = depth_at_0(texel_1, depth_extent_0, &kernelContext_4);

#line 463
            float _S18 = view_z_0(texel_1, _S17, depth_size_0, &kernelContext_4);

            float away_0 = abs(_S18 - _S10);

#line 470
            bool _S19 = x_0 == int(0);

#line 470
            if(_S19)
            {

#line 470
                shown_0 = 1.0f - _S13.x;

#line 470
            }
            else
            {

#line 470
                shown_0 = _S13.x;

#line 470
            }
            bool _S20 = y_0 == int(0);

#line 471
            float _S21;

#line 471
            if(_S20)
            {

#line 471
                _S21 = 1.0f - _S13.y;

#line 471
            }
            else
            {

#line 471
                _S21 = _S13.y;

#line 471
            }
            float _S22 = shown_0 * _S21;

#line 472
            float _S23;
            if(_S17 <= 0.0f)
            {

#line 473
                _S23 = 0.0f;

#line 473
            }
            else
            {

#line 473
                _S23 = saturate(1.0f - away_0 / _S12);

#line 473
            }

#line 473
            float share_0 = _S22 * _S23;

#line 473
            bool _S24;



            if(_S19)
            {

#line 477
                _S24 = _S20;

#line 477
            }
            else
            {

#line 477
                _S24 = false;

#line 477
            }

#line 477
            float share_1;

#line 477
            if(_S24)
            {

#line 477
                share_1 = max(share_0, 0.000244140625f);

#line 477
            }
            else
            {

#line 477
                share_1 = share_0;

#line 477
            }
            int3 _S25 = int3(tap_0, int(0));

#line 478
            float4 sample_0 = (((&kernelContext_4)->occlusion_0).read(vec<uint,2>(((_S25)).xy), uint(((_S25)).z)));
            float3 direction_1 = decode_bent_0(sample_0);

#line 485
            float total_1 = total_0 + sample_0.x * share_1;
            float3 bent_1 = bent_0 + direction_1 * float3(share_1) ;
            float bent_weight_1 = bent_weight_0 + dot(direction_1, direction_1) * share_1;
            float weight_2 = weight_1 + share_1;

#line 455
            x_0 = x_0 + int(1);

#line 455
            total_0 = total_1;

#line 455
            bent_0 = bent_1;

#line 455
            bent_weight_0 = bent_weight_1;

#line 455
            weight_1 = weight_2;

#line 455
        }

#line 453
        y_0 = y_0 + int(1);

#line 453
    }

#line 492
    float visibility_0 = total_0 / weight_1;

#line 492
    float _S26 = ao_intensity_0(&kernelContext_4);

#line 500
    if(_S26 == 1.0f)
    {

#line 500
        shown_0 = visibility_0;

#line 500
    }
    else
    {

#line 500
        shown_0 = pow(visibility_0, _S26);

#line 500
    }

#line 500
    pixelOutput_0 _S27 = { float4(shown_0, encode_bent_0(bent_0, bent_weight_0)) };


    return _S27;
}


#line 503
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 273
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 273
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 273
    thread KernelContext_0 kernelContext_5;

#line 273
    (&kernelContext_5)->occlusion_0 = occlusion_2;

#line 273
    (&kernelContext_5)->scene_depth_0 = scene_depth_2;

#line 273
    (&kernelContext_5)->camera_0 = camera_2;

#line 393
    thread FullscreenOutput_0 output_1;

    float2 _S28 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 395
    (&output_1)->uv_2 = _S28;
    (&output_1)->position_2 = float4(_S28 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 396
    thread vertexMain_Result_0 _S29;

#line 396
    (&_S29)->position_1 = output_1.position_2;

#line 396
    (&_S29)->uv_1 = output_1.uv_2;

#line 396
    return _S29;
}

