#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 119 "shaders/ssao.slang"
constant array<float3, int(8)> KERNEL_0 = { float3(0.875f, 0.0f, 0.25f), float3(-0.75f, 0.0f, 0.375f), float3(0.0f, 0.75f, 0.25f), float3(0.0f, -0.625f, 0.5f), float3(0.5f, 0.5f, 0.375f), float3(-0.5f, 0.5f, 0.625f), float3(0.375f, -0.375f, 0.75f), float3(-0.25f, -0.25f, 0.875f) };

#line 143
constant array<float2, int(16)> ROTATIONS_0 = { float2(2.0f, 0.0f), float2(-2.0f, 0.0f), float2(1.0f, 1.0f), float2(-1.0f, -1.0f), float2(0.0f, -2.0f), float2(0.0f, 2.0f), float2(1.0f, -1.0f), float2(-1.0f, 1.0f), float2(1.0f, 2.0f), float2(-1.0f, -2.0f), float2(2.0f, 1.0f), float2(-2.0f, -1.0f), float2(2.0f, -1.0f), float2(-2.0f, 1.0f), float2(1.0f, -2.0f), float2(-1.0f, 2.0f) };

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
    depth2d<float, access::sample> scene_depth_0;
    SsaoParams_natural_0 constant* ssao_0;
};


#line 162 "shaders/ssao.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 165
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 162
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 165
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 174
float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 184
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->ssao_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->ssao_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 174
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 184
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->ssao_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->ssao_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}


#line 200
float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 202
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 202
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 203
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 203
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 204
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 204
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 205
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 205
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 207
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 208
        horizontal_0 = _S8 - centre_0;

#line 208
    }
    else
    {

#line 208
        horizontal_0 = centre_0 - _S5;

#line 208
    }

#line 208
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 211
        vertical_0 = _S14 - centre_0;

#line 211
    }
    else
    {

#line 211
        vertical_0 = centre_0 - _S11;

#line 211
    }

#line 221
    return normalize(cross(vertical_0, horizontal_0));
}




float occlusion_at_0(int2 pixel_5, float3 centre_1, float3 normal_0, int2 extent_5, float2 size_1, KernelContext_0 thread* kernelContext_5)
{
    float _S16 = kernelContext_5->ssao_0->params_0.x;
    float _S17 = kernelContext_5->ssao_0->params_0.y;

#line 238
    float3 seed_0 = float3(ROTATIONS_0[(uint(pixel_5.y) & 3U) * 4U + (uint(pixel_5.x) & 3U)], 0.0f);
    float3 tangent_0 = seed_0 - normal_0 * float3(dot(seed_0, normal_0)) ;

#line 239
    float3 across_0;



    if((dot(tangent_0, tangent_0)) > 9.99999993922529029e-09f)
    {

#line 243
        across_0 = normalize(tangent_0);

#line 243
    }
    else
    {

#line 243
        across_0 = float3(1.0f, 0.0f, 0.0f);

#line 243
    }
    float3 _S18 = cross(normal_0, across_0);

#line 244
    uint index_0 = 0U;

#line 244
    float blocked_0 = 0.0f;


    for(;;)
    {

#line 247
        if(index_0 < 8U)
        {
        }
        else
        {

#line 247
            break;
        }

        float3 at_0 = centre_1 + (across_0 * float3(KERNEL_0[index_0].x)  + _S18 * float3(KERNEL_0[index_0].y)  + normal_0 * float3(KERNEL_0[index_0].z) ) * float3(_S16) ;

        float4 clip_0 = (((float4(at_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_5->ssao_0->proj_0.data_0[int(0)][int(0)], kernelContext_5->ssao_0->proj_0.data_0[int(1)][int(0)], kernelContext_5->ssao_0->proj_0.data_0[int(2)][int(0)], kernelContext_5->ssao_0->proj_0.data_0[int(3)][int(0)], kernelContext_5->ssao_0->proj_0.data_0[int(0)][int(1)], kernelContext_5->ssao_0->proj_0.data_0[int(1)][int(1)], kernelContext_5->ssao_0->proj_0.data_0[int(2)][int(1)], kernelContext_5->ssao_0->proj_0.data_0[int(3)][int(1)], kernelContext_5->ssao_0->proj_0.data_0[int(0)][int(2)], kernelContext_5->ssao_0->proj_0.data_0[int(1)][int(2)], kernelContext_5->ssao_0->proj_0.data_0[int(2)][int(2)], kernelContext_5->ssao_0->proj_0.data_0[int(3)][int(2)], kernelContext_5->ssao_0->proj_0.data_0[int(0)][int(3)], kernelContext_5->ssao_0->proj_0.data_0[int(1)][int(3)], kernelContext_5->ssao_0->proj_0.data_0[int(2)][int(3)], kernelContext_5->ssao_0->proj_0.data_0[int(3)][int(3)]))));

        float _S19 = clip_0.w;

#line 254
        if(_S19 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 247
            continue;
        }

#line 258
        float2 ndc_0 = clip_0.xy / float2(_S19) ;

        int _S20 = int((ndc_0.x * 0.5f + 0.5f) * size_1.x);
        int _S21 = int((0.5f - ndc_0.y * 0.5f) * size_1.y);

#line 259
        int2 tap_0 = int2(_S20, _S21);

#line 259
        bool _S22;

#line 265
        if(_S20 < int(0))
        {

#line 265
            _S22 = true;

#line 265
        }
        else
        {

#line 265
            _S22 = _S21 < int(0);

#line 265
        }

#line 265
        bool _S23;

#line 265
        if(_S22)
        {

#line 265
            _S23 = true;

#line 265
        }
        else
        {

#line 265
            _S23 = _S20 >= (extent_5.x);

#line 265
        }

#line 265
        bool _S24;

#line 265
        if(_S23)
        {

#line 265
            _S24 = true;

#line 265
        }
        else
        {

#line 265
            _S24 = _S21 >= (extent_5.y);

#line 265
        }

#line 265
        if(_S24)
        {
            index_0 = index_0 + 1U;

#line 247
            continue;
        }

#line 247
        float _S25 = depth_at_0(tap_0, extent_5, kernelContext_5);

#line 271
        if(_S25 <= 0.0f)
        {
            index_0 = index_0 + 1U;

#line 247
            continue;
        }

#line 247
        float3 _S26 = view_position_0(tap_0, _S25, size_1, kernelContext_5);

#line 280
        float _S27 = _S26.z;

#line 280
        float blocked_1;

#line 280
        if(_S27 >= (at_0.z + _S17))
        {

#line 280
            blocked_1 = blocked_0 + saturate(_S16 / max(abs(centre_1.z - _S27), 0.00000999999974738f));

#line 280
        }
        else
        {

#line 280
            blocked_1 = blocked_0;

#line 280
        }

#line 280
        blocked_0 = blocked_1;

#line 247
        index_0 = index_0 + 1U;

#line 247
    }

#line 289
    return blocked_0 / 8.0f;
}


#line 289
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 289
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 304
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S28 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], SsaoParams_natural_0 constant* ssao_1 [[buffer(0)]])
{

#line 304
    thread KernelContext_0 kernelContext_6;

#line 304
    (&kernelContext_6)->scene_depth_0 = scene_depth_1;

#line 304
    (&kernelContext_6)->ssao_0 = ssao_1;

    thread uint width_0;
    thread uint height_0;



    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_6 = int2(int(width_0), int(height_0));
    float2 size_2 = float2(float(width_0), float(height_0));

    int2 _S29 = int2(position_0.xy);

#line 315
    float _S30 = depth_at_0(_S29, extent_6, &kernelContext_6);



    if(_S30 <= 0.0f)
    {

#line 319
        pixelOutput_0 _S31 = { 1.0f };

        return _S31;
    }

#line 321
    float3 _S32 = view_position_0(_S29, _S30, size_2, &kernelContext_6);

#line 321
    float3 _S33 = normal_at_0(_S29, _S32, extent_6, size_2, &kernelContext_6);

#line 321
    float _S34 = occlusion_at_0(_S29, _S32, _S33, extent_6, size_2, &kernelContext_6);

#line 321
    pixelOutput_0 _S35 = { saturate(1.0f - _S34) };

#line 326
    return _S35;
}


#line 326
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 150
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 150
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], SsaoParams_natural_0 constant* ssao_2 [[buffer(0)]])
{

#line 150
    thread KernelContext_0 kernelContext_7;

#line 150
    (&kernelContext_7)->scene_depth_0 = scene_depth_2;

#line 150
    (&kernelContext_7)->ssao_0 = ssao_2;

#line 295
    thread FullscreenOutput_0 output_1;


    float2 _S36 = float2(float((index_1 << 1U) & 2U), float(index_1 & 2U));

#line 298
    (&output_1)->uv_2 = _S36;
    (&output_1)->position_2 = float4(_S36 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 299
    thread vertexMain_Result_0 _S37;

#line 299
    (&_S37)->position_1 = output_1.position_2;

#line 299
    (&_S37)->uv_1 = output_1.uv_2;

#line 299
    return _S37;
}

