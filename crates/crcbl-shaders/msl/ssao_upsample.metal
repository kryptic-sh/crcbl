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


#line 179 "shaders/ssao_upsample.slang"
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


#line 168
int2 full_res_pixel_0(int2 pixel_2)
{
    return pixel_2 * int2(int(2)) ;
}


#line 170
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 170
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 213
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 213
    thread KernelContext_0 kernelContext_2;

#line 213
    (&kernelContext_2)->occlusion_0 = occlusion_1;

#line 213
    (&kernelContext_2)->scene_depth_0 = scene_depth_1;

#line 213
    (&kernelContext_2)->camera_0 = camera_1;

#line 219
    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S3 = int2(int(width_0), int(height_0));
    thread uint depth_width_0;
    thread uint depth_height_0;
    (*((&depth_width_0)) = (scene_depth_1).get_width(0)),(*((&depth_height_0)) = (scene_depth_1).get_height(0));
    int2 depth_extent_0 = int2(int(depth_width_0), int(depth_height_0));
    float2 depth_size_0 = float2(float(depth_width_0), float(depth_height_0));

    int2 _S4 = int2(position_0.xy);

#line 229
    float _S5 = depth_at_0(_S4, depth_extent_0, &kernelContext_2);

#line 234
    if(_S5 <= 0.0f)
    {

#line 234
        pixelOutput_0 _S6 = { 1.0f };

        return _S6;
    }

#line 236
    float _S7 = view_z_0(_S4, _S5, depth_size_0, &kernelContext_2);


    float _S8 = (&kernelContext_2)->camera_0->params_0.x * 2.0f;

#line 247
    int2 nearest_0 = _S4 / int2(int(2)) ;
    int2 offset_0 = _S4 - full_res_pixel_0(nearest_0);
    float2 _S9 = float2(offset_0) / float2(2.0f) ;

#line 255
    int2 _S10 = int2(int(1), int(1));

#line 255
    int2 _S11 = min(offset_0, _S10);

#line 255
    int y_0 = int(0);

#line 255
    float total_0 = 0.0f;

#line 255
    float weight_0 = 0.0f;



    for(;;)
    {

#line 259
        if(y_0 <= (_S11.y))
        {
        }
        else
        {

#line 259
            break;
        }

#line 259
        int x_0 = int(0);

        for(;;)
        {

#line 261
            if(x_0 <= (_S11.x))
            {
            }
            else
            {

#line 261
                break;
            }

#line 268
            int2 tap_0 = clamp(nearest_0 + int2(x_0, y_0), int2(int(0), int(0)), _S3 - _S10);
            int2 texel_0 = full_res_pixel_0(tap_0);

#line 269
            float _S12 = depth_at_0(texel_0, depth_extent_0, &kernelContext_2);

#line 269
            float _S13 = view_z_0(texel_0, _S12, depth_size_0, &kernelContext_2);

            float away_0 = abs(_S13 - _S7);

#line 276
            bool _S14 = x_0 == int(0);

#line 276
            float _S15;

#line 276
            if(_S14)
            {

#line 276
                _S15 = 1.0f - _S9.x;

#line 276
            }
            else
            {

#line 276
                _S15 = _S9.x;

#line 276
            }
            bool _S16 = y_0 == int(0);

#line 277
            float _S17;

#line 277
            if(_S16)
            {

#line 277
                _S17 = 1.0f - _S9.y;

#line 277
            }
            else
            {

#line 277
                _S17 = _S9.y;

#line 277
            }
            float _S18 = _S15 * _S17;

#line 278
            float _S19;
            if(_S12 <= 0.0f)
            {

#line 279
                _S19 = 0.0f;

#line 279
            }
            else
            {

#line 279
                _S19 = saturate(1.0f - away_0 / _S8);

#line 279
            }

#line 279
            float share_0 = _S18 * _S19;

#line 279
            bool _S20;



            if(_S14)
            {

#line 283
                _S20 = _S16;

#line 283
            }
            else
            {

#line 283
                _S20 = false;

#line 283
            }

#line 283
            float share_1;

#line 283
            if(_S20)
            {

#line 283
                share_1 = max(share_0, 0.000244140625f);

#line 283
            }
            else
            {

#line 283
                share_1 = share_0;

#line 283
            }
            int3 _S21 = int3(tap_0, int(0));

#line 284
            float total_1 = total_0 + (((&kernelContext_2)->occlusion_0).read(vec<uint,2>(((_S21)).xy), uint(((_S21)).z)).x) * share_1;
            float weight_1 = weight_0 + share_1;

#line 261
            x_0 = x_0 + int(1);

#line 261
            total_0 = total_1;

#line 261
            weight_0 = weight_1;

#line 261
        }

#line 259
        y_0 = y_0 + int(1);

#line 259
    }

#line 259
    pixelOutput_0 _S22 = { total_0 / weight_0 };

#line 289
    return _S22;
}


#line 289
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 156
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 156
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 156
    thread KernelContext_0 kernelContext_3;

#line 156
    (&kernelContext_3)->occlusion_0 = occlusion_2;

#line 156
    (&kernelContext_3)->scene_depth_0 = scene_depth_2;

#line 156
    (&kernelContext_3)->camera_0 = camera_2;

#line 205
    thread FullscreenOutput_0 output_1;

    float2 _S23 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 207
    (&output_1)->uv_2 = _S23;
    (&output_1)->position_2 = float4(_S23 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 208
    thread vertexMain_Result_0 _S24;

#line 208
    (&_S24)->position_1 = output_1.position_2;

#line 208
    (&_S24)->uv_1 = output_1.uv_2;

#line 208
    return _S24;
}

