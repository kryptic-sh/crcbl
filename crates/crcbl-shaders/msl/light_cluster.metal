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
struct LightClusterParams_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 inverse_view_proj_0;
    float4 eye_0;
    float4 depth_row_0;
    uint grid_x_0;
    uint grid_y_0;
    uint slices_0;
    uint light_count_0;
    uint viewport_x_0;
    uint viewport_y_0;
    uint perspective_0;
    uint tile_pixels_0;
};


#line 90
struct GpuLight_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 direction_0;
    uint kind_0;
    float cos_inner_0;
    uint pad0_0;
    uint pad1_0;
};


#line 374 "shaders/light_cluster.slang"
struct KernelContext_0
{
    LightClusterParams_natural_0 constant* params_0;
    GpuLight_natural_0 device* lights_0;
    uint device* cluster_lights_0;
    atomic<uint> device* cull_stats_0;
};


#line 196
float3 unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 190
float view_depth_0(float3 point_0, KernelContext_0 thread* kernelContext_1)
{
    return dot(kernelContext_1->params_0->depth_row_0, float4(point_0, 1.0f));
}


#line 207
float slice_start_0(uint index_0)
{
    return 0.10000000149011612f * pow(10000.0f, float(index_0) / 24.0f);
}



[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], LightClusterParams_natural_0 constant* params_1 [[buffer(0)]], GpuLight_natural_0 device* lights_1 [[buffer(1)]], uint device* cluster_lights_1 [[buffer(2)]], atomic<uint> device* cull_stats_1 [[buffer(3)]])
{

#line 214
    thread KernelContext_0 kernelContext_2;

#line 214
    (&kernelContext_2)->params_0 = params_1;

#line 214
    (&kernelContext_2)->lights_0 = lights_1;

#line 214
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 214
    (&kernelContext_2)->cull_stats_0 = cull_stats_1;

    uint froxel_0 = thread_0.x;
    uint tiles_0 = params_1->grid_x_0 * params_1->grid_y_0;
    if(froxel_0 >= (tiles_0 * params_1->slices_0))
    {
        return;
    }

    uint tile_x_0 = froxel_0 % params_1->grid_x_0;
    uint _S1 = froxel_0 / params_1->grid_x_0;

#line 224
    uint tile_y_0 = _S1 % params_1->grid_y_0;
    uint slice_0 = froxel_0 / tiles_0;

#line 231
    float _S2 = float((&kernelContext_2)->params_0->viewport_x_0);

#line 231
    float _S3 = float((&kernelContext_2)->params_0->viewport_y_0);
    float2 pixel_min_0 = float2(float(tile_x_0), float(tile_y_0)) * float2(float((&kernelContext_2)->params_0->tile_pixels_0)) ;
    float2 pixel_max_0 = pixel_min_0 + float2(float((&kernelContext_2)->params_0->tile_pixels_0)) ;



    float _S4 = pixel_min_0.x / _S2 * 2.0f - 1.0f;

#line 237
    float _S5 = 1.0f - pixel_max_0.y / _S3 * 2.0f;
    float _S6 = pixel_max_0.x / _S2 * 2.0f - 1.0f;

#line 238
    float _S7 = 1.0f - pixel_min_0.y / _S3 * 2.0f;

#line 244
    thread array<float3, int(4)> corner_near_0;

#line 244
    float3 _S8 = unproject_0(float2(_S4, _S5), 1.0f, &kernelContext_2);
    corner_near_0[int(0)] = _S8;

#line 245
    float3 _S9 = unproject_0(float2(_S6, _S5), 1.0f, &kernelContext_2);
    corner_near_0[int(1)] = _S9;

#line 246
    float3 _S10 = unproject_0(float2(_S4, _S7), 1.0f, &kernelContext_2);
    corner_near_0[int(2)] = _S10;

#line 247
    float3 _S11 = unproject_0(float2(_S6, _S7), 1.0f, &kernelContext_2);
    corner_near_0[int(3)] = _S11;

#line 256
    bool perspective_1 = ((&kernelContext_2)->params_0->perspective_0) != 0U;
    float3 _S12 = float3(0.0f, 0.0f, 0.0f);

#line 257
    float3 ortho_lo_0;

#line 257
    float3 ortho_hi_0;

#line 257
    uint index_1;

#line 257
    bool touches_0;

#line 257
    float eye_to_near_0;

#line 257
    float _S13;


    if(perspective_1)
    {

#line 260
        float _S14 = view_depth_0(corner_near_0[int(0)], &kernelContext_2);

#line 260
        eye_to_near_0 = _S14;

#line 260
        ortho_lo_0 = _S12;

#line 260
        ortho_hi_0 = _S12;

#line 260
    }
    else
    {

#line 260
        ortho_lo_0 = corner_near_0[int(0)];

#line 260
        ortho_hi_0 = corner_near_0[int(0)];

#line 260
        index_1 = 0U;

#line 269
        for(;;)
        {

#line 269
            if(index_1 < 4U)
            {
            }
            else
            {

#line 269
                break;
            }

            if(index_1 == 1U)
            {

#line 272
                touches_0 = true;

#line 272
            }
            else
            {

#line 272
                touches_0 = index_1 == 3U;

#line 272
            }

#line 272
            if(touches_0)
            {

#line 272
                eye_to_near_0 = _S6;

#line 272
            }
            else
            {

#line 272
                eye_to_near_0 = _S4;

#line 272
            }
            if(index_1 < 2U)
            {

#line 273
                _S13 = _S5;

#line 273
            }
            else
            {

#line 273
                _S13 = _S7;

#line 273
            }

#line 273
            float3 _S15 = unproject_0(float2(eye_to_near_0, _S13), 0.0f, &kernelContext_2);

            float3 _S16 = min(ortho_lo_0, min(corner_near_0[index_1], _S15));
            float3 _S17 = max(ortho_hi_0, max(corner_near_0[index_1], _S15));

#line 269
            uint corner_0 = index_1 + 1U;

#line 269
            ortho_lo_0 = _S16;

#line 269
            ortho_hi_0 = _S17;

#line 269
            index_1 = corner_0;

#line 269
        }

#line 269
        eye_to_near_0 = 1.0f;

#line 260
    }

#line 282
    if(perspective_1)
    {

#line 282
        _S13 = slice_start_0(slice_0);

#line 282
    }
    else
    {

#line 282
        _S13 = 0.0f;

#line 282
    }
    if(!perspective_1)
    {

#line 283
        touches_0 = true;

#line 283
    }
    else
    {

#line 283
        touches_0 = (slice_0 + 1U) >= (params_1->slices_0);

#line 283
    }

#line 283
    float _S18;
    if(touches_0)
    {

#line 284
        _S18 = 3.4028234663852886e+38f;

#line 284
    }
    else
    {

#line 284
        _S18 = slice_start_0(slice_0 + 1U);

#line 284
    }


    uint base_0 = froxel_0 * 17U;

#line 287
    index_1 = 0U;

#line 287
    uint kept_0 = 0U;

#line 287
    uint dropped_0 = 0U;


    for(;;)
    {

#line 290
        if(index_1 < ((&kernelContext_2)->params_0->light_count_0))
        {
        }
        else
        {

#line 290
            break;
        }
        GpuLight_natural_0 light_0 = (&kernelContext_2)->lights_0[index_1];

#line 292
        uint kept_1;

        if((light_0.kind_0) == 0U)
        {

#line 294
            touches_0 = true;

#line 294
        }
        else
        {

#line 294
            float4 _S19 = float4(light_0.position_0) ;

#line 303
            float3 center_0 = _S19.xyz;
            float radius_0 = _S19.w;

#line 304
            float3 box_lo_0;

#line 304
            float3 box_hi_0;


            if(perspective_1)
            {

#line 307
                float _S20 = view_depth_0(center_0, &kernelContext_2);

#line 314
                float light_lo_0 = _S20 - radius_0;
                float light_hi_0 = _S20 + radius_0;
                if(light_hi_0 < _S13)
                {

#line 316
                    touches_0 = true;

#line 316
                }
                else
                {

#line 316
                    touches_0 = light_lo_0 > _S18;

#line 316
                }

#line 316
                if(touches_0)
                {
                    index_1 = index_1 + 1U;

#line 290
                    continue;
                }

#line 324
                float _S21 = max(max(_S13, light_lo_0), _S13);
                float _S22 = max(min(_S18, light_hi_0), _S21);


                float3 first_0 = (&kernelContext_2)->params_0->eye_0.xyz + (corner_near_0[int(0)] - (&kernelContext_2)->params_0->eye_0.xyz) * float3((_S21 / eye_to_near_0)) ;

#line 328
                box_lo_0 = first_0;

#line 328
                box_hi_0 = first_0;

#line 328
                kept_1 = 0U;


                for(;;)
                {

#line 331
                    if(kept_1 < 4U)
                    {
                    }
                    else
                    {

#line 331
                        break;
                    }
                    float3 ray_0 = corner_near_0[kept_1] - (&kernelContext_2)->params_0->eye_0.xyz;
                    float3 at_lo_0 = (&kernelContext_2)->params_0->eye_0.xyz + ray_0 * float3((_S21 / eye_to_near_0)) ;
                    float3 at_hi_0 = (&kernelContext_2)->params_0->eye_0.xyz + ray_0 * float3((_S22 / eye_to_near_0)) ;
                    float3 _S23 = min(box_lo_0, min(at_lo_0, at_hi_0));
                    float3 _S24 = max(box_hi_0, max(at_lo_0, at_hi_0));

#line 331
                    uint corner_1 = kept_1 + 1U;

#line 331
                    box_lo_0 = _S23;

#line 331
                    box_hi_0 = _S24;

#line 331
                    kept_1 = corner_1;

#line 331
                }

#line 307
            }
            else
            {

#line 307
                box_lo_0 = ortho_lo_0;

#line 307
                box_hi_0 = ortho_hi_0;

#line 307
            }

#line 349
            float3 offset_0 = center_0 - clamp(center_0, box_lo_0, box_hi_0);

#line 349
            touches_0 = (dot(offset_0, offset_0)) <= (radius_0 * radius_0);

#line 294
        }

#line 353
        if(!touches_0)
        {
            index_1 = index_1 + 1U;

#line 290
            continue;
        }

#line 290
        uint dropped_1;

#line 357
        if(kept_0 < 16U)
        {
            *((&kernelContext_2)->cluster_lights_0+(base_0 + 1U + kept_0)) = index_1;

#line 359
            kept_1 = kept_0 + 1U;

#line 359
            dropped_1 = dropped_0;

#line 357
        }
        else
        {

#line 367
            uint dropped_2 = dropped_0 + 1U;

#line 367
            kept_1 = kept_0;

#line 367
            dropped_1 = dropped_2;

#line 357
        }

#line 357
        kept_0 = kept_1;

#line 357
        dropped_0 = dropped_1;

#line 290
        index_1 = index_1 + 1U;

#line 290
    }

#line 371
    *((&kernelContext_2)->cluster_lights_0+base_0) = kept_0;
    if(dropped_0 > 0U)
    {
        uint _S25 = atomic_fetch_add_explicit((&kernelContext_2)->cull_stats_0+2U, dropped_0, memory_order_relaxed);

#line 372
    }



    return;
}

