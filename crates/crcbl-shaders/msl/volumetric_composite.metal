#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 82 "shaders/volumetric_composite.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 77
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 90 "core"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 90
struct VolumetricParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inverse_view_proj_0;
    float4 eye_0;
    float4 depth_row_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sun_direction_0;
    float4 sun_radiance_0;
    uint grid_x_0;
    uint grid_y_0;
    uint slices_0;
    uint tile_pixels_0;
    uint viewport_x_0;
    uint viewport_y_0;
    uint froxel_count_0;
    uint pad0_0;
};


#line 90
struct KernelContext_0
{
    texture2d<float, access::sample> scene_color_0;
    VolumetricParams_natural_0 constant* params_0;
    depth2d<float, access::sample> scene_depth_0;
    packed_float4 device* volumetrics_0;
};


#line 189 "shaders/volumetric_composite.slang"
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 138
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S1 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 145
    float kernel_0 = 0.0001984127011383f;

#line 145
    int term_0 = int(6);

    for(;;)
    {

#line 147
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 147
            break;
        }
        float _S2 = kernel_0 * _S1 + FOG_KERNEL_0[term_0];

#line 147
        int term_1 = term_0 - int(1);

#line 147
        kernel_0 = _S2;

#line 147
        term_0 = term_1;

#line 147
    }

#line 152
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S3 = - d_0;

#line 161
        float series_0 = 0.00833333376795053f;

#line 161
        int term_2 = int(3);

        for(;;)
        {

#line 163
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 163
                break;
            }
            float _S4 = series_0 * _S3 + FOG_RATIO_KERNEL_0[term_2];

#line 163
            int term_3 = term_2 - int(1);

#line 163
            series_0 = _S4;

#line 163
            term_2 = term_3;

#line 163
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}



float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_0)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_0, 0.0f, 32.0f);
    }

#line 185
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 221
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_0 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S5 = a_0 * a_0;

#line 224
    float d_1 = 1.0f + _S5 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S5) / (d_1 * sqrt(d_1));
}


#line 240
float3 volumetric_source_0(float3 view_direction_0, KernelContext_0 thread* kernelContext_1)
{



    return kernelContext_1->params_0->fog_color_0.xyz + kernelContext_1->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_1->params_0->sun_direction_0.w, dot(kernelContext_1->params_0->sun_direction_0.xyz, view_direction_0))) ;
}


#line 245
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 245
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 259
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S6 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], packed_float4 device* volumetrics_1 [[buffer(1)]])
{

#line 259
    thread KernelContext_0 kernelContext_2;

#line 259
    (&kernelContext_2)->scene_color_0 = scene_color_1;

#line 259
    (&kernelContext_2)->params_0 = params_1;

#line 259
    (&kernelContext_2)->scene_depth_0 = scene_depth_1;

#line 259
    (&kernelContext_2)->volumetrics_0 = volumetrics_1;

    int2 _S7 = int2(position_0.xy);
    int3 _S8 = int3(_S7, int(0));

#line 262
    float4 scene_0 = ((scene_color_1).read(vec<uint,2>(((_S8)).xy), uint(((_S8)).z)));

    uint _S9 = max(params_1->grid_x_0, 1U);
    uint _S10 = max(params_1->grid_y_0, 1U);
    uint _S11 = max(params_1->slices_0, 1U);
    uint tiles_0 = _S9 * _S10;
    uint _S12 = max(params_1->tile_pixels_0, 1U);



    int _S13 = _S7.x;
    int _S14 = _S7.y;

#line 271
    float2 ndc_1 = float2((float(_S13) + 0.5f) / float(max(params_1->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S14) + 0.5f) / float(max(params_1->viewport_y_0, 1U)) * 2.0f);

#line 279
    float _S15 = ((scene_depth_1).read(vec<uint,2>(((_S8)).xy), uint(((_S8)).z)));

#line 279
    float view_depth_0;

    if(_S15 > 0.0f)
    {

#line 281
        float3 _S16 = volumetric_unproject_0(ndc_1, _S15, &kernelContext_2);

#line 281
        view_depth_0 = dot((&kernelContext_2)->params_0->depth_row_0, float4(_S16, 1.0f));

#line 281
    }
    else
    {

#line 281
        view_depth_0 = 1000.0f;

#line 281
    }

#line 286
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 286
    float slice_start_0 = 0.0f;

#line 286
    uint slice_0 = 0U;

#line 286
    float next_start_0 = 0.14677993953227997f;

#line 305
    for(;;)
    {

#line 305
        uint _S17 = slice_0 + 1U;

#line 305
        bool _S18;

#line 305
        if(_S17 < _S11)
        {

#line 305
            _S18 = next_start_0 <= view_depth_1;

#line 305
        }
        else
        {

#line 305
            _S18 = false;

#line 305
        }

#line 305
        if(_S18)
        {
        }
        else
        {

#line 305
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 308
        slice_start_0 = next_start_0;

#line 308
        next_start_0 = next_start_1;

#line 308
        slice_0 = _S17;

#line 305
    }

#line 316
    uint _S19 = uint(max(_S13, int(0))) / _S12;

#line 316
    uint _S20 = min(_S19, _S9 - 1U);
    uint _S21 = uint(max(_S14, int(0))) / _S12;
    uint froxel_0 = _S20 + min(_S21, _S10 - 1U) * _S9 + slice_0 * tiles_0;
    if(froxel_0 >= ((&kernelContext_2)->params_0->froxel_count_0))
    {

#line 319
        pixelOutput_0 _S22 = { scene_0 };

        return _S22;
    }

#line 321
    float4 _S23 = float4(*((&kernelContext_2)->volumetrics_0+froxel_0)) ;

#line 321
    float3 _S24 = volumetric_unproject_0(ndc_1, 1.0f, &kernelContext_2);

#line 331
    float3 along_0 = (_S24 - (&kernelContext_2)->params_0->eye_0.xyz) / float3(max(dot((&kernelContext_2)->params_0->depth_row_0, float4(_S24, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = (&kernelContext_2)->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = (&kernelContext_2)->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;

    float reference_0 = (&kernelContext_2)->params_0->fog_params_0.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0((&kernelContext_2)->params_0->fog_params_0.x, (&kernelContext_2)->params_0->fog_params_0.y, from_0.y - reference_0, to_0.y - reference_0, length_of_0));

#line 340
    float3 view_direction_1;

#line 346
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 346
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 346
    }
    else
    {

#line 346
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 346
    }

#line 346
    float3 _S25 = volumetric_source_0(view_direction_1, &kernelContext_2);


    float _S26 = _S23.w;

#line 349
    pixelOutput_0 _S27 = { float4(scene_0.xyz * float3((_S26 * partial_survives_0))  + _S23.xyz + float3(_S26)  * (_S25 * float3((1.0f - partial_survives_0)) ), scene_0.w) };

    return _S27;
}


#line 351
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 130
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 130
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], packed_float4 device* volumetrics_2 [[buffer(1)]])
{

#line 130
    thread KernelContext_0 kernelContext_3;

#line 130
    (&kernelContext_3)->scene_color_0 = scene_color_2;

#line 130
    (&kernelContext_3)->params_0 = params_2;

#line 130
    (&kernelContext_3)->scene_depth_0 = scene_depth_2;

#line 130
    (&kernelContext_3)->volumetrics_0 = volumetrics_2;

#line 251
    thread FullscreenOutput_0 output_1;

    float2 _S28 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 253
    (&output_1)->uv_2 = _S28;
    (&output_1)->position_2 = float4(_S28 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 254
    thread vertexMain_Result_0 _S29;

#line 254
    (&_S29)->position_1 = output_1.position_2;

#line 254
    (&_S29)->uv_1 = output_1.uv_2;

#line 254
    return _S29;
}

