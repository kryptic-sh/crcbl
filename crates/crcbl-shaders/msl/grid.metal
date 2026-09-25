#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 151 "shaders/grid.slang"
float3 unproject_0(float4 homogeneous_0)
{
    float _S1 = homogeneous_0.w;

#line 153
    float w_0;

#line 153
    if((abs(_S1)) < 9.99999997475242708e-07f)
    {

#line 153
        w_0 = 9.99999997475242708e-07f;

#line 153
    }
    else
    {

#line 153
        w_0 = _S1;

#line 153
    }
    return homogeneous_0.xyz / float3(w_0) ;
}


#line 188
float line_coverage_0(float cell_0, float derivative_0, float width_0)
{



    return saturate(width_0 * 0.5f + 0.5f - abs(fract(cell_0 - 0.5f) - 0.5f) / max(derivative_0, 9.99999993922529029e-09f));
}


#line 166
float resolvable_0(float derivative_1, float width_1)
{
    return saturate(2.0f * (1.0f - width_1 * derivative_1));
}


#line 203
float scale_coverage_0(float2 plane_0, float spacing_0, float width_2)
{
    float2 cell_1 = plane_0 / float2(spacing_0) ;
    float2 derivative_2 = (fwidth((cell_1)));

    float _S2 = derivative_2.x;
    float _S3 = derivative_2.y;
    return max(line_coverage_0(cell_1.x, _S2, width_2), line_coverage_0(cell_1.y, _S3, width_2)) * resolvable_0(max(_S2, _S3), width_2);
}


#line 144
struct GridOutput_0
{
    float4 color_0 [[color(0)]];
    float depth_0 [[depth(any)]];
};


#line 144
struct pixelInput_0
{
    float2 uv_0 [[user(TEXCOORD)]];
};


#line 144
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 144
struct GridParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inv_view_proj_0;
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
    float4 params_0;
    float4 fine_color_0;
    float4 coarse_color_0;
};


#line 94
struct KernelContext_0
{
    GridParams_natural_0 constant* grid_0;
};


#line 225
[[fragment]] GridOutput_0 fragmentMain(pixelInput_0 _S4 [[stage_in]], float4 position_0 [[position]], GridParams_natural_0 constant* grid_1 [[buffer(0)]])
{

#line 225
    thread KernelContext_0 kernelContext_0;

#line 225
    (&kernelContext_0)->grid_0 = grid_1;

#line 232
    float2 ndc_0 = _S4.uv_0 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f);

    float3 origin_0 = unproject_0((((float4(ndc_0, 1.0f, 1.0f)) * (matrix<float,int(4),int(4)> (grid_1->inv_view_proj_0.data_0[int(0)][int(0)], grid_1->inv_view_proj_0.data_0[int(1)][int(0)], grid_1->inv_view_proj_0.data_0[int(2)][int(0)], grid_1->inv_view_proj_0.data_0[int(3)][int(0)], grid_1->inv_view_proj_0.data_0[int(0)][int(1)], grid_1->inv_view_proj_0.data_0[int(1)][int(1)], grid_1->inv_view_proj_0.data_0[int(2)][int(1)], grid_1->inv_view_proj_0.data_0[int(3)][int(1)], grid_1->inv_view_proj_0.data_0[int(0)][int(2)], grid_1->inv_view_proj_0.data_0[int(1)][int(2)], grid_1->inv_view_proj_0.data_0[int(2)][int(2)], grid_1->inv_view_proj_0.data_0[int(3)][int(2)], grid_1->inv_view_proj_0.data_0[int(0)][int(3)], grid_1->inv_view_proj_0.data_0[int(1)][int(3)], grid_1->inv_view_proj_0.data_0[int(2)][int(3)], grid_1->inv_view_proj_0.data_0[int(3)][int(3)])))));

    float3 direction_0 = normalize(unproject_0((((float4(ndc_0, 0.5f, 1.0f)) * (matrix<float,int(4),int(4)> (grid_1->inv_view_proj_0.data_0[int(0)][int(0)], grid_1->inv_view_proj_0.data_0[int(1)][int(0)], grid_1->inv_view_proj_0.data_0[int(2)][int(0)], grid_1->inv_view_proj_0.data_0[int(3)][int(0)], grid_1->inv_view_proj_0.data_0[int(0)][int(1)], grid_1->inv_view_proj_0.data_0[int(1)][int(1)], grid_1->inv_view_proj_0.data_0[int(2)][int(1)], grid_1->inv_view_proj_0.data_0[int(3)][int(1)], grid_1->inv_view_proj_0.data_0[int(0)][int(2)], grid_1->inv_view_proj_0.data_0[int(1)][int(2)], grid_1->inv_view_proj_0.data_0[int(2)][int(2)], grid_1->inv_view_proj_0.data_0[int(3)][int(2)], grid_1->inv_view_proj_0.data_0[int(0)][int(3)], grid_1->inv_view_proj_0.data_0[int(1)][int(3)], grid_1->inv_view_proj_0.data_0[int(2)][int(3)], grid_1->inv_view_proj_0.data_0[int(3)][int(3)]))))) - origin_0);

#line 241
    float _S5 = direction_0.y;

#line 241
    bool crosses_0 = (abs(_S5)) > 1.00000001168609742e-07f;

#line 241
    float denominator_0;
    if(crosses_0)
    {

#line 242
        denominator_0 = _S5;

#line 242
    }
    else
    {

#line 242
        denominator_0 = 1.00000001168609742e-07f;

#line 242
    }
    float t_0 = - origin_0.y / denominator_0;

    float fade_distance_0 = (&kernelContext_0)->grid_0->params_0.w;

#line 245
    bool visible_0;

#line 251
    if(crosses_0)
    {

#line 251
        visible_0 = t_0 > 0.0f;

#line 251
    }
    else
    {

#line 251
        visible_0 = false;

#line 251
    }

#line 251
    if(visible_0)
    {

#line 251
        visible_0 = t_0 < fade_distance_0;

#line 251
    }
    else
    {

#line 251
        visible_0 = false;

#line 251
    }
    float3 hit_0 = origin_0 + direction_0 * float3(clamp(t_0, 0.0f, fade_distance_0)) ;

    float spacing_1 = (&kernelContext_0)->grid_0->params_0.x;

    float width_3 = (&kernelContext_0)->grid_0->params_0.z;
    float2 _S6 = hit_0.xz;

    float fade_0 = saturate(1.0f - t_0 / fade_distance_0);

#line 264
    float coarse_alpha_0 = (&kernelContext_0)->grid_0->coarse_color_0.w * scale_coverage_0(_S6, spacing_1 * (&kernelContext_0)->grid_0->params_0.y, width_3) * fade_0;
    float fine_alpha_0 = (&kernelContext_0)->grid_0->fine_color_0.w * scale_coverage_0(_S6, spacing_1, width_3) * fade_0 * (1.0f - coarse_alpha_0);
    float alpha_0 = coarse_alpha_0 + fine_alpha_0;
    float3 color_1 = (&kernelContext_0)->grid_0->coarse_color_0.xyz * float3(coarse_alpha_0)  + (&kernelContext_0)->grid_0->fine_color_0.xyz * float3(fine_alpha_0) ;

    float4 clip_0 = (((float4(hit_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_0)->grid_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_0)->grid_0->view_proj_0.data_0[int(3)][int(3)]))));


    if(visible_0)
    {

#line 272
        visible_0 = (clip_0.w) > 0.0f;

#line 272
    }
    else
    {

#line 272
        visible_0 = false;

#line 272
    }

#line 272
    if(visible_0)
    {

#line 272
        visible_0 = alpha_0 > 0.0f;

#line 272
    }
    else
    {

#line 272
        visible_0 = false;

#line 272
    }
    if(!visible_0)
    {
        discard_fragment();

#line 273
    }

#line 278
    thread GridOutput_0 output_0;
    (&output_0)->color_0 = float4(color_1, alpha_0);


    (&output_0)->depth_0 = saturate(clip_0.z / clip_0.w);
    return output_0;
}


#line 283
struct vertexMain_Result_0
{
    float4 position_1 [[position]];
    float2 uv_1 [[user(TEXCOORD)]];
};


#line 132
struct FullscreenOutput_0
{
    float4 position_2;
    float2 uv_2;
};


#line 132
[[vertex]] vertexMain_Result_0 vertexMain(uint index_0 [[vertex_id]], GridParams_natural_0 constant* grid_2 [[buffer(0)]])
{

#line 216
    thread FullscreenOutput_0 output_1;


    float2 _S7 = float2(float((index_0 << 1U) & 2U), float(index_0 & 2U));

#line 219
    (&output_1)->uv_2 = _S7;
    (&output_1)->position_2 = float4(_S7 * float2(2.0f, -2.0f) + float2(-1.0f, 1.0f), 0.0f, 1.0f);

#line 220
    thread vertexMain_Result_0 _S8;

#line 220
    (&_S8)->position_1 = output_1.position_2;

#line 220
    (&_S8)->uv_1 = output_1.uv_2;

#line 220
    return _S8;
}

