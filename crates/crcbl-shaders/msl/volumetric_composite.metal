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


#line 185 "shaders/volumetric_composite.slang"
float3 volumetric_unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 134
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);

    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S1 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 141
    float kernel_0 = 0.0001984127011383f;

#line 141
    int term_0 = int(6);

    for(;;)
    {

#line 143
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 143
            break;
        }
        float _S2 = kernel_0 * _S1 + FOG_KERNEL_0[term_0];

#line 143
        int term_1 = term_0 - int(1);

#line 143
        kernel_0 = _S2;

#line 143
        term_0 = term_1;

#line 143
    }

#line 148
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}



float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S3 = - d_0;

#line 157
        float series_0 = 0.00833333376795053f;

#line 157
        int term_2 = int(3);

        for(;;)
        {

#line 159
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 159
                break;
            }
            float _S4 = series_0 * _S3 + FOG_RATIO_KERNEL_0[term_2];

#line 159
            int term_3 = term_2 - int(1);

#line 159
            series_0 = _S4;

#line 159
            term_2 = term_3;

#line 159
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

#line 181
    return clamp(density_0 * distance_0 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 181
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 181
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 202
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S5 [[stage_in]], float4 position_0 [[position]], texture2d<float, access::sample> scene_color_1 [[texture(1)]], VolumetricParams_natural_0 constant* params_1 [[buffer(0)]], depth2d<float, access::sample> scene_depth_1 [[texture(0)]], packed_float4 device* volumetrics_1 [[buffer(1)]])
{

#line 202
    thread KernelContext_0 kernelContext_1;

#line 202
    (&kernelContext_1)->scene_color_0 = scene_color_1;

#line 202
    (&kernelContext_1)->params_0 = params_1;

#line 202
    (&kernelContext_1)->scene_depth_0 = scene_depth_1;

#line 202
    (&kernelContext_1)->volumetrics_0 = volumetrics_1;

    int2 _S6 = int2(position_0.xy);
    int3 _S7 = int3(_S6, int(0));

#line 205
    float4 scene_0 = ((scene_color_1).read(vec<uint,2>(((_S7)).xy), uint(((_S7)).z)));

    uint _S8 = max(params_1->grid_x_0, 1U);
    uint _S9 = max(params_1->grid_y_0, 1U);
    uint _S10 = max(params_1->slices_0, 1U);
    uint tiles_0 = _S8 * _S9;
    uint _S11 = max(params_1->tile_pixels_0, 1U);



    int _S12 = _S6.x;
    int _S13 = _S6.y;

#line 214
    float2 ndc_1 = float2((float(_S12) + 0.5f) / float(max(params_1->viewport_x_0, 1U)) * 2.0f - 1.0f, 1.0f - (float(_S13) + 0.5f) / float(max(params_1->viewport_y_0, 1U)) * 2.0f);

#line 222
    float _S14 = ((scene_depth_1).read(vec<uint,2>(((_S7)).xy), uint(((_S7)).z)));

#line 222
    float view_depth_0;

    if(_S14 > 0.0f)
    {

#line 224
        float3 _S15 = volumetric_unproject_0(ndc_1, _S14, &kernelContext_1);

#line 224
        view_depth_0 = dot((&kernelContext_1)->params_0->depth_row_0, float4(_S15, 1.0f));

#line 224
    }
    else
    {

#line 224
        view_depth_0 = 1000.0f;

#line 224
    }

#line 229
    float view_depth_1 = clamp(view_depth_0, 0.0f, 1000.0f);

#line 229
    float slice_start_0 = 0.0f;

#line 229
    uint slice_0 = 0U;

#line 229
    float next_start_0 = 0.14677993953227997f;

#line 248
    for(;;)
    {

#line 248
        uint _S16 = slice_0 + 1U;

#line 248
        bool _S17;

#line 248
        if(_S16 < _S10)
        {

#line 248
            _S17 = next_start_0 <= view_depth_1;

#line 248
        }
        else
        {

#line 248
            _S17 = false;

#line 248
        }

#line 248
        if(_S17)
        {
        }
        else
        {

#line 248
            break;
        }

        float next_start_1 = next_start_0 * 1.46779930591583252f;

#line 251
        slice_start_0 = next_start_0;

#line 251
        next_start_0 = next_start_1;

#line 251
        slice_0 = _S16;

#line 248
    }

#line 259
    uint _S18 = uint(max(_S12, int(0))) / _S11;

#line 259
    uint _S19 = min(_S18, _S8 - 1U);
    uint _S20 = uint(max(_S13, int(0))) / _S11;
    uint froxel_0 = _S19 + min(_S20, _S9 - 1U) * _S8 + slice_0 * tiles_0;
    if(froxel_0 >= ((&kernelContext_1)->params_0->froxel_count_0))
    {

#line 262
        pixelOutput_0 _S21 = { scene_0 };

        return _S21;
    }

#line 264
    float4 _S22 = float4(*((&kernelContext_1)->volumetrics_0+froxel_0)) ;

#line 264
    float3 _S23 = volumetric_unproject_0(ndc_1, 1.0f, &kernelContext_1);

#line 274
    float3 along_0 = (_S23 - (&kernelContext_1)->params_0->eye_0.xyz) / float3(max(dot((&kernelContext_1)->params_0->depth_row_0, float4(_S23, 1.0f)), 9.99999997475242708e-07f)) ;
    float3 from_0 = (&kernelContext_1)->params_0->eye_0.xyz + along_0 * float3(slice_start_0) ;
    float3 to_0 = (&kernelContext_1)->params_0->eye_0.xyz + along_0 * float3(max(view_depth_1, slice_start_0)) ;

    float reference_0 = (&kernelContext_1)->params_0->fog_params_0.z;


    float partial_survives_0 = fog_exp_neg_0(fog_optical_depth_0((&kernelContext_1)->params_0->fog_params_0.x, (&kernelContext_1)->params_0->fog_params_0.y, from_0.y - reference_0, to_0.y - reference_0, length(to_0 - from_0)));


    float _S24 = _S22.w;

#line 284
    pixelOutput_0 _S25 = { float4(scene_0.xyz * float3((_S24 * partial_survives_0))  + _S22.xyz + float3(_S24)  * ((&kernelContext_1)->params_0->fog_color_0.xyz * float3((1.0f - partial_survives_0)) ), scene_0.w) };

    return _S25;
}


#line 286
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 126
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 473 "core"
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], texture2d<float, access::sample> scene_color_2 [[texture(1)]], VolumetricParams_natural_0 constant* params_2 [[buffer(0)]], depth2d<float, access::sample> scene_depth_2 [[texture(0)]], packed_float4 device* volumetrics_2 [[buffer(1)]])
{

#line 473
    thread KernelContext_0 kernelContext_2;

#line 473
    (&kernelContext_2)->scene_color_0 = scene_color_2;

#line 473
    (&kernelContext_2)->params_0 = params_2;

#line 473
    (&kernelContext_2)->scene_depth_0 = scene_depth_2;

#line 473
    (&kernelContext_2)->volumetrics_0 = volumetrics_2;

#line 194 "shaders/volumetric_composite.slang"
    thread FullscreenOutput_0 output_1;

    float2 _S26 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 196
    (&output_1)->uv_2 = _S26;
    (&output_1)->position_2 = float4(_S26 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 197
    thread vertexMain_Result_0 _S27;

#line 197
    (&_S27)->position_1 = output_1.position_2;

#line 197
    (&_S27)->uv_1 = output_1.uv_2;

#line 197
    return _S27;
}

