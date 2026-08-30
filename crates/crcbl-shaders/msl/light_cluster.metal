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
    packed_float4 tangent_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint flags_0;
};


#line 453 "shaders/light_cluster.slang"
struct KernelContext_0
{
    LightClusterParams_natural_0 constant* params_0;
    GpuLight_natural_0 device* lights_0;
    uint device* cluster_lights_0;
    atomic<uint> device* cull_stats_0;
};


#line 224
float3 unproject_0(float2 ndc_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    float4 world_0 = (((float4(ndc_0, depth_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(0)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(1)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(2)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(0)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(1)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(2)][int(3)], kernelContext_0->params_0->inverse_view_proj_0.data_0[int(3)][int(3)]))));
    return world_0.xyz / float3(world_0.w) ;
}


#line 218
float view_depth_0(float3 point_0, KernelContext_0 thread* kernelContext_1)
{
    return dot(kernelContext_1->params_0->depth_row_0, float4(point_0, 1.0f));
}


#line 274
float slice_start_0(uint index_0)
{
    return 0.10000000149011612f * pow(10000.0f, float(index_0) / 24.0f);
}


#line 252
bool cone_touches_sphere_0(float3 apex_0, float3 axis_0, float cos_outer_0, float range_0, float3 center_0, float radius_0)
{

    float3 offset_0 = center_0 - apex_0;
    float along_0 = dot(offset_0, axis_0);

#line 256
    bool _S1;
    if(along_0 < (- radius_0))
    {

#line 257
        _S1 = true;

#line 257
    }
    else
    {

#line 257
        _S1 = along_0 > (range_0 + radius_0);

#line 257
    }

#line 257
    if(_S1)
    {
        return false;
    }

#line 266
    return (cos_outer_0 * sqrt(max(dot(offset_0, offset_0) - along_0 * along_0, 0.0f)) - along_0 * sqrt(saturate(1.0f - cos_outer_0 * cos_outer_0))) <= radius_0;
}


#line 281
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], LightClusterParams_natural_0 constant* params_1 [[buffer(0)]], GpuLight_natural_0 device* lights_1 [[buffer(1)]], uint device* cluster_lights_1 [[buffer(2)]], atomic<uint> device* cull_stats_1 [[buffer(3)]])
{

#line 281
    thread KernelContext_0 kernelContext_2;

#line 281
    (&kernelContext_2)->params_0 = params_1;

#line 281
    (&kernelContext_2)->lights_0 = lights_1;

#line 281
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 281
    (&kernelContext_2)->cull_stats_0 = cull_stats_1;

    uint froxel_0 = thread_0.x;
    uint tiles_0 = params_1->grid_x_0 * params_1->grid_y_0;
    if(froxel_0 >= (tiles_0 * params_1->slices_0))
    {
        return;
    }

    uint tile_x_0 = froxel_0 % params_1->grid_x_0;
    uint _S2 = froxel_0 / params_1->grid_x_0;

#line 291
    uint tile_y_0 = _S2 % params_1->grid_y_0;
    uint slice_0 = froxel_0 / tiles_0;

#line 298
    float _S3 = float((&kernelContext_2)->params_0->viewport_x_0);

#line 298
    float _S4 = float((&kernelContext_2)->params_0->viewport_y_0);
    float2 pixel_min_0 = float2(float(tile_x_0), float(tile_y_0)) * float2(float((&kernelContext_2)->params_0->tile_pixels_0)) ;
    float2 pixel_max_0 = pixel_min_0 + float2(float((&kernelContext_2)->params_0->tile_pixels_0)) ;



    float _S5 = pixel_min_0.x / _S3 * 2.0f - 1.0f;

#line 304
    float _S6 = 1.0f - pixel_max_0.y / _S4 * 2.0f;
    float _S7 = pixel_max_0.x / _S3 * 2.0f - 1.0f;

#line 305
    float _S8 = 1.0f - pixel_min_0.y / _S4 * 2.0f;

#line 311
    thread array<float3, int(4)> corner_near_0;

#line 311
    float3 _S9 = unproject_0(float2(_S5, _S6), 1.0f, &kernelContext_2);
    corner_near_0[int(0)] = _S9;

#line 312
    float3 _S10 = unproject_0(float2(_S7, _S6), 1.0f, &kernelContext_2);
    corner_near_0[int(1)] = _S10;

#line 313
    float3 _S11 = unproject_0(float2(_S5, _S8), 1.0f, &kernelContext_2);
    corner_near_0[int(2)] = _S11;

#line 314
    float3 _S12 = unproject_0(float2(_S7, _S8), 1.0f, &kernelContext_2);
    corner_near_0[int(3)] = _S12;

#line 323
    bool perspective_1 = ((&kernelContext_2)->params_0->perspective_0) != 0U;
    float3 _S13 = float3(0.0f, 0.0f, 0.0f);

#line 324
    float3 ortho_lo_0;

#line 324
    float3 ortho_hi_0;

#line 324
    uint index_1;

#line 324
    bool _S14;

#line 324
    float eye_to_near_0;

#line 324
    float _S15;


    if(perspective_1)
    {

#line 327
        float _S16 = view_depth_0(corner_near_0[int(0)], &kernelContext_2);

#line 327
        eye_to_near_0 = _S16;

#line 327
        ortho_lo_0 = _S13;

#line 327
        ortho_hi_0 = _S13;

#line 327
    }
    else
    {

#line 327
        ortho_lo_0 = corner_near_0[int(0)];

#line 327
        ortho_hi_0 = corner_near_0[int(0)];

#line 327
        index_1 = 0U;

#line 336
        for(;;)
        {

#line 336
            if(index_1 < 4U)
            {
            }
            else
            {

#line 336
                break;
            }

            if(index_1 == 1U)
            {

#line 339
                _S14 = true;

#line 339
            }
            else
            {

#line 339
                _S14 = index_1 == 3U;

#line 339
            }

#line 339
            if(_S14)
            {

#line 339
                eye_to_near_0 = _S7;

#line 339
            }
            else
            {

#line 339
                eye_to_near_0 = _S5;

#line 339
            }
            if(index_1 < 2U)
            {

#line 340
                _S15 = _S6;

#line 340
            }
            else
            {

#line 340
                _S15 = _S8;

#line 340
            }

#line 340
            float3 _S17 = unproject_0(float2(eye_to_near_0, _S15), 0.0f, &kernelContext_2);

            float3 _S18 = min(ortho_lo_0, min(corner_near_0[index_1], _S17));
            float3 _S19 = max(ortho_hi_0, max(corner_near_0[index_1], _S17));

#line 336
            uint corner_0 = index_1 + 1U;

#line 336
            ortho_lo_0 = _S18;

#line 336
            ortho_hi_0 = _S19;

#line 336
            index_1 = corner_0;

#line 336
        }

#line 336
        eye_to_near_0 = 1.0f;

#line 327
    }

#line 349
    if(perspective_1)
    {

#line 349
        _S15 = slice_start_0(slice_0);

#line 349
    }
    else
    {

#line 349
        _S15 = 0.0f;

#line 349
    }
    if(!perspective_1)
    {

#line 350
        _S14 = true;

#line 350
    }
    else
    {

#line 350
        _S14 = (slice_0 + 1U) >= (params_1->slices_0);

#line 350
    }

#line 350
    float _S20;
    if(_S14)
    {

#line 351
        _S20 = 3.4028234663852886e+38f;

#line 351
    }
    else
    {

#line 351
        _S20 = slice_start_0(slice_0 + 1U);

#line 351
    }


    uint base_0 = froxel_0 * 17U;

#line 354
    index_1 = 0U;

#line 354
    uint kept_0 = 0U;

#line 354
    uint dropped_0 = 0U;


    for(;;)
    {

#line 357
        if(index_1 < ((&kernelContext_2)->params_0->light_count_0))
        {
        }
        else
        {

#line 357
            break;
        }
        GpuLight_natural_0 light_0 = (&kernelContext_2)->lights_0[index_1];

#line 359
        uint kept_1;

#line 359
        bool touches_0;

        if((light_0.kind_0) == 0U)
        {

#line 361
            touches_0 = true;

#line 361
        }
        else
        {

#line 361
            float4 _S21 = float4(light_0.position_0) ;

#line 370
            float3 center_1 = _S21.xyz;
            float radius_1 = _S21.w;

#line 371
            float3 box_lo_0;

#line 371
            float3 box_hi_0;


            if(perspective_1)
            {

#line 374
                float _S22 = view_depth_0(center_1, &kernelContext_2);

#line 381
                float light_lo_0 = _S22 - radius_1;
                float light_hi_0 = _S22 + radius_1;
                if(light_hi_0 < _S15)
                {

#line 383
                    _S14 = true;

#line 383
                }
                else
                {

#line 383
                    _S14 = light_lo_0 > _S20;

#line 383
                }

#line 383
                if(_S14)
                {
                    index_1 = index_1 + 1U;

#line 357
                    continue;
                }

#line 391
                float _S23 = max(max(_S15, light_lo_0), _S15);
                float _S24 = max(min(_S20, light_hi_0), _S23);


                float3 first_0 = (&kernelContext_2)->params_0->eye_0.xyz + (corner_near_0[int(0)] - (&kernelContext_2)->params_0->eye_0.xyz) * float3((_S23 / eye_to_near_0)) ;

#line 395
                box_lo_0 = first_0;

#line 395
                box_hi_0 = first_0;

#line 395
                kept_1 = 0U;


                for(;;)
                {

#line 398
                    if(kept_1 < 4U)
                    {
                    }
                    else
                    {

#line 398
                        break;
                    }
                    float3 ray_0 = corner_near_0[kept_1] - (&kernelContext_2)->params_0->eye_0.xyz;
                    float3 at_lo_0 = (&kernelContext_2)->params_0->eye_0.xyz + ray_0 * float3((_S23 / eye_to_near_0)) ;
                    float3 at_hi_0 = (&kernelContext_2)->params_0->eye_0.xyz + ray_0 * float3((_S24 / eye_to_near_0)) ;
                    float3 _S25 = min(box_lo_0, min(at_lo_0, at_hi_0));
                    float3 _S26 = max(box_hi_0, max(at_lo_0, at_hi_0));

#line 398
                    uint corner_1 = kept_1 + 1U;

#line 398
                    box_lo_0 = _S25;

#line 398
                    box_hi_0 = _S26;

#line 398
                    kept_1 = corner_1;

#line 398
                }

#line 374
            }
            else
            {

#line 374
                box_lo_0 = ortho_lo_0;

#line 374
                box_hi_0 = ortho_hi_0;

#line 374
            }

#line 416
            float3 offset_1 = center_1 - clamp(center_1, box_lo_0, box_hi_0);
            bool touches_1 = (dot(offset_1, offset_1)) <= (radius_1 * radius_1);

#line 423
            if(touches_1)
            {

#line 423
                _S14 = (light_0.kind_0) == 2U;

#line 423
            }
            else
            {

#line 423
                _S14 = false;

#line 423
            }

#line 423
            if(_S14)
            {

#line 423
                float4 _S27 = float4(light_0.direction_0) ;

#line 423
                touches_0 = cone_touches_sphere_0(center_1, _S27.xyz, _S27.w, radius_1, (box_lo_0 + box_hi_0) * float3(0.5f) , length(box_hi_0 - box_lo_0) * 0.5f);

#line 423
            }
            else
            {

#line 423
                touches_0 = touches_1;

#line 423
            }

#line 361
        }

#line 432
        if(!touches_0)
        {
            index_1 = index_1 + 1U;

#line 357
            continue;
        }

#line 357
        uint dropped_1;

#line 436
        if(kept_0 < 16U)
        {
            *((&kernelContext_2)->cluster_lights_0+(base_0 + 1U + kept_0)) = index_1;

#line 438
            kept_1 = kept_0 + 1U;

#line 438
            dropped_1 = dropped_0;

#line 436
        }
        else
        {

#line 446
            uint dropped_2 = dropped_0 + 1U;

#line 446
            kept_1 = kept_0;

#line 446
            dropped_1 = dropped_2;

#line 436
        }

#line 436
        kept_0 = kept_1;

#line 436
        dropped_0 = dropped_1;

#line 357
        index_1 = index_1 + 1U;

#line 357
    }

#line 450
    *((&kernelContext_2)->cluster_lights_0+base_0) = kept_0;
    if(dropped_0 > 0U)
    {
        uint _S28 = atomic_fetch_add_explicit((&kernelContext_2)->cull_stats_0+2U, dropped_0, memory_order_relaxed);

#line 451
    }



    return;
}

