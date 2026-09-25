#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 205 "shaders/light_cluster.slang"
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 205
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


#line 208
struct GpuLight_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 direction_0;
    packed_float4 tangent_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint flags_0;
};


#line 465
struct KernelContext_0
{
    LightClusterParams_natural_0 constant* params_0;
    GpuLight_natural_0 device* lights_0;
    uint device* cluster_lights_0;
    atomic<uint> device* cull_stats_0;
};


#line 236
float3 unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 230
float view_depth_0(float3 point_0, KernelContext_0 thread* kernelContext_1)
{
    return dot(kernelContext_1->params_0->depth_row_0, float4(point_0, 1.0f));
}


#line 286
float slice_start_0(uint index_0)
{
    return 0.10000000149011612f * pow(10000.0f, float(index_0) / 24.0f);
}


#line 264
bool cone_touches_sphere_0(float3 apex_0, float3 axis_0, float cos_outer_0, float range_0, float3 center_0, float radius_0)
{

    float3 offset_0 = center_0 - apex_0;
    float along_0 = dot(offset_0, axis_0);

#line 268
    bool _S1;
    if(along_0 < (- radius_0))
    {

#line 269
        _S1 = true;

#line 269
    }
    else
    {

#line 269
        _S1 = along_0 > (range_0 + radius_0);

#line 269
    }

#line 269
    if(_S1)
    {
        return false;
    }

#line 278
    return (cos_outer_0 * sqrt(max(dot(offset_0, offset_0) - along_0 * along_0, 0.0f)) - along_0 * sqrt(saturate(1.0f - cos_outer_0 * cos_outer_0))) <= radius_0;
}


#line 293
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], LightClusterParams_natural_0 constant* params_1 [[buffer(0)]], GpuLight_natural_0 device* lights_1 [[buffer(1)]], uint device* cluster_lights_1 [[buffer(2)]], atomic<uint> device* cull_stats_1 [[buffer(3)]])
{

#line 293
    thread KernelContext_0 kernelContext_2;

#line 293
    (&kernelContext_2)->params_0 = params_1;

#line 293
    (&kernelContext_2)->lights_0 = lights_1;

#line 293
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 293
    (&kernelContext_2)->cull_stats_0 = cull_stats_1;

    uint froxel_0 = thread_0.x;
    uint tiles_0 = params_1->grid_x_0 * params_1->grid_y_0;
    if(froxel_0 >= (tiles_0 * params_1->slices_0))
    {
        return;
    }

    uint tile_x_0 = froxel_0 % params_1->grid_x_0;
    uint _S2 = froxel_0 / params_1->grid_x_0;

#line 303
    uint tile_y_0 = _S2 % params_1->grid_y_0;
    uint slice_0 = froxel_0 / tiles_0;

#line 310
    float _S3 = float((&kernelContext_2)->params_0->viewport_x_0);

#line 310
    float _S4 = float((&kernelContext_2)->params_0->viewport_y_0);
    float2 pixel_min_0 = float2(float(tile_x_0), float(tile_y_0)) * float2(float((&kernelContext_2)->params_0->tile_pixels_0)) ;
    float2 pixel_max_0 = pixel_min_0 + float2(float((&kernelContext_2)->params_0->tile_pixels_0)) ;



    float _S5 = pixel_min_0.x / _S3 * 2.0f - 1.0f;

#line 316
    float _S6 = 1.0f - pixel_max_0.y / _S4 * 2.0f;
    float _S7 = pixel_max_0.x / _S3 * 2.0f - 1.0f;

#line 317
    float _S8 = 1.0f - pixel_min_0.y / _S4 * 2.0f;

#line 323
    thread array<float3, int(4)> corner_near_0;

#line 323
    float3 _S9 = unproject_0(float2(_S5, _S6), 1.0f, &kernelContext_2);
    corner_near_0[int(0)] = _S9;

#line 324
    float3 _S10 = unproject_0(float2(_S7, _S6), 1.0f, &kernelContext_2);
    corner_near_0[int(1)] = _S10;

#line 325
    float3 _S11 = unproject_0(float2(_S5, _S8), 1.0f, &kernelContext_2);
    corner_near_0[int(2)] = _S11;

#line 326
    float3 _S12 = unproject_0(float2(_S7, _S8), 1.0f, &kernelContext_2);
    corner_near_0[int(3)] = _S12;

#line 335
    bool perspective_1 = ((&kernelContext_2)->params_0->perspective_0) != 0U;
    float3 _S13 = float3(0.0f, 0.0f, 0.0f);

#line 336
    float3 ortho_lo_0;

#line 336
    float3 ortho_hi_0;

#line 336
    uint index_1;

#line 336
    bool _S14;

#line 336
    float eye_to_near_0;

#line 336
    float _S15;


    if(perspective_1)
    {

#line 339
        float _S16 = view_depth_0(corner_near_0[int(0)], &kernelContext_2);

#line 339
        eye_to_near_0 = _S16;

#line 339
        ortho_lo_0 = _S13;

#line 339
        ortho_hi_0 = _S13;

#line 339
    }
    else
    {

#line 339
        ortho_lo_0 = corner_near_0[int(0)];

#line 339
        ortho_hi_0 = corner_near_0[int(0)];

#line 339
        index_1 = 0U;

#line 348
        for(;;)
        {

#line 348
            if(index_1 < 4U)
            {
            }
            else
            {

#line 348
                break;
            }

            if(index_1 == 1U)
            {

#line 351
                _S14 = true;

#line 351
            }
            else
            {

#line 351
                _S14 = index_1 == 3U;

#line 351
            }

#line 351
            if(_S14)
            {

#line 351
                eye_to_near_0 = _S7;

#line 351
            }
            else
            {

#line 351
                eye_to_near_0 = _S5;

#line 351
            }
            if(index_1 < 2U)
            {

#line 352
                _S15 = _S6;

#line 352
            }
            else
            {

#line 352
                _S15 = _S8;

#line 352
            }

#line 352
            float3 _S17 = unproject_0(float2(eye_to_near_0, _S15), 0.0f, &kernelContext_2);

            float3 _S18 = min(ortho_lo_0, min(corner_near_0[index_1], _S17));
            float3 _S19 = max(ortho_hi_0, max(corner_near_0[index_1], _S17));

#line 348
            uint corner_0 = index_1 + 1U;

#line 348
            ortho_lo_0 = _S18;

#line 348
            ortho_hi_0 = _S19;

#line 348
            index_1 = corner_0;

#line 348
        }

#line 348
        eye_to_near_0 = 1.0f;

#line 339
    }

#line 361
    if(perspective_1)
    {

#line 361
        _S15 = slice_start_0(slice_0);

#line 361
    }
    else
    {

#line 361
        _S15 = 0.0f;

#line 361
    }
    if(!perspective_1)
    {

#line 362
        _S14 = true;

#line 362
    }
    else
    {

#line 362
        _S14 = (slice_0 + 1U) >= (params_1->slices_0);

#line 362
    }

#line 362
    float _S20;
    if(_S14)
    {

#line 363
        _S20 = 3.4028234663852886e+38f;

#line 363
    }
    else
    {

#line 363
        _S20 = slice_start_0(slice_0 + 1U);

#line 363
    }


    uint base_0 = froxel_0 * 17U;

#line 366
    index_1 = 0U;

#line 366
    uint kept_0 = 0U;

#line 366
    uint dropped_0 = 0U;


    for(;;)
    {

#line 369
        if(index_1 < ((&kernelContext_2)->params_0->light_count_0))
        {
        }
        else
        {

#line 369
            break;
        }
        GpuLight_natural_0 light_0 = (&kernelContext_2)->lights_0[index_1];

#line 371
        uint kept_1;

#line 371
        bool touches_0;

        if((light_0.kind_0) == 0U)
        {

#line 373
            touches_0 = true;

#line 373
        }
        else
        {

#line 373
            float4 _S21 = float4(light_0.position_0) ;

#line 382
            float3 center_1 = _S21.xyz;
            float radius_1 = _S21.w;

#line 383
            float3 box_lo_0;

#line 383
            float3 box_hi_0;


            if(perspective_1)
            {

#line 386
                float _S22 = view_depth_0(center_1, &kernelContext_2);

#line 393
                float light_lo_0 = _S22 - radius_1;
                float light_hi_0 = _S22 + radius_1;
                if(light_hi_0 < _S15)
                {

#line 395
                    _S14 = true;

#line 395
                }
                else
                {

#line 395
                    _S14 = light_lo_0 > _S20;

#line 395
                }

#line 395
                if(_S14)
                {
                    index_1 = index_1 + 1U;

#line 369
                    continue;
                }

#line 403
                float _S23 = max(max(_S15, light_lo_0), _S15);
                float _S24 = max(min(_S20, light_hi_0), _S23);


                float3 first_0 = (&kernelContext_2)->params_0->eye_0.xyz + (corner_near_0[int(0)] - (&kernelContext_2)->params_0->eye_0.xyz) * float3((_S23 / eye_to_near_0)) ;

#line 407
                box_lo_0 = first_0;

#line 407
                box_hi_0 = first_0;

#line 407
                kept_1 = 0U;


                for(;;)
                {

#line 410
                    if(kept_1 < 4U)
                    {
                    }
                    else
                    {

#line 410
                        break;
                    }
                    float3 ray_0 = corner_near_0[kept_1] - (&kernelContext_2)->params_0->eye_0.xyz;
                    float3 at_lo_0 = (&kernelContext_2)->params_0->eye_0.xyz + ray_0 * float3((_S23 / eye_to_near_0)) ;
                    float3 at_hi_0 = (&kernelContext_2)->params_0->eye_0.xyz + ray_0 * float3((_S24 / eye_to_near_0)) ;
                    float3 _S25 = min(box_lo_0, min(at_lo_0, at_hi_0));
                    float3 _S26 = max(box_hi_0, max(at_lo_0, at_hi_0));

#line 410
                    uint corner_1 = kept_1 + 1U;

#line 410
                    box_lo_0 = _S25;

#line 410
                    box_hi_0 = _S26;

#line 410
                    kept_1 = corner_1;

#line 410
                }

#line 386
            }
            else
            {

#line 386
                box_lo_0 = ortho_lo_0;

#line 386
                box_hi_0 = ortho_hi_0;

#line 386
            }

#line 428
            float3 offset_1 = center_1 - clamp(center_1, box_lo_0, box_hi_0);
            bool touches_1 = (dot(offset_1, offset_1)) <= (radius_1 * radius_1);

#line 435
            if(touches_1)
            {

#line 435
                _S14 = (light_0.kind_0) == 2U;

#line 435
            }
            else
            {

#line 435
                _S14 = false;

#line 435
            }

#line 435
            if(_S14)
            {

#line 435
                float4 _S27 = float4(light_0.direction_0) ;

#line 435
                touches_0 = cone_touches_sphere_0(center_1, _S27.xyz, _S27.w, radius_1, (box_lo_0 + box_hi_0) * float3(0.5f) , length(box_hi_0 - box_lo_0) * 0.5f);

#line 435
            }
            else
            {

#line 435
                touches_0 = touches_1;

#line 435
            }

#line 373
        }

#line 444
        if(!touches_0)
        {
            index_1 = index_1 + 1U;

#line 369
            continue;
        }

#line 369
        uint dropped_1;

#line 448
        if(kept_0 < 16U)
        {
            *((&kernelContext_2)->cluster_lights_0+(base_0 + 1U + kept_0)) = index_1;

#line 450
            kept_1 = kept_0 + 1U;

#line 450
            dropped_1 = dropped_0;

#line 448
        }
        else
        {

#line 458
            uint dropped_2 = dropped_0 + 1U;

#line 458
            kept_1 = kept_0;

#line 458
            dropped_1 = dropped_2;

#line 448
        }

#line 448
        kept_0 = kept_1;

#line 448
        dropped_0 = dropped_1;

#line 369
        index_1 = index_1 + 1U;

#line 369
    }

#line 462
    *((&kernelContext_2)->cluster_lights_0+base_0) = kept_0;
    if(dropped_0 > 0U)
    {
        uint _S28 = atomic_fetch_add_explicit((&kernelContext_2)->cull_stats_0+2U, dropped_0, memory_order_relaxed);

#line 463
    }



    return;
}

