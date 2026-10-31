#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 204 "shaders/ssao_blur_bentless.slang"
int2 full_res_pixel_0(int2 pixel_0)
{
    return pixel_0 * int2(int(2)) ;
}


#line 65
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 65
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


#line 215 "shaders/ssao_blur_bentless.slang"
float depth_at_0(int2 pixel_1, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 218
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 236
float2 unproject_z_0(float depth_0, KernelContext_0 thread* kernelContext_1)
{
    return float2((&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].z * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].z, (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(2)].w * depth_0 + (&kernelContext_1->camera_0->inv_proj_0)->data_0[int(3)].w);
}


#line 285
float view_z_0(int2 pixel_2, float depth_1, float2 extent_1, KernelContext_0 thread* kernelContext_2)
{

#line 285
    float2 _S2 = unproject_z_0(depth_1, kernelContext_2);

#line 291
    return _S2.x / _S2.y;
}


#line 171
float sampling_radius_0(KernelContext_0 thread* kernelContext_3)
{
    float asked_0 = kernelContext_3->camera_0->params_0.x;
    if(asked_0 <= 0.0f)
    {
        return 0.5f;
    }
    return clamp(asked_0, 0.0625f, 4.0f);
}


#line 178
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 178
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 305
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S3 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 305
    thread KernelContext_0 kernelContext_4;

#line 305
    (&kernelContext_4)->occlusion_0 = occlusion_1;

#line 305
    (&kernelContext_4)->scene_depth_0 = scene_depth_1;

#line 305
    (&kernelContext_4)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S4 = int2(int(width_0), int(height_0));

#line 316
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));
    int2 _S5 = int2(position_0.xy);
    int2 centre_texel_0 = full_res_pixel_0(_S5);

#line 322
    float _S6 = depth_at_0(centre_texel_0, depth_extent_0, &kernelContext_4);

#line 328
    if(_S6 <= 0.0f)
    {

#line 328
        pixelOutput_0 _S7 = { float4(1.0f, float3(0.50196081399917603f, 0.50196081399917603f, 0.50196081399917603f)) };



        return _S7;
    }

#line 332
    float _S8 = view_z_0(centre_texel_0, _S6, depth_size_0, &kernelContext_4);

#line 332
    float _S9 = sampling_radius_0(&kernelContext_4);


    float _S10 = _S9 * 2.0f;

#line 335
    int y_0 = int(-1);

#line 335
    float total_0 = 0.0f;

#line 335
    float weight_0 = 0.0f;



    for(;;)
    {

#line 339
        if(y_0 < int(3))
        {
        }
        else
        {

#line 339
            break;
        }

#line 339
        int x_0 = int(-1);

        for(;;)
        {

#line 341
            if(x_0 < int(3))
            {
            }
            else
            {

#line 341
                break;
            }

#line 349
            int2 tap_0 = clamp(_S5 + int2(x_0, y_0), int2(int(0), int(0)), _S4 - int2(int(1), int(1)));

#line 349
            bool _S11;

#line 356
            if(x_0 != int(0))
            {

#line 356
                _S11 = true;

#line 356
            }
            else
            {

#line 356
                _S11 = y_0 != int(0);

#line 356
            }

#line 356
            float share_0;

#line 356
            if(_S11)
            {
                int2 texel_0 = full_res_pixel_0(tap_0);

#line 358
                float _S12 = depth_at_0(texel_0, depth_extent_0, &kernelContext_4);

#line 358
                float _S13 = view_z_0(texel_0, _S12, depth_size_0, &kernelContext_4);

                float away_0 = abs(_S13 - _S8);



                if(_S12 <= 0.0f)
                {

#line 364
                    share_0 = 0.0f;

#line 364
                }
                else
                {

#line 364
                    share_0 = saturate(1.0f - away_0 / _S10);

#line 364
                }

#line 356
            }
            else
            {

#line 356
                share_0 = 1.0f;

#line 356
            }

#line 366
            int3 _S14 = int3(tap_0, int(0));
            float total_1 = total_0 + (((&kernelContext_4)->occlusion_0).read(vec<uint,2>(((_S14)).xy), uint(((_S14)).z))).x * share_0;
            float weight_1 = weight_0 + share_0;

#line 341
            x_0 = x_0 + int(1);

#line 341
            total_0 = total_1;

#line 341
            weight_0 = weight_1;

#line 341
        }

#line 339
        y_0 = y_0 + int(1);

#line 339
    }

#line 339
    pixelOutput_0 _S15 = { float4(total_0 / weight_0, float3(0.50196081399917603f, 0.50196081399917603f, 0.50196081399917603f)) };

#line 372
    return _S15;
}


#line 372
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 193
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 193
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 193
    thread KernelContext_0 kernelContext_5;

#line 193
    (&kernelContext_5)->occlusion_0 = occlusion_2;

#line 193
    (&kernelContext_5)->scene_depth_0 = scene_depth_2;

#line 193
    (&kernelContext_5)->camera_0 = camera_2;

#line 297
    thread FullscreenOutput_0 output_1;

    float2 _S16 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 299
    (&output_1)->uv_2 = _S16;
    (&output_1)->position_2 = float4(_S16 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 300
    thread vertexMain_Result_0 _S17;

#line 300
    (&_S17)->position_1 = output_1.position_2;

#line 300
    (&_S17)->uv_1 = output_1.uv_2;

#line 300
    return _S17;
}

