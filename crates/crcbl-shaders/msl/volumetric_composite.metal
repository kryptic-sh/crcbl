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
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 90
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 106 "shaders/volumetric_composite.slang"
struct VolumetricParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inverse_view_proj_0;
    float4 eye_0;
    float4 depth_row_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sun_direction_0;
    float4 sun_radiance_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint grid_x_0;
    uint grid_y_0;
    uint slices_0;
    uint tile_pixels_0;
    uint viewport_x_0;
    uint viewport_y_0;
    uint froxel_count_0;
    uint pad0_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_0;
    array<float4, int(16)> shadow_atlas_rect_0;
};


#line 106
struct KernelContext_0
{
    texture2d<float, access::sample> scene_color_0;
    VolumetricParams_natural_0 constant* params_0;
    depth2d<float, access::sample> scene_depth_0;
    packed_float4 device* volumetrics_0;
    packed_float4 device* lighting_0;
};


#line 241
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 190
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S1 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 197
    float kernel_0 = 0.0001984127011383f;

#line 197
    int term_0 = int(6);

    for(;;)
    {

#line 199
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 199
            break;
        }
        float _S2 = kernel_0 * _S1 + FOG_KERNEL_0[term_0];

#line 199
        int term_1 = term_0 - int(1);

#line 199
        kernel_0 = _S2;

#line 199
        term_0 = term_1;

#line 199
    }

#line 204
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S3 = - d_0;

#line 213
        float series_0 = 0.00833333376795053f;

#line 213
        int term_2 = int(3);

        for(;;)
        {

#line 215
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 215
                break;
            }
            float _S4 = series_0 * _S3 + FOG_RATIO_KERNEL_0[term_2];

#line 215
            int term_3 = term_2 - int(1);

#line 215
            series_0 = _S4;

#line 215
            term_2 = term_3;

#line 215
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

#line 237
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 273
float volumetric_phase_0(float g_0, float cos_theta_0)
{
    float a_0 = clamp(g_0, -0.99000000953674316f, 0.99000000953674316f);
    float _S5 = a_0 * a_0;

#line 276
    float d_1 = 1.0f + _S5 - 2.0f * a_0 * clamp(cos_theta_0, -1.0f, 1.0f);
    return 0.07957746833562851f * (1.0f - _S5) / (d_1 * sqrt(d_1));
}


#line 295
float3 volumetric_source_0(float3 view_direction_0, float4 lit_0, KernelContext_0 thread* kernelContext_1)
{



    return kernelContext_1->params_0->fog_color_0.xyz + kernelContext_1->params_0->sun_radiance_0.xyz * float3(volumetric_phase_0(kernelContext_1->params_0->sun_direction_0.w, dot(kernelContext_1->params_0->sun_direction_0.xyz, view_direction_0)))  * float3(lit_0.w)  + lit_0.xyz;
}


#line 300
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 300
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 314
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S6 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], packed_float4 device* volumetrics_1 [[buffer(1)]], packed_float4 device* lighting_1 [[buffer(2)]])
{

#line 314
    thread KernelContext_0 kernelContext_2;

#line 314
    (&kernelContext_2)->scene_color_0 = scene_color_1;

#line 314
    (&kernelContext_2)->params_0 = params_1;

#line 314
    (&kernelContext_2)->scene_depth_0 = scene_depth_1;

#line 314
    (&kernelContext_2)->volumetrics_0 = volumetrics_1;

#line 314
    (&kernelContext_2)->lighting_0 = lighting_1;

    int2 _S7 = int2(position_0.xy);
    int3 _S8 = int3(_S7, int(0));

#line 317
    float4 scene_0 = ((scene_color_1).read(vec<uint,2>(((_S8)).xy), uint(((_S8)).z)));

    uint _S9 = max(params_1->grid_x_0, 1U);
    uint _S10 = max(params_1->grid_y_0, 1U);
    uint _S11 = max(params_1->slices_0, 1U);
    uint tiles_0 = _S9 * _S10;
    uint _S12 = max(params_1->tile_pixels_0, 1U);



    int _S13 = _S7.x;
    int _S14 = _S7.y;

#line 326
    float2 ndc_1 = float2((float(_S13) + 0.5f) / float(max(params_1->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S14) + 0.5f) / float(max(params_1->viewport_y_0, 1U)) * 2.0f);

#line 334
    float _S15 = ((scene_depth_1).read(vec<uint,2>(((_S8)).xy), uint(((_S8)).z)));

#line 334
    float view_depth_0;

    if(_S15 > 0.0f)
    {

#line 336
        float3 _S16 = volumetric_unproject_0(ndc_1, _S15, &kernelContext_2);

#line 336
        view_depth_0 = dot((&kernelContext_2)->params_0->depth_row_0, float4(_S16, 1.0f));

#line 336
    }
    else
    {

#line 336
        view_depth_0 = 1000.0f;

#line 336
    }

#line 341
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 341
    float slice_start_0 = 0.0f;

#line 341
    uint slice_0 = 0U;

#line 341
    float next_start_0 = 0.14677993953227997f;

#line 360
    for(;;)
    {

#line 360
        uint _S17 = slice_0 + 1U;

#line 360
        bool _S18;

#line 360
        if(_S17 < _S11)
        {

#line 360
            _S18 = next_start_0 <= view_depth_1;

#line 360
        }
        else
        {

#line 360
            _S18 = false;

#line 360
        }

#line 360
        if(_S18)
        {
        }
        else
        {

#line 360
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 363
        slice_start_0 = next_start_0;

#line 363
        next_start_0 = next_start_1;

#line 363
        slice_0 = _S17;

#line 360
    }

#line 371
    uint _S19 = uint(max(_S13, int(0))) / _S12;

#line 371
    uint _S20 = min(_S19, _S9 - 1U);
    uint _S21 = uint(max(_S14, int(0))) / _S12;
    uint froxel_0 = _S20 + min(_S21, _S10 - 1U) * _S9 + slice_0 * tiles_0;
    if(froxel_0 >= ((&kernelContext_2)->params_0->froxel_count_0))
    {

#line 374
        pixelOutput_0 _S22 = { scene_0 };

        return _S22;
    }

#line 376
    float4 _S23 = float4(*((&kernelContext_2)->volumetrics_0+froxel_0)) ;

#line 376
    float3 _S24 = volumetric_unproject_0(ndc_1, 1.0f, &kernelContext_2);

#line 386
    float3 along_0 = (_S24 - (&kernelContext_2)->params_0->eye_0.xyz) / float3(max(dot((&kernelContext_2)->params_0->depth_row_0, float4(_S24, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = (&kernelContext_2)->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = (&kernelContext_2)->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;

    float reference_0 = (&kernelContext_2)->params_0->fog_params_0.z;
    float3 segment_0 = to_0 - from_0;
    float length_of_0 = length(segment_0);


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0((&kernelContext_2)->params_0->fog_params_0.x, (&kernelContext_2)->params_0->fog_params_0.y, from_0.y - reference_0, to_0.y - reference_0, length_of_0));

#line 395
    float3 view_direction_1;

#line 401
    if(length_of_0 > 9.99999997475242708e-07f)
    {

#line 401
        view_direction_1 = segment_0 / float3(length_of_0) ;

#line 401
    }
    else
    {

#line 401
        view_direction_1 = float3(0.0f, 0.0f, 1.0f);

#line 401
    }

#line 401
    float3 _S25 = volumetric_source_0(view_direction_1, float4(*((&kernelContext_2)->lighting_0+froxel_0)) , &kernelContext_2);

#line 410
    float _S26 = _S23.w;

#line 410
    pixelOutput_0 _S27 = { float4(scene_0.xyz * float3((_S26 * partial_survives_0))  + _S23.xyz + float3(_S26)  * (_S25 * float3((1.0f - partial_survives_0)) ), scene_0.w) };

    return _S27;
}


#line 412
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 182
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 182
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], packed_float4 device* volumetrics_2 [[buffer(1)]], packed_float4 device* lighting_2 [[buffer(2)]])
{

#line 182
    thread KernelContext_0 kernelContext_3;

#line 182
    (&kernelContext_3)->scene_color_0 = scene_color_2;

#line 182
    (&kernelContext_3)->params_0 = params_2;

#line 182
    (&kernelContext_3)->scene_depth_0 = scene_depth_2;

#line 182
    (&kernelContext_3)->volumetrics_0 = volumetrics_2;

#line 182
    (&kernelContext_3)->lighting_0 = lighting_2;

#line 306
    thread FullscreenOutput_0 output_1;

    float2 _S28 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 308
    (&output_1)->uv_2 = _S28;
    (&output_1)->position_2 = float4(_S28 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 309
    thread vertexMain_Result_0 _S29;

#line 309
    (&_S29)->position_1 = output_1.position_2;

#line 309
    (&_S29)->uv_1 = output_1.uv_2;

#line 309
    return _S29;
}

