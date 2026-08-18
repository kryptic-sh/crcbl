#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 93 "shaders/push_constant_raster.slang"
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 72
struct RasterConstants_0
{
    float4 color_0;
    float4 rect_0;
};


#line 147
[[fragment]] pixelOutput_0 fragmentMain(float4 position_0 [[position]], RasterConstants_0 constant* constants_0 [[buffer(0)]])
{

#line 147
    pixelOutput_0 _S1 = { constants_0->color_0 };

    return _S1;
}


#line 149
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
};


#line 93
struct VertexOutput_0
{
    float4 position_2;
};


#line 93
struct KernelContext_0
{
    RasterConstants_0 constant* constants_1;
};


#line 93
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], RasterConstants_0 constant* constants_2 [[buffer(0)]])
{

#line 93
    VertexOutput_0 _S2;

#line 93
    thread KernelContext_0 kernelContext_0;

#line 93
    (&kernelContext_0)->constants_1 = constants_2;

#line 93
    for(;;)
    {

#line 93
        uint corner_0;

#line 114
        switch(index_0)
        {
        case 1U:
            {

#line 114
                corner_0 = 1U;



                break;
            }
        case 2U:
        case 4U:
            {

#line 118
                corner_0 = 2U;



                break;
            }
        case 5U:
            {

#line 122
                corner_0 = 3U;


                break;
            }
        default:
            {

#line 125
                corner_0 = 0U;

#line 131
                break;
            }
        }

#line 131
        bool atMaxX_0;


        if(corner_0 == 1U)
        {

#line 134
            atMaxX_0 = true;

#line 134
        }
        else
        {

#line 134
            atMaxX_0 = corner_0 == 2U;

#line 134
        }

#line 134
        bool atMaxY_0;
        if(corner_0 == 2U)
        {

#line 135
            atMaxY_0 = true;

#line 135
        }
        else
        {

#line 135
            atMaxY_0 = corner_0 == 3U;

#line 135
        }

        thread VertexOutput_0 output_1;

#line 137
        float _S3;

        if(atMaxX_0)
        {

#line 139
            _S3 = (&kernelContext_0)->constants_1->rect_0.z;

#line 139
        }
        else
        {

#line 139
            _S3 = (&kernelContext_0)->constants_1->rect_0.x;

#line 139
        }

#line 139
        float _S4;
        if(atMaxY_0)
        {

#line 140
            _S4 = (&kernelContext_0)->constants_1->rect_0.w;

#line 140
        }
        else
        {

#line 140
            _S4 = (&kernelContext_0)->constants_1->rect_0.y;

#line 140
        }

#line 138
        (&output_1)->position_2 = float4(_S3, _S4, 0.5f, 1.0f);

#line 138
        _S2 = output_1;

#line 143
        break;
    }

#line 143
    thread vertexMain_Result_0 _S5;

#line 143
    (&_S5)->position_1 = _S2.position_2;

#line 143
    return _S5;
}

