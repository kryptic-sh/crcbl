#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 90 "core"
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 2578 "core.meta.slang"
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 2580
struct KernelContext_0
{
    texture2d<float, access::sample> occlusion_0;
};


#line 69 "shaders/ssao_blur.slang"
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> occlusion_1 [[texture(0)]])
{

#line 69
    thread KernelContext_0 kernelContext_0;

#line 69
    (&kernelContext_0)->occlusion_0 = occlusion_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (occlusion_1).get_width(0)),(*((&height_0)) = (occlusion_1).get_height(0));
    int2 _S2 = int2(int(width_0), int(height_0));
    int2 _S3 = int2(position_0.xy);

#line 75
    int y_0 = int(-1);

#line 75
    float total_0 = 0.0f;


    for(;;)
    {

#line 78
        if(y_0 < int(3))
        {
        }
        else
        {

#line 78
            break;
        }

#line 78
        int x_0 = int(-1);

        for(;;)
        {

#line 80
            if(x_0 < int(3))
            {
            }
            else
            {

#line 80
                break;
            }

#line 89
            int3 _S4 = int3(clamp(_S3 + int2(x_0, y_0), int2(int(0), int(0)), _S2 - int2(int(1), int(1))), int(0));

#line 89
            float total_1 = total_0 + (((&kernelContext_0)->occlusion_0).read(vec<uint,2>(((_S4)).xy), uint(((_S4)).z)).x);

#line 80
            x_0 = x_0 + int(1);

#line 80
            total_0 = total_1;

#line 80
        }

#line 78
        y_0 = y_0 + int(1);

#line 78
    }

#line 78
    pixelOutput_0 _S5 = { total_0 / 16.0f };

#line 93
    return _S5;
}


#line 93
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 52
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 52
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> occlusion_2 [[texture(0)]])
{

#line 61
    thread FullscreenOutput_0 output_1;

    float2 _S6 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 63
    (&output_1)->uv_2 = _S6;
    (&output_1)->position_2 = float4(_S6 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 64
    thread vertexMain_Result_0 _S7;

#line 64
    (&_S7)->position_1 = output_1.position_2;

#line 64
    (&_S7)->uv_1 = output_1.uv_2;

#line 64
    return _S7;
}

