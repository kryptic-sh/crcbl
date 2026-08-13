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
    SsaoParams_natural_0 constant* ssao_0;
};


#line 167 "shaders/ssao_blur.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 170
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 181
float view_z_0(int2 pixel_1, float depth_0, float2 extent_1, KernelContext_0 thread* kernelContext_1)
{



    float4 view_0 = (((float4(float2((float(pixel_1.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (float(pixel_1.y) + 0.5f) / extent_1.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_1->ssao_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_1->ssao_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.z / view_0.w;
}


#line 187
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 187
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 201
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S2 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(1)]], SsaoParams_natural_0 constant* ssao_1 [[buffer(0)]])
{

#line 201
    thread KernelContext_0 kernelContext_2;

#line 201
    (&kernelContext_2)->occlusion_0 = occlusion_1;

#line 201
    (&kernelContext_2)->scene_depth_0 = scene_depth_1;

#line 201
    (&kernelContext_2)->ssao_0 = ssao_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 extent_2 = int2(int(width_0), int(height_0));
    float2 size_0 = float2(float(width_0), float(height_0));
    int2 _S3 = int2(position_0.xy);

#line 208
    float _S4 = depth_at_0(_S3, extent_2, &kernelContext_2);

#line 214
    if(_S4 <= 0.0f)
    {

#line 214
        pixelOutput_0 _S5 = { 1.0f };

        return _S5;
    }

#line 216
    float _S6 = view_z_0(_S3, _S4, size_0, &kernelContext_2);


    float _S7 = (&kernelContext_2)->ssao_0->params_0.x * 2.0f;

#line 219
    int y_0 = int(-1);

#line 219
    float total_0 = 0.0f;

#line 219
    float weight_0 = 0.0f;



    for(;;)
    {

#line 223
        if(y_0 < int(3))
        {
        }
        else
        {

#line 223
            break;
        }

#line 223
        int x_0 = int(-1);

        for(;;)
        {

#line 225
            if(x_0 < int(3))
            {
            }
            else
            {

#line 225
                break;
            }

#line 233
            int2 tap_0 = clamp(_S3 + int2(x_0, y_0), int2(int(0), int(0)), extent_2 - int2(int(1), int(1)));

#line 233
            bool _S8;

#line 240
            if(x_0 != int(0))
            {

#line 240
                _S8 = true;

#line 240
            }
            else
            {

#line 240
                _S8 = y_0 != int(0);

#line 240
            }

#line 240
            float share_0;

#line 240
            if(_S8)
            {

#line 240
                float _S9 = depth_at_0(tap_0, extent_2, &kernelContext_2);

#line 240
                float _S10 = view_z_0(tap_0, _S9, size_0, &kernelContext_2);


                float away_0 = abs(_S10 - _S6);



                if(_S9 <= 0.0f)
                {

#line 247
                    share_0 = 0.0f;

#line 247
                }
                else
                {

#line 247
                    share_0 = saturate(1.0f - away_0 / _S7);

#line 247
                }

#line 240
            }
            else
            {

#line 240
                share_0 = 1.0f;

#line 240
            }

#line 249
            int3 _S11 = int3(tap_0, int(0));

#line 249
            float total_1 = total_0 + (((&kernelContext_2)->occlusion_0).read(vec<uint,2>(((_S11)).xy), uint(((_S11)).z)).x) * share_0;
            float weight_1 = weight_0 + share_0;

#line 225
            x_0 = x_0 + int(1);

#line 225
            total_0 = total_1;

#line 225
            weight_0 = weight_1;

#line 225
        }

#line 223
        y_0 = y_0 + int(1);

#line 223
    }

#line 223
    pixelOutput_0 _S12 = { total_0 / weight_0 };

#line 254
    return _S12;
}


#line 254
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 155
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 155
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(1)]], SsaoParams_natural_0 constant* ssao_2 [[buffer(0)]])
{

#line 155
    thread KernelContext_0 kernelContext_3;

#line 155
    (&kernelContext_3)->occlusion_0 = occlusion_2;

#line 155
    (&kernelContext_3)->scene_depth_0 = scene_depth_2;

#line 155
    (&kernelContext_3)->ssao_0 = ssao_2;

#line 193
    thread FullscreenOutput_0 output_1;

    float2 _S13 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 195
    (&output_1)->uv_2 = _S13;
    (&output_1)->position_2 = float4(_S13 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 196
    thread vertexMain_Result_0 _S14;

#line 196
    (&_S14)->position_1 = output_1.position_2;

#line 196
    (&_S14)->uv_1 = output_1.uv_2;

#line 196
    return _S14;
}

