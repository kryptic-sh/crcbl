#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 102 "shaders/grass_gen.slang"
struct GrassGenParams_0
{
    float4 camera_0;
    float4 ground_0;
    float4 cover_0;
    uint4 maps_0;
    uint4 limits_0;
    uint4 looks_0;
};


#line 135
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 135
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_1;
    packed_uint4 lanes_0;
};


#line 135
struct GrassBlade_natural_0
{
    packed_float4 root_color_0;
    packed_float4 tip_color_0;
    packed_float4 size_0;
    packed_float4 occlusion_0;
    packed_float4 glow_0;
    packed_float4 patch_0;
    packed_uint4 flags_0;
};


#line 57
struct WindParams_0
{
    float2 baseDirection_0;
    float baseSpeed_0;
    float gustAmplitude_0;
    float gustPhase_0;
    float invGustWavelength_0;
    float2 pad0_0;
    float2 directionUv_0;
    float2 directionUvPerMetre_0;
    float2 intensityUv_0;
    float2 intensityUvPerMetre_0;
};


#line 57
struct KernelContext_0
{
    GrassGenParams_0 constant* grass_0;
    atomic<uint> device* drawArgs_0;
    GrassTile_0 constant* tile_1;
    GrassInstance_natural_0 device* grassCells_0;
    texture2d<float, access::sample> grassCover_0;
    GrassBlade_natural_0 device* blades_0;
    texture2d<float, access::sample> grassGround_0;
    WindParams_0 constant* wind_0;
    texture2d<float, access::sample> windDirectionLayer_0;
    sampler windSampler_0;
    texture2d<float, access::sample> windIntensityLayer_0;
    GrassInstance_natural_0 device* instances_0;
};


#line 467
[[kernel]] void clearMain(uint3 thread_0 [[thread_position_in_grid]], GrassGenParams_0 constant* grass_1 [[buffer(1)]], atomic<uint> device* drawArgs_1 [[buffer(5)]], GrassTile_0 constant* tile_2 [[buffer(2)]], GrassInstance_natural_0 device* grassCells_1 [[buffer(6)]], texture2d<float, access::sample> grassCover_1 [[texture(3)]], GrassBlade_natural_0 device* blades_1 [[buffer(3)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(0)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(0)]], sampler windSampler_1 [[sampler(0)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(4)]])
{

#line 467
    thread KernelContext_0 kernelContext_0;

#line 467
    (&kernelContext_0)->grass_0 = grass_1;

#line 467
    (&kernelContext_0)->drawArgs_0 = drawArgs_1;

#line 467
    (&kernelContext_0)->tile_1 = tile_2;

#line 467
    (&kernelContext_0)->grassCells_0 = grassCells_1;

#line 467
    (&kernelContext_0)->grassCover_0 = grassCover_1;

#line 467
    (&kernelContext_0)->blades_0 = blades_1;

#line 467
    (&kernelContext_0)->grassGround_0 = grassGround_1;

#line 467
    (&kernelContext_0)->wind_0 = wind_1;

#line 467
    (&kernelContext_0)->windDirectionLayer_0 = windDirectionLayer_1;

#line 467
    (&kernelContext_0)->windSampler_0 = windSampler_1;

#line 467
    (&kernelContext_0)->windIntensityLayer_0 = windIntensityLayer_1;

#line 467
    (&kernelContext_0)->instances_0 = instances_1;

    uint slot_1 = thread_0.x;
    if(slot_1 >= (grass_1->limits_0.w))
    {
        return;
    }

#line 478
    uint _S1 = 2U * (grass_1->limits_0.z / 4U) * 16U * 4U * 6U;

#line 478
    uint draw_0 = 0U;
    for(;;)
    {

#line 479
        if(draw_0 < 3U)
        {
        }
        else
        {

#line 479
            break;
        }
        uint at_0 = (slot_1 * 3U + draw_0) * 4U;

#line 481
        uint vertices_0;
        if(draw_0 == 1U)
        {

#line 482
            vertices_0 = 1536U;

#line 482
        }
        else
        {

#line 483
            if(draw_0 == 2U)
            {

#line 483
                vertices_0 = _S1;

#line 483
            }
            else
            {

#line 483
                vertices_0 = 12U;

#line 483
            }

#line 482
        }


        atomic_store_explicit((&kernelContext_0)->drawArgs_0+at_0, vertices_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 1U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 2U), 0U, memory_order_relaxed);



        atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 3U), 0U, memory_order_relaxed);

#line 479
        draw_0 = draw_0 + 1U;

#line 479
    }

#line 493
    return;
}


#line 354
uint grass_hash_0(uint value_0)
{
    uint state_0 = value_0 * 747796405U + 2891336453U;
    uint word_0 = ((state_0 >> ((state_0 >> 28U) + 4U)) ^ state_0) * 277803737U;
    return (word_0 >> 22U) ^ word_0;
}


#line 367
float2 grass_unit_pair_0(uint lane_0)
{
    return float2(float(lane_0 & 65535U), float((lane_0 >> 16U) & 65535U)) * float2(0.0000152587890625f) ;
}


#line 441
float4 grass_cover_under_0(float2 world_0, KernelContext_0 thread* kernelContext_1)
{



    int3 _S2 = int3(clamp(int2(floor((world_0 - kernelContext_1->grass_0->cover_0.xy) * float2(kernelContext_1->grass_0->cover_0.w)  + float2(0.5f) )), int2(int(0), int(0)), int2(int(kernelContext_1->grass_0->maps_0.z) - int(1), int(kernelContext_1->grass_0->maps_0.w) - int(1))), int(0));

#line 446
    return ((kernelContext_1->grassCover_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 453
uint grass_unorm8_0(float value_1)
{
    return uint(value_1 * 255.0f + 0.5f);
}


#line 404
float grass_ground_texel_0(int2 texel_0, KernelContext_0 thread* kernelContext_2)
{

    int3 _S3 = int3(clamp(texel_0, int2(int(0), int(0)), int2(int(kernelContext_2->grass_0->maps_0.x) - int(1), int(kernelContext_2->grass_0->maps_0.y) - int(1))), int(0));

#line 407
    return ((kernelContext_2->grassGround_0).read(vec<uint,2>(((_S3)).xy), uint(((_S3)).z)).x);
}


#line 395
struct GrassGround_0
{
    float height_0;
    float3 normal_0;
};


#line 413
GrassGround_0 grass_ground_under_0(float2 world_1, KernelContext_0 thread* kernelContext_3)
{
    float2 at_1 = (world_1 - kernelContext_3->grass_0->ground_0.xy) * float2(kernelContext_3->grass_0->ground_0.w) ;
    float2 base_0 = floor(at_1);
    float2 blend_0 = at_1 - base_0;
    int2 _S4 = int2(base_0);

#line 418
    float _S5 = grass_ground_texel_0(_S4, kernelContext_3);

#line 418
    float _S6 = grass_ground_texel_0(_S4 + int2(int(1), int(0)), kernelContext_3);

#line 418
    float _S7 = grass_ground_texel_0(_S4 + int2(int(0), int(1)), kernelContext_3);

#line 418
    float _S8 = grass_ground_texel_0(_S4 + int2(int(1), int(1)), kernelContext_3);

#line 425
    float _S9 = blend_0.x;

#line 425
    float _S10 = _S6 - _S5;

#line 425
    float lower_0 = _S5 + _S9 * _S10;
    float _S11 = _S8 - _S7;

#line 424
    thread GrassGround_0 under_0;


    (&under_0)->height_0 = lower_0 + blend_0.y * (_S7 + _S9 * _S11 - lower_0);

#line 434
    (&under_0)->normal_0 = normalize(float3(- (0.5f * (_S10 + _S11)), kernelContext_3->grass_0->ground_0.z, - (0.5f * (_S7 - _S5 + (_S8 - _S6)))));
    return under_0;
}


#line 246
float windSmoothTriangle_0(float u_0)
{
    float s_0 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}



float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_4)
{


    float2 _S12 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_4->windDirectionLayer_0).sample((kernelContext_4->windSampler_0), (kernelContext_4->wind_0->directionUv_0 + _S12 * kernelContext_4->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_4->windIntensityLayer_0).sample((kernelContext_4->windSampler_0), (kernelContext_4->wind_0->intensityUv_0 + _S12 * kernelContext_4->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;



    float2 base_1 = kernelContext_4->wind_0->baseDirection_0;
    float _S13 = kernelContext_4->wind_0->baseDirection_0.x;

#line 270
    float _S14 = deflection_0.x;

#line 270
    float _S15 = kernelContext_4->wind_0->baseDirection_0.y;

#line 270
    float _S16 = deflection_0.y;

#line 270
    float2 turned_0 = float2(_S13 * _S14 - _S15 * _S16, _S13 * _S16 + _S15 * _S14);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 272
    float2 direction_0;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 274
        direction_0 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 274
    }
    else
    {

#line 274
        direction_0 = base_1;

#line 274
    }

#line 279
    float speed_0 = intensity_0 * kernelContext_4->wind_0->baseSpeed_0 * (1.0f + kernelContext_4->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S12, base_1) * kernelContext_4->wind_0->invGustWavelength_0 + kernelContext_4->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_0.x * speed_0, 0.0f, direction_0.y * speed_0);
}


#line 379
float3 grass_lean_0(float3 velocity_0, float height_1)
{
    float speed_1 = length(velocity_0);
    float bend_0 = height_1 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_0 = velocity_0.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;

#line 390
    float _S17 = bend_0 * bend_0;
    return float3(along_0.x, - (_S17 / (height_1 + sqrt(max(height_1 * height_1 - _S17, 0.0f)))), along_0.y);
}


#line 181
struct GrassInstance_0
{
    float4 root_0;
    float4 facing_0;
    float4 lean_0;
    float4 ground_1;
    uint4 lanes_0;
};


#line 498
[[kernel]] void generateMain(uint3 thread_1 [[thread_position_in_grid]], GrassGenParams_0 constant* grass_2 [[buffer(1)]], atomic<uint> device* drawArgs_2 [[buffer(5)]], GrassTile_0 constant* tile_3 [[buffer(2)]], GrassInstance_natural_0 device* grassCells_2 [[buffer(6)]], texture2d<float, access::sample> grassCover_2 [[texture(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(3)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(0)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(0)]], sampler windSampler_2 [[sampler(0)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(4)]])
{

#line 498
    thread KernelContext_0 kernelContext_5;

#line 498
    (&kernelContext_5)->grass_0 = grass_2;

#line 498
    (&kernelContext_5)->drawArgs_0 = drawArgs_2;

#line 498
    (&kernelContext_5)->tile_1 = tile_3;

#line 498
    (&kernelContext_5)->grassCells_0 = grassCells_2;

#line 498
    (&kernelContext_5)->grassCover_0 = grassCover_2;

#line 498
    (&kernelContext_5)->blades_0 = blades_2;

#line 498
    (&kernelContext_5)->grassGround_0 = grassGround_2;

#line 498
    (&kernelContext_5)->wind_0 = wind_2;

#line 498
    (&kernelContext_5)->windDirectionLayer_0 = windDirectionLayer_2;

#line 498
    (&kernelContext_5)->windSampler_0 = windSampler_2;

#line 498
    (&kernelContext_5)->windIntensityLayer_0 = windIntensityLayer_2;

#line 498
    (&kernelContext_5)->instances_0 = instances_2;

    uint cells_0 = grass_2->limits_0.z;
    uint capacity_0 = grass_2->limits_0.x;

#line 507
    uint _S18 = thread_1.x;

#line 507
    if(_S18 >= (min(cells_0 * cells_0, capacity_0)))
    {
        return;
    }


    uint cell_0 = (&kernelContext_5)->tile_1->slot_0.x * capacity_0 + _S18;



    thread GrassInstance_0 empty_0;
    float4 _S19 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 518
    (&empty_0)->root_0 = _S19;
    (&empty_0)->facing_0 = _S19;
    (&empty_0)->lean_0 = _S19;
    (&empty_0)->ground_1 = _S19;
    (&empty_0)->lanes_0 = uint4(0U, 0U, 0U, 0U);

#line 522
    GrassInstance_natural_0 device* _S20 = (&kernelContext_5)->grassCells_0+cell_0;

#line 522
    _S20->root_0 = packed_float4(empty_0.root_0) ;

#line 522
    _S20->facing_0 = packed_float4(empty_0.facing_0) ;

#line 522
    _S20->lean_0 = packed_float4(empty_0.lean_0) ;

#line 522
    _S20->ground_1 = packed_float4(empty_0.ground_1) ;

#line 522
    _S20->lanes_0 = packed_uint4(empty_0.lanes_0) ;

    float side_0 = (&kernelContext_5)->tile_1->tile_0.z / float(cells_0);
    float2 jitter_0 = grass_unit_pair_0(grass_hash_0(cell_0));
    float2 _S21 = (&kernelContext_5)->tile_1->tile_0.xy;
    uint _S22 = _S18 % cells_0;

#line 527
    float _S23 = float(_S22);

#line 527
    uint _S24 = _S18 / cells_0;

#line 527
    float2 world_2 = _S21 + (float2(_S23, float(_S24)) + jitter_0) * float2(side_0) ;

#line 527
    float4 _S25 = grass_cover_under_0(world_2, &kernelContext_5);

#line 534
    if(((grass_hash_0(cell_0 ^ 2654435769U)) & 65535U) >= (grass_unorm8_0(_S25.x) * 257U))
    {
        return;
    }

    uint _S26 = min(grass_unorm8_0(_S25.y), max(grass_2->limits_0.y, 1U) - 1U);
    GrassBlade_natural_0 blade_0 = (&kernelContext_5)->blades_0[_S26];

#line 540
    GrassGround_0 _S27 = grass_ground_under_0(world_2, &kernelContext_5);


    float3 root_1 = float3(world_2.x, _S27.height_0, world_2.y);



    float3 relative_0 = root_1 - (&kernelContext_5)->grass_0->camera_0.xyz;
    if((dot(relative_0, relative_0)) > ((&kernelContext_5)->grass_0->camera_0.w * (&kernelContext_5)->grass_0->camera_0.w))
    {
        return;
    }



    float2 spread_0 = grass_unit_pair_0(grass_hash_0(cell_0 ^ 3266489909U));

#line 555
    float4 _S28 = float4(blade_0.size_0) ;
    float height_2 = _S28.x * (1.0f - _S28.z * spread_0.x);
    float half_width_0 = _S28.y * (1.0f - _S28.w * spread_0.y);

#line 564
    float2 square_0 = grass_unit_pair_0(grass_hash_0(cell_0 ^ 2246822507U)) * float2(2.0f)  - float2(1.0f) ;
    float square_length_0 = dot(square_0, square_0);

#line 565
    float2 facing_1;
    if(square_length_0 > 9.99999993922529029e-09f)
    {

#line 566
        facing_1 = square_0 / float2(sqrt(square_length_0)) ;

#line 566
    }
    else
    {

#line 566
        facing_1 = float2(1.0f, 0.0f);

#line 566
    }

#line 566
    float3 _S29 = windSample_0(relative_0, &kernelContext_5);



    float3 lean_1 = grass_lean_0(_S29, height_2);

    thread GrassInstance_0 instance_0;
    (&instance_0)->root_0 = float4(root_1, height_2);
    (&instance_0)->facing_0 = float4(facing_1, half_width_0, grass_unit_pair_0(grass_hash_0(cell_0 ^ 668265263U)).x);

    (&instance_0)->lean_0 = float4(lean_1, 0.0f);
    (&instance_0)->ground_1 = float4(_S27.normal_0, 0.0f);
    (&instance_0)->lanes_0 = uint4(cell_0, _S26, 0U, 0U);

#line 578
    GrassInstance_natural_0 device* _S30 = (&kernelContext_5)->grassCells_0+cell_0;

#line 578
    _S30->root_0 = packed_float4(instance_0.root_0) ;

#line 578
    _S30->facing_0 = packed_float4(instance_0.facing_0) ;

#line 578
    _S30->lean_0 = packed_float4(instance_0.lean_0) ;

#line 578
    _S30->ground_1 = packed_float4(instance_0.ground_1) ;

#line 578
    _S30->lanes_0 = packed_uint4(instance_0.lanes_0) ;



    uint slot_args_0 = (&kernelContext_5)->tile_1->slot_0.x * 3U;
    if(((uint4(blade_0.flags_0) ).x) == 1U)
    {

#line 589
        atomic_store_explicit((&kernelContext_5)->drawArgs_0+((slot_args_0 + 1U) * 4U + 1U), (&kernelContext_5)->grass_0->looks_0.x, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_5)->drawArgs_0+((slot_args_0 + 2U) * 4U + 1U), (&kernelContext_5)->grass_0->looks_0.y, memory_order_relaxed);
        return;
    }

#line 598
    uint at_2 = atomic_fetch_add_explicit((&kernelContext_5)->drawArgs_0+(slot_args_0 * 4U + 1U), 1U, memory_order_relaxed);

#line 598
    GrassInstance_natural_0 device* _S31 = (&kernelContext_5)->instances_0+((&kernelContext_5)->tile_1->slot_0.x * capacity_0 + at_2);

#line 598
    _S31->root_0 = packed_float4(instance_0.root_0) ;

#line 598
    _S31->facing_0 = packed_float4(instance_0.facing_0) ;

#line 598
    _S31->lean_0 = packed_float4(instance_0.lean_0) ;

#line 598
    _S31->ground_1 = packed_float4(instance_0.ground_1) ;

#line 598
    _S31->lanes_0 = packed_uint4(instance_0.lanes_0) ;

    return;
}

