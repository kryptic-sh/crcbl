#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 99 "shaders/volumetric.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 94
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
    VolumetricParams_natural_0 constant* params_0;
    packed_float4 device* volumetrics_0;
};


#line 211 "shaders/volumetric.slang"
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 247
void volumetric_tile_ray_0(uint tile_x_0, uint tile_y_0, float3 thread* near_point_0, float thread* near_depth_0, KernelContext_0 thread* kernelContext_1)
{

    float2 pixel_0 = (float2(float(tile_x_0), float(tile_y_0)) + float2(0.5f) ) * float2(float(kernelContext_1->params_0->tile_pixels_0)) ;

#line 250
    float3 _S1 = volumetric_unproject_0(float2(pixel_0.x / float(max(kernelContext_1->params_0->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - pixel_0.y / float(max(kernelContext_1->params_0->viewport_y_0, 1U)) * 2.0f), 1.0f, kernelContext_1);



    *near_point_0 = _S1;
    *near_depth_0 = max(dot(kernelContext_1->params_0->depth_row_0, float4(_S1, 1.0f)), 9.99999997475242708e-07f);
    return;
}


#line 226
float volumetric_slice_start_0(uint index_0)
{

#line 226
    uint step_0 = 0U;

#line 226
    float start_0 = 0.10000000149011612f;


    for(;;)
    {

#line 229
        if(step_0 < index_0)
        {
        }
        else
        {

#line 229
            break;
        }
        float start_1 = start_0 * 1.46779930591583252f;

#line 229
        step_0 = step_0 + 1U;

#line 229
        start_0 = start_1;

#line 229
    }



    return start_0;
}


#line 160
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S2 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 167
    float kernel_0 = 0.0001984127011383f;

#line 167
    int term_0 = int(6);

    for(;;)
    {

#line 169
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 169
            break;
        }
        float _S3 = kernel_0 * _S2 + FOG_KERNEL_0[term_0];

#line 169
        int term_1 = term_0 - int(1);

#line 169
        kernel_0 = _S3;

#line 169
        term_0 = term_1;

#line 169
    }

#line 174
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S4 = - d_0;

#line 183
        float series_0 = 0.00833333376795053f;

#line 183
        int term_2 = int(3);

        for(;;)
        {

#line 185
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 185
                break;
            }
            float _S5 = series_0 * _S4 + FOG_RATIO_KERNEL_0[term_2];

#line 185
            int term_3 = term_2 - int(1);

#line 185
            series_0 = _S5;

#line 185
            term_2 = term_3;

#line 185
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

#line 207
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 269
float4 volumetric_slice_0(float3 from_0, float3 to_0, KernelContext_0 thread* kernelContext_2)
{
    float reference_0 = kernelContext_2->params_0->fog_params_0.z;


    float survives_0 = fog_exp_neg_0(fog_optical_depth_0(kernelContext_2->params_0->fog_params_0.x, kernelContext_2->params_0->fog_params_0.y, from_0.y - reference_0, to_0.y - reference_0, length(to_0 - from_0)));
    return float4(kernelContext_2->params_0->fog_color_0.xyz * float3((1.0f - survives_0)) , survives_0);
}


#line 285
[[kernel]] void scatterMain(uint3 thread_0 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], packed_float4 device* volumetrics_1 [[buffer(1)]])
{

#line 285
    thread KernelContext_0 kernelContext_3;

#line 285
    (&kernelContext_3)->params_0 = params_1;

#line 285
    (&kernelContext_3)->volumetrics_0 = volumetrics_1;

    uint froxel_0 = thread_0.x;
    uint tiles_0 = max(params_1->grid_x_0, 1U) * max(params_1->grid_y_0, 1U);
    uint _S6 = max(params_1->slices_0, 1U);

#line 289
    bool _S7;
    if(froxel_0 >= (tiles_0 * _S6))
    {

#line 290
        _S7 = true;

#line 290
    }
    else
    {

#line 290
        _S7 = froxel_0 >= ((&kernelContext_3)->params_0->froxel_count_0);

#line 290
    }

#line 290
    if(_S7)
    {
        return;
    }

    uint tile_x_1 = froxel_0 % max(params_1->grid_x_0, 1U);
    uint _S8 = froxel_0 / max(params_1->grid_x_0, 1U);

#line 296
    uint tile_y_1 = _S8 % max(params_1->grid_y_0, 1U);
    uint slice_0 = froxel_0 / tiles_0;

    thread float3 near_point_1;
    thread float near_depth_1;

#line 300
    volumetric_tile_ray_0(tile_x_1, tile_y_1, &near_point_1, &near_depth_1, &kernelContext_3);

    float3 along_0 = (near_point_1 - (&kernelContext_3)->params_0->eye_0.xyz) / float3(near_depth_1) ;

#line 302
    float from_depth_0;

#line 312
    if(slice_0 == 0U)
    {

#line 312
        from_depth_0 = 0.0f;

#line 312
    }
    else
    {

#line 312
        from_depth_0 = volumetric_slice_start_0(slice_0);

#line 312
    }
    uint _S9 = slice_0 + 1U;

#line 313
    float to_depth_0;

#line 313
    if(_S9 == _S6)
    {

#line 313
        to_depth_0 = 1000.0f;

#line 313
    }
    else
    {

#line 313
        to_depth_0 = volumetric_slice_start_0(_S9);

#line 313
    }

#line 313
    packed_float4 device* _S10 = (&kernelContext_3)->volumetrics_0+froxel_0;

#line 313
    float4 _S11 = volumetric_slice_0((&kernelContext_3)->params_0->eye_0.xyz + along_0 * float3(from_depth_0) , (&kernelContext_3)->params_0->eye_0.xyz + along_0 * float3(to_depth_0) , &kernelContext_3);

#line 313
    *_S10 = packed_float4(_S11) ;



    return;
}


#line 329
[[kernel]] void integrateMain(uint3 thread_1 [[thread_position_in_grid]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], packed_float4 device* volumetrics_2 [[buffer(1)]])
{

#line 329
    thread KernelContext_0 kernelContext_4;

#line 329
    (&kernelContext_4)->params_0 = params_2;

#line 329
    (&kernelContext_4)->volumetrics_0 = volumetrics_2;

    uint tile_0 = thread_1.x;
    uint tiles_1 = max(params_2->grid_x_0, 1U) * max(params_2->grid_y_0, 1U);
    if(tile_0 >= tiles_1)
    {
        return;
    }
    uint _S12 = max((&kernelContext_4)->params_0->slices_0, 1U);

    float3 _S13 = float3(0.0f, 0.0f, 0.0f);

#line 339
    uint slice_1 = 0U;

#line 339
    float3 accumulated_0 = _S13;

#line 339
    float through_0 = 1.0f;

    for(;;)
    {

#line 341
        if(slice_1 < _S12)
        {
        }
        else
        {

#line 341
            break;
        }
        uint froxel_1 = tile_0 + slice_1 * tiles_1;
        if(froxel_1 >= ((&kernelContext_4)->params_0->froxel_count_0))
        {
            break;
        }

#line 346
        float4 _S14 = float4(*((&kernelContext_4)->volumetrics_0+froxel_1)) ;

#line 346
        *((&kernelContext_4)->volumetrics_0+froxel_1) = packed_float4(float4(accumulated_0, through_0)) ;



        float3 accumulated_1 = accumulated_0 + float3(through_0)  * _S14.xyz;
        float through_1 = through_0 * _S14.w;

#line 341
        slice_1 = slice_1 + 1U;

#line 341
        accumulated_0 = accumulated_1;

#line 341
        through_0 = through_1;

#line 341
    }

#line 353
    return;
}

