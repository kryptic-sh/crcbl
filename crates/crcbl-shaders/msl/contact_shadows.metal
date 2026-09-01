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
struct ContactShadowParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 proj_0;
    float4 to_light_0;
};


#line 1084
struct KernelContext_0
{
    depth2d<float, access::sample> scene_depth_0;
    ContactShadowParams_natural_0 constant* camera_0;
};


#line 173 "shaders/contact_shadows.slang"
float depth_at_0(int2 pixel_0, int2 extent_0, KernelContext_0 thread* kernelContext_0)
{

    int3 _S1 = int3(clamp(pixel_0, int2(int(0), int(0)), extent_0 - int2(int(1), int(1))), int(0));

#line 176
    return ((kernelContext_0->scene_depth_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 173
float depth_at_1(int2 pixel_1, int2 extent_1, KernelContext_0 thread* kernelContext_1)
{

    int3 _S2 = int3(clamp(pixel_1, int2(int(0), int(0)), extent_1 - int2(int(1), int(1))), int(0));

#line 176
    return ((kernelContext_1->scene_depth_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}

float3 view_position_0(int2 pixel_2, float depth_0, float2 extent_2, KernelContext_0 thread* kernelContext_2)
{

#line 189
    float4 view_0 = (((float4(float2((float(pixel_2.x) + 0.5f) / extent_2.x * 2.0f - 1.0f, 1.0f - (float(pixel_2.y) + 0.5f) / extent_2.y * 2.0f), depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_2->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_2->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_0.xyz / float3(view_0.w) ;
}


#line 179
float3 view_position_1(int2 pixel_3, float depth_1, float2 extent_3, KernelContext_0 thread* kernelContext_3)
{

#line 189
    float4 view_1 = (((float4(float2((float(pixel_3.x) + 0.5f) / extent_3.x * 2.0f - 1.0f, 1.0f - (float(pixel_3.y) + 0.5f) / extent_3.y * 2.0f), depth_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_3->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_3->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_1.xyz / float3(view_1.w) ;
}

float3 normal_at_0(int2 pixel_4, float3 centre_0, int2 extent_4, float2 size_0, KernelContext_0 thread* kernelContext_4)
{
    int2 _S3 = pixel_4 + int2(int(-1), int(0));

#line 195
    float _S4 = depth_at_1(_S3, extent_4, kernelContext_4);

#line 195
    float3 _S5 = view_position_1(_S3, _S4, size_0, kernelContext_4);
    int2 _S6 = pixel_4 + int2(int(1), int(0));

#line 196
    float _S7 = depth_at_1(_S6, extent_4, kernelContext_4);

#line 196
    float3 _S8 = view_position_1(_S6, _S7, size_0, kernelContext_4);
    int2 _S9 = pixel_4 + int2(int(0), int(-1));

#line 197
    float _S10 = depth_at_1(_S9, extent_4, kernelContext_4);

#line 197
    float3 _S11 = view_position_1(_S9, _S10, size_0, kernelContext_4);
    int2 _S12 = pixel_4 + int2(int(0), int(1));

#line 198
    float _S13 = depth_at_1(_S12, extent_4, kernelContext_4);

#line 198
    float3 _S14 = view_position_1(_S12, _S13, size_0, kernelContext_4);

    float _S15 = centre_0.z;

#line 200
    float3 horizontal_0;
    if((abs(_S8.z - _S15)) < (abs(_S15 - _S5.z)))
    {

#line 201
        horizontal_0 = _S8 - centre_0;

#line 201
    }
    else
    {

#line 201
        horizontal_0 = centre_0 - _S5;

#line 201
    }

#line 201
    float3 vertical_0;


    if((abs(_S14.z - _S15)) < (abs(_S15 - _S11.z)))
    {

#line 204
        vertical_0 = _S14 - centre_0;

#line 204
    }
    else
    {

#line 204
        vertical_0 = centre_0 - _S11;

#line 204
    }

#line 214
    return normalize(cross(vertical_0, horizontal_0));
}

float2 pixel_of_0(float2 ndc_0, float2 size_1)
{
    return float2((ndc_0.x * 0.5f + 0.5f) * size_1.x, (0.5f - ndc_0.y * 0.5f) * size_1.y);
}

float2 ndc_of_0(float2 at_0, float2 size_2)
{
    return float2(at_0.x / size_2.x * 2.0f - 1.0f, 1.0f - at_0.y / size_2.y * 2.0f);
}


#line 238
float cell_exit_0(float2 at_1, float2 forward_0, float size_3, float reach_0)
{

    float _S16 = forward_0.x;

#line 241
    bool _S17 = _S16 > 0.0f;

#line 241
    float along_x_0;

#line 241
    if(_S17)
    {

#line 241
        along_x_0 = (floor(at_1.x / size_3) + 1.0f) * size_3;

#line 241
    }
    else
    {

#line 241
        along_x_0 = floor(at_1.x / size_3) * size_3;

#line 241
    }
    float _S18 = forward_0.y;

#line 242
    bool _S19 = _S18 > 0.0f;

#line 242
    float along_y_0;

#line 242
    if(_S19)
    {

#line 242
        along_y_0 = (floor(at_1.y / size_3) + 1.0f) * size_3;

#line 242
    }
    else
    {

#line 242
        along_y_0 = floor(at_1.y / size_3) * size_3;

#line 242
    }
    float nudge_0 = size_3 * 0.00390625f;

#line 243
    float _S20;

    if((abs(_S16)) < 9.99999997475242708e-07f)
    {

#line 245
        along_x_0 = reach_0;

#line 245
    }
    else
    {

#line 246
        if(_S17)
        {

#line 246
            _S20 = nudge_0;

#line 246
        }
        else
        {

#line 246
            _S20 = - nudge_0;

#line 246
        }

#line 246
        along_x_0 = (along_x_0 + _S20 - at_1.x) / _S16;

#line 245
    }


    if((abs(_S18)) < 9.99999997475242708e-07f)
    {

#line 248
        along_y_0 = reach_0;

#line 248
    }
    else
    {

#line 249
        if(_S19)
        {

#line 249
            _S20 = nudge_0;

#line 249
        }
        else
        {

#line 249
            _S20 = - nudge_0;

#line 249
        }

#line 249
        along_y_0 = (along_y_0 + _S20 - at_1.y) / _S18;

#line 248
    }

    return max(min(along_x_0, along_y_0), nudge_0);
}


#line 232
float view_z_of_0(float depth_2, KernelContext_0 thread* kernelContext_5)
{
    float4 view_2 = (((float4(0.0f, 0.0f, depth_2, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_5->camera_0->inv_proj_0.data_0[int(0)][int(0)], kernelContext_5->camera_0->inv_proj_0.data_0[int(1)][int(0)], kernelContext_5->camera_0->inv_proj_0.data_0[int(2)][int(0)], kernelContext_5->camera_0->inv_proj_0.data_0[int(3)][int(0)], kernelContext_5->camera_0->inv_proj_0.data_0[int(0)][int(1)], kernelContext_5->camera_0->inv_proj_0.data_0[int(1)][int(1)], kernelContext_5->camera_0->inv_proj_0.data_0[int(2)][int(1)], kernelContext_5->camera_0->inv_proj_0.data_0[int(3)][int(1)], kernelContext_5->camera_0->inv_proj_0.data_0[int(0)][int(2)], kernelContext_5->camera_0->inv_proj_0.data_0[int(1)][int(2)], kernelContext_5->camera_0->inv_proj_0.data_0[int(2)][int(2)], kernelContext_5->camera_0->inv_proj_0.data_0[int(3)][int(2)], kernelContext_5->camera_0->inv_proj_0.data_0[int(0)][int(3)], kernelContext_5->camera_0->inv_proj_0.data_0[int(1)][int(3)], kernelContext_5->camera_0->inv_proj_0.data_0[int(2)][int(3)], kernelContext_5->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));
    return view_2.z / view_2.w;
}


#line 227
float thickness_at_0(float advance_0, float depth_3)
{
    return max(advance_0, abs(depth_3) * 0.01999999955296516f);
}


#line 229
struct pixelOutput_0
{
    float output_0 [[color(0)]];
};


#line 229
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 263
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S21 [[stage_in]], float4 position_0 [[position]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], ContactShadowParams_natural_0 constant* camera_1 [[buffer(0)]])
{

#line 263
    thread KernelContext_0 kernelContext_6;

#line 263
    (&kernelContext_6)->scene_depth_0 = scene_depth_1;

#line 263
    (&kernelContext_6)->camera_0 = camera_1;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (scene_depth_1).get_width(0)),(*((&height_0)) = (scene_depth_1).get_height(0));
    int2 extent_5 = int2(int(width_0), int(height_0));
    float _S22 = float(width_0);

#line 269
    float _S23 = float(height_0);

#line 269
    float2 size_4 = float2(_S22, _S23);
    int2 _S24 = int2(position_0.xy);

#line 270
    float _S25 = depth_at_0(_S24, extent_5, &kernelContext_6);


    if(_S25 <= 0.0f)
    {

#line 273
        pixelOutput_0 _S26 = { 1.0f };



        return _S26;
    }

#line 277
    float3 _S27 = view_position_0(_S24, _S25, size_4, &kernelContext_6);

#line 277
    float3 _S28 = normal_at_0(_S24, _S27, extent_5, size_4, &kernelContext_6);

#line 282
    float3 ray_0 = (&kernelContext_6)->camera_0->to_light_0.xyz;

    float facing_0 = saturate(dot(_S28, ray_0) / 0.10000000149011612f);
    if(facing_0 <= 0.0f)
    {

#line 285
        pixelOutput_0 _S29 = { 1.0f };

        return _S29;
    }

    float _S30 = _S27.z;

#line 290
    float3 start_0 = _S27 + _S28 * float3((abs(_S30) * 0.00499999988824129f)) ;
    float4 clip_start_0 = (((float4(start_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float4 clip_ray_0 = (((float4(ray_0, 0.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S31 = clip_start_0.w;

#line 293
    if(_S31 <= 0.0f)
    {

#line 293
        pixelOutput_0 _S32 = { 1.0f };

        return _S32;
    }

    float2 _S33 = clip_start_0.xy;

#line 298
    float2 _S34 = float2(_S31) ;

#line 298
    float2 at_start_0 = pixel_of_0(_S33 / _S34, size_4);

    float _S35 = clip_ray_0.w;

#line 300
    float2 ndc_rate_0 = (clip_ray_0.xy * _S34 - _S33 * float2(_S35) ) / float2((_S31 * _S31)) ;
    float2 screen_rate_0 = float2(ndc_rate_0.x * 0.5f * _S22, - ndc_rate_0.y * 0.5f * _S23);
    float rate_0 = length(screen_rate_0);
    if(rate_0 < 9.99999997475242708e-07f)
    {

#line 303
        pixelOutput_0 _S36 = { 1.0f };



        return _S36;
    }
    float2 forward_1 = screen_rate_0 / float2(rate_0) ;

#line 316
    float4 clip_end_0 = (((float4(start_0 + ray_0 * float3(0.25f) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->proj_0.data_0[int(3)][int(3)]))));
    float _S37 = clip_end_0.w;

#line 317
    float travel_0;

#line 317
    if(_S37 > 0.0f)
    {

#line 317
        travel_0 = min(15.0f, max(dot(pixel_of_0(clip_end_0.xy / float2(_S37) , size_4) - at_start_0, forward_1), 0.0f));

#line 317
    }
    else
    {

#line 317
        travel_0 = 15.0f;

#line 317
    }

#line 322
    float _S38 = forward_1.x;

#line 322
    if(_S38 > 0.0f)
    {

#line 322
        travel_0 = min(travel_0, (_S22 - 1.0f - at_start_0.x) / _S38);

#line 322
    }
    else
    {

        if(_S38 < 0.0f)
        {

#line 326
            travel_0 = min(travel_0, - at_start_0.x / _S38);

#line 326
        }

#line 322
    }

#line 330
    float _S39 = forward_1.y;

#line 330
    if(_S39 > 0.0f)
    {

#line 330
        travel_0 = min(travel_0, (_S23 - 1.0f - at_start_0.y) / _S39);

#line 330
    }
    else
    {

        if(_S39 < 0.0f)
        {

#line 334
            travel_0 = min(travel_0, - at_start_0.y / _S39);

#line 334
        }

#line 330
    }

#line 338
    if(_S35 < 0.0f)
    {


        float4 on_near_0 = (((float4(0.0f, 0.0f, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(0)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(1)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(2)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(0)][int(3)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(1)][int(3)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(2)][int(3)], (&kernelContext_6)->camera_0->inv_proj_0.data_0[int(3)][int(3)]))));


        float4 clip_near_0 = clip_start_0 + clip_ray_0 * float4(((- on_near_0.z / on_near_0.w - _S31) / _S35)) ;

#line 345
        travel_0 = min(travel_0, max(dot(pixel_of_0(clip_near_0.xy / float2(clip_near_0.w) , size_4) - at_start_0, forward_1), 0.0f));

#line 338
    }

#line 349
    float _S40 = max(travel_0, 0.0f);
    if(_S40 < 2.0f)
    {

#line 350
        pixelOutput_0 _S41 = { 1.0f };



        return _S41;
    }


    float2 ndc_end_0 = ndc_of_0(at_start_0 + forward_1 * float2(_S40) , size_4);

#line 358
    float when_end_0;

    if((abs(_S38)) >= (abs(_S39)))
    {

#line 360
        float _S42 = ndc_end_0.x;

#line 360
        when_end_0 = (_S42 * _S31 - clip_start_0.x) / (clip_ray_0.x - _S42 * _S35);

#line 360
    }
    else
    {

#line 361
        float _S43 = ndc_end_0.y;

#line 361
        when_end_0 = (_S43 * _S31 - clip_start_0.y) / (clip_ray_0.y - _S43 * _S35);

#line 360
    }

#line 360
    bool _S44;

    if(!(when_end_0 > 0.0f))
    {

#line 362
        _S44 = true;

#line 362
    }
    else
    {

#line 362
        _S44 = !isfinite(when_end_0);

#line 362
    }

#line 362
    if(_S44)
    {

#line 362
        pixelOutput_0 _S45 = { 1.0f };

        return _S45;
    }

    float inverse_w_start_0 = 1.0f / _S31;

    float inverse_w_end_0 = 1.0f / (_S31 + when_end_0 * _S35);
    float _S46 = start_0.z;

#line 370
    float _S47 = _S46 * inverse_w_start_0;
    float _S48 = (_S46 + when_end_0 * ray_0.z) * inverse_w_end_0;

#line 379
    float _S49 = _S46 - _S30;

#line 379
    float at_travel_0 = min(cell_exit_0(at_start_0, forward_1, 1.0f, _S40), _S40);

#line 379
    float previous_gap_0 = _S49;

#line 379
    float entry_z_0 = _S46;

#line 379
    uint step_0 = 0U;
    for(;;)
    {

#line 380
        if(step_0 < 16U)
        {
        }
        else
        {

#line 380
            break;
        }
        float2 at_2 = at_start_0 + forward_1 * float2(at_travel_0) ;
        float _S50 = min(at_travel_0 + cell_exit_0(at_2, forward_1, 1.0f, _S40), _S40);
        float2 exit_at_0 = at_start_0 + forward_1 * float2(_S50) ;
        float along_0 = _S50 / _S40;

        float exit_z_0 = mix(_S47, _S48, along_0) / mix(inverse_w_start_0, inverse_w_end_0, along_0);

#line 387
        float _S51 = depth_at_0(int2(floor(at_2)), extent_5, &kernelContext_6);

#line 387
        float gap_0;



        if(_S51 <= 0.0f)
        {

#line 391
            gap_0 = 1.0f;

#line 391
        }
        else
        {

#line 391
            float _S52 = view_z_of_0(_S51, &kernelContext_6);

#line 391
            gap_0 = exit_z_0 - _S52;

#line 391
        }
        if(gap_0 <= 0.0f)
        {

#line 392
            _S44 = previous_gap_0 > 0.0f;

#line 392
        }
        else
        {

#line 392
            _S44 = false;

#line 392
        }

#line 392
        if(_S44)
        {
            float behind_0 = - gap_0;
            float thickness_0 = thickness_at_0(abs(exit_z_0 - entry_z_0), exit_z_0);
            if(behind_0 <= thickness_0)
            {


                float2 hit_ndc_0 = ndc_of_0(mix(at_2, exit_at_0, float2((previous_gap_0 / max(previous_gap_0 - gap_0, 9.99999993922529029e-09f))) ), size_4);

#line 400
                pixelOutput_0 _S53 = { saturate(1.0f - facing_0 * saturate((1.0f - max(abs(hit_ndc_0.x), abs(hit_ndc_0.y))) / 0.15000000596046448f) * saturate((1.0f - _S50 / _S40) / 0.25f) * saturate(1.0f - behind_0 / thickness_0)) };



                return _S53;
            }

#line 392
        }

#line 410
        if(_S50 >= _S40)
        {
            break;
        }

#line 380
        uint step_1 = step_0 + 1U;

#line 380
        at_travel_0 = _S50;

#line 380
        previous_gap_0 = gap_0;

#line 380
        entry_z_0 = exit_z_0;

#line 380
        step_0 = step_1;

#line 380
    }

#line 380
    pixelOutput_0 _S54 = { 1.0f };

#line 415
    return _S54;
}


#line 415
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 167
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 167
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], ContactShadowParams_natural_0 constant* camera_2 [[buffer(0)]])
{

#line 167
    thread KernelContext_0 kernelContext_7;

#line 167
    (&kernelContext_7)->scene_depth_0 = scene_depth_2;

#line 167
    (&kernelContext_7)->camera_0 = camera_2;

#line 256
    thread FullscreenOutput_0 output_1;
    float2 _S55 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 257
    (&output_1)->uv_2 = _S55;
    (&output_1)->position_2 = float4(_S55 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 258
    thread vertexMain_Result_0 _S56;

#line 258
    (&_S56)->position_1 = output_1.position_2;

#line 258
    (&_S56)->uv_1 = output_1.uv_2;

#line 258
    return _S56;
}

