#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 131 "shaders/grass_gen.slang"
struct GrassGenParams_0
{
    float4 camera_0;
    float4 ground_0;
    float4 cover_0;
    uint4 maps_0;
    uint4 limits_0;
    uint4 looks_0;
    float4 lod_0;
};


#line 168
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 168
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_1;
    packed_float4 clump_0;
    packed_uint4 lanes_0;
};


#line 168
struct GrassBlade_natural_0
{
    packed_float4 root_color_0;
    packed_float4 tip_color_0;
    packed_float4 size_0;
    packed_float4 occlusion_0;
    packed_float4 glow_0;
    packed_float4 patch_0;
    packed_float4 shape_0;
    packed_float4 clump_1;
    packed_uint4 flags_0;
};


#line 86
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


#line 265
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


#line 602
[[kernel]] void clearMain(uint3 thread_0 [[thread_position_in_grid]], GrassGenParams_0 constant* grass_1 [[buffer(1)]], atomic<uint> device* drawArgs_1 [[buffer(5)]], GrassTile_0 constant* tile_2 [[buffer(2)]], GrassInstance_natural_0 device* grassCells_1 [[buffer(6)]], texture2d<float, access::sample> grassCover_1 [[texture(3)]], GrassBlade_natural_0 device* blades_1 [[buffer(3)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(0)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(0)]], sampler windSampler_1 [[sampler(0)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(4)]])
{

#line 602
    thread KernelContext_0 kernelContext_0;

#line 602
    (&kernelContext_0)->grass_0 = grass_1;

#line 602
    (&kernelContext_0)->drawArgs_0 = drawArgs_1;

#line 602
    (&kernelContext_0)->tile_1 = tile_2;

#line 602
    (&kernelContext_0)->grassCells_0 = grassCells_1;

#line 602
    (&kernelContext_0)->grassCover_0 = grassCover_1;

#line 602
    (&kernelContext_0)->blades_0 = blades_1;

#line 602
    (&kernelContext_0)->grassGround_0 = grassGround_1;

#line 602
    (&kernelContext_0)->wind_0 = wind_1;

#line 602
    (&kernelContext_0)->windDirectionLayer_0 = windDirectionLayer_1;

#line 602
    (&kernelContext_0)->windSampler_0 = windSampler_1;

#line 602
    (&kernelContext_0)->windIntensityLayer_0 = windIntensityLayer_1;

#line 602
    (&kernelContext_0)->instances_0 = instances_1;

    uint slot_1 = thread_0.x;
    if(slot_1 >= (grass_1->limits_0.w))
    {
        return;
    }

#line 613
    uint _S1 = 2U * (grass_1->limits_0.z / 4U) * 16U * 4U * 6U;

#line 613
    uint draw_0 = 0U;
    for(;;)
    {

#line 614
        if(draw_0 < 5U)
        {
        }
        else
        {

#line 614
            break;
        }
        uint at_0 = (slot_1 * 5U + draw_0) * 4U;

#line 616
        uint vertices_0;
        if(draw_0 == 1U)
        {

#line 617
            vertices_0 = 1536U;

#line 617
        }
        else
        {

#line 618
            if(draw_0 == 2U)
            {

#line 618
                vertices_0 = _S1;

#line 618
            }
            else
            {

#line 619
                if(draw_0 == 3U)
                {

#line 619
                    vertices_0 = 15U;

#line 619
                }
                else
                {

#line 620
                    if(draw_0 == 4U)
                    {

#line 620
                        vertices_0 = 7U;

#line 620
                    }
                    else
                    {

#line 620
                        vertices_0 = 12U;

#line 620
                    }

#line 619
                }

#line 618
            }

#line 617
        }

#line 622
        atomic_store_explicit((&kernelContext_0)->drawArgs_0+at_0, vertices_0, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 1U), 0U, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 2U), 0U, memory_order_relaxed);



        atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 3U), 0U, memory_order_relaxed);

#line 614
        draw_0 = draw_0 + 1U;

#line 614
    }

#line 630
    return;
}


#line 427
uint grass_hash_0(uint value_0)
{
    uint state_0 = value_0 * 747796405U + 2891336453U;
    uint word_0 = ((state_0 >> ((state_0 >> 28U) + 4U)) ^ state_0) * 277803737U;
    return (word_0 >> 22U) ^ word_0;
}


#line 440
float2 grass_unit_pair_0(uint lane_0)
{
    return float2(float(lane_0 & 65535U), float((lane_0 >> 16U) & 65535U)) * float2(0.0000152587890625f) ;
}


#line 514
float4 grass_cover_under_0(float2 world_0, KernelContext_0 thread* kernelContext_1)
{



    int3 _S2 = int3(clamp(int2(floor((world_0 - kernelContext_1->grass_0->cover_0.xy) * float2(kernelContext_1->grass_0->cover_0.w)  + float2(0.5f) )), int2(int(0), int(0)), int2(int(kernelContext_1->grass_0->maps_0.z) - int(1), int(kernelContext_1->grass_0->maps_0.w) - int(1))), int(0));

#line 519
    return ((kernelContext_1->grassCover_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)));
}


#line 588
uint grass_unorm8_0(float value_1)
{
    return uint(value_1 * 255.0f + 0.5f);
}


#line 477
float grass_ground_texel_0(int2 texel_0, KernelContext_0 thread* kernelContext_2)
{

    int3 _S3 = int3(clamp(texel_0, int2(int(0), int(0)), int2(int(kernelContext_2->grass_0->maps_0.x) - int(1), int(kernelContext_2->grass_0->maps_0.y) - int(1))), int(0));

#line 480
    return ((kernelContext_2->grassGround_0).read(vec<uint,2>(((_S3)).xy), uint(((_S3)).z)).x);
}


#line 468
struct GrassGround_0
{
    float height_0;
    float3 normal_0;
};


#line 486
GrassGround_0 grass_ground_under_0(float2 world_1, KernelContext_0 thread* kernelContext_3)
{
    float2 at_1 = (world_1 - kernelContext_3->grass_0->ground_0.xy) * float2(kernelContext_3->grass_0->ground_0.w) ;
    float2 base_0 = floor(at_1);
    float2 blend_0 = at_1 - base_0;
    int2 _S4 = int2(base_0);

#line 491
    float _S5 = grass_ground_texel_0(_S4, kernelContext_3);

#line 491
    float _S6 = grass_ground_texel_0(_S4 + int2(int(1), int(0)), kernelContext_3);

#line 491
    float _S7 = grass_ground_texel_0(_S4 + int2(int(0), int(1)), kernelContext_3);

#line 491
    float _S8 = grass_ground_texel_0(_S4 + int2(int(1), int(1)), kernelContext_3);

#line 498
    float _S9 = blend_0.x;

#line 498
    float _S10 = _S6 - _S5;

#line 498
    float lower_0 = _S5 + _S9 * _S10;
    float _S11 = _S8 - _S7;

#line 497
    thread GrassGround_0 under_0;


    (&under_0)->height_0 = lower_0 + blend_0.y * (_S7 + _S9 * _S11 - lower_0);

#line 507
    (&under_0)->normal_0 = normalize(float3(- (0.5f * (_S10 + _S11)), kernelContext_3->grass_0->ground_0.z, - (0.5f * (_S7 - _S5 + (_S8 - _S6)))));
    return under_0;
}


#line 524
uint grass_lattice_key_0(int2 square_0)
{
    return (uint(square_0.x) * 2376512323U) ^ (uint(square_0.y) * 3625334849U);
}



struct GrassClump_0
{
    uint id_0;
    int2 offset_0;
};


#line 544
GrassClump_0 grass_clump_of_0(uint2 global_0, uint jitter_0)
{



    int2 _S12 = int2(global_0) * int2(int(256))  + int2(int((jitter_0 & 65535U) >> 8U), int(((jitter_0 >> 16U) & 65535U) >> 8U));
    uint2 _S13 = global_0 / uint2(8U) ;

#line 550
    int2 _S14 = int2(_S13);
    thread GrassClump_0 nearest_0;
    (&nearest_0)->id_0 = 0U;
    (&nearest_0)->offset_0 = int2(int(0), int(0));

#line 553
    int best_0 = int(2147483647);

#line 553
    int dz_0 = int(-1);

    for(;;)
    {

#line 555
        if(dz_0 <= int(1))
        {
        }
        else
        {

#line 555
            break;
        }

#line 555
        int best_1 = best_0;

#line 555
        int dx_0 = int(-1);

        for(;;)
        {

#line 557
            if(dx_0 <= int(1))
            {
            }
            else
            {

#line 557
                break;
            }
            int2 square_1 = _S14 + int2(dx_0, dz_0);
            uint id_1 = grass_hash_0((grass_lattice_key_0(square_1)) ^ 739982445U);


            int2 offset_1 = _S12 - (square_1 * int2(int(2048))  + int2(int(id_1 & 2047U), int((id_1 >> 16U) & 2047U)));
            int _S15 = offset_1.x;

#line 564
            int _S16 = offset_1.y;

#line 564
            int distance_0 = _S15 * _S15 + _S16 * _S16;
            if(distance_0 < best_1)
            {

                (&nearest_0)->id_0 = id_1;
                (&nearest_0)->offset_0 = offset_1;

#line 569
                best_1 = distance_0;

#line 565
            }

#line 557
            dx_0 = dx_0 + int(1);

#line 557
        }

#line 555
        int dz_1 = dz_0 + int(1);

#line 555
        best_0 = best_1;

#line 555
        dz_0 = dz_1;

#line 555
    }

#line 573
    return nearest_0;
}


#line 291
float windSmoothTriangle_0(float u_0)
{
    float s_0 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}



float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_4)
{


    float2 _S17 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_4->windDirectionLayer_0).sample((kernelContext_4->windSampler_0), (kernelContext_4->wind_0->directionUv_0 + _S17 * kernelContext_4->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_4->windIntensityLayer_0).sample((kernelContext_4->windSampler_0), (kernelContext_4->wind_0->intensityUv_0 + _S17 * kernelContext_4->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;



    float2 base_1 = kernelContext_4->wind_0->baseDirection_0;
    float _S18 = kernelContext_4->wind_0->baseDirection_0.x;

#line 315
    float _S19 = deflection_0.x;

#line 315
    float _S20 = kernelContext_4->wind_0->baseDirection_0.y;

#line 315
    float _S21 = deflection_0.y;

#line 315
    float2 turned_0 = float2(_S18 * _S19 - _S20 * _S21, _S18 * _S21 + _S20 * _S19);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 317
    float2 direction_0;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 319
        direction_0 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 319
    }
    else
    {

#line 319
        direction_0 = base_1;

#line 319
    }

#line 324
    float speed_0 = intensity_0 * kernelContext_4->wind_0->baseSpeed_0 * (1.0f + kernelContext_4->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S17, base_1) * kernelContext_4->wind_0->invGustWavelength_0 + kernelContext_4->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_0.x * speed_0, 0.0f, direction_0.y * speed_0);
}


#line 452
float3 grass_lean_0(float3 velocity_0, float height_1)
{
    float speed_1 = length(velocity_0);
    float bend_0 = height_1 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_0 = velocity_0.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;

#line 463
    float _S22 = bend_0 * bend_0;
    return float3(along_0.x, - (_S22 / (height_1 + sqrt(max(height_1 * height_1 - _S22, 0.0f)))), along_0.y);
}


#line 578
bool grass_kept_far_0(uint2 global_1)
{

    return (((global_1.x) & 1U) + 2U * ((global_1.y) & 1U)) == ((grass_hash_0((grass_lattice_key_0(int2(global_1 >> (uint2(1U) )))) ^ 695872825U)) & 3U);
}


#line 222
struct GrassInstance_0
{
    float4 root_0;
    float4 facing_0;
    float4 lean_0;
    float4 ground_1;
    float4 clump_0;
    uint4 lanes_0;
};


#line 635
[[kernel]] void generateMain(uint3 thread_1 [[thread_position_in_grid]], GrassGenParams_0 constant* grass_2 [[buffer(1)]], atomic<uint> device* drawArgs_2 [[buffer(5)]], GrassTile_0 constant* tile_3 [[buffer(2)]], GrassInstance_natural_0 device* grassCells_2 [[buffer(6)]], texture2d<float, access::sample> grassCover_2 [[texture(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(3)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(0)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(0)]], sampler windSampler_2 [[sampler(0)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(4)]])
{

#line 635
    thread KernelContext_0 kernelContext_5;

#line 635
    (&kernelContext_5)->grass_0 = grass_2;

#line 635
    (&kernelContext_5)->drawArgs_0 = drawArgs_2;

#line 635
    (&kernelContext_5)->tile_1 = tile_3;

#line 635
    (&kernelContext_5)->grassCells_0 = grassCells_2;

#line 635
    (&kernelContext_5)->grassCover_0 = grassCover_2;

#line 635
    (&kernelContext_5)->blades_0 = blades_2;

#line 635
    (&kernelContext_5)->grassGround_0 = grassGround_2;

#line 635
    (&kernelContext_5)->wind_0 = wind_2;

#line 635
    (&kernelContext_5)->windDirectionLayer_0 = windDirectionLayer_2;

#line 635
    (&kernelContext_5)->windSampler_0 = windSampler_2;

#line 635
    (&kernelContext_5)->windIntensityLayer_0 = windIntensityLayer_2;

#line 635
    (&kernelContext_5)->instances_0 = instances_2;

    uint cells_0 = grass_2->limits_0.z;
    uint capacity_0 = grass_2->limits_0.x;

#line 644
    uint _S23 = thread_1.x;

#line 644
    if(_S23 >= (min(cells_0 * cells_0, capacity_0)))
    {
        return;
    }


    uint cell_0 = (&kernelContext_5)->tile_1->slot_0.x * capacity_0 + _S23;



    thread GrassInstance_0 empty_0;
    float4 _S24 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 655
    (&empty_0)->root_0 = _S24;
    (&empty_0)->facing_0 = _S24;
    (&empty_0)->lean_0 = _S24;
    (&empty_0)->ground_1 = _S24;
    (&empty_0)->clump_0 = _S24;
    (&empty_0)->lanes_0 = uint4(0U, 0U, 0U, 0U);

#line 660
    GrassInstance_natural_0 device* _S25 = (&kernelContext_5)->grassCells_0+cell_0;

#line 660
    _S25->root_0 = packed_float4(empty_0.root_0) ;

#line 660
    _S25->facing_0 = packed_float4(empty_0.facing_0) ;

#line 660
    _S25->lean_0 = packed_float4(empty_0.lean_0) ;

#line 660
    _S25->ground_1 = packed_float4(empty_0.ground_1) ;

#line 660
    _S25->clump_0 = packed_float4(empty_0.clump_0) ;

#line 660
    _S25->lanes_0 = packed_uint4(empty_0.lanes_0) ;

    float side_0 = (&kernelContext_5)->tile_1->tile_0.z / float(cells_0);
    uint jitter_lane_0 = grass_hash_0(cell_0);
    float2 jitter_1 = grass_unit_pair_0(jitter_lane_0);
    float2 _S26 = (&kernelContext_5)->tile_1->tile_0.xy;
    uint _S27 = _S23 % cells_0;

#line 666
    float _S28 = float(_S27);

#line 666
    uint _S29 = _S23 / cells_0;

#line 666
    float2 world_2 = _S26 + (float2(_S28, float(_S29)) + jitter_1) * float2(side_0) ;

#line 666
    float4 _S30 = grass_cover_under_0(world_2, &kernelContext_5);

#line 673
    if(((grass_hash_0(cell_0 ^ 2654435769U)) & 65535U) >= (grass_unorm8_0(_S30.x) * 257U))
    {
        return;
    }

    uint _S31 = min(grass_unorm8_0(_S30.y), max(grass_2->limits_0.y, 1U) - 1U);

#line 678
    GrassBlade_natural_0 device* _S32 = (&kernelContext_5)->blades_0+_S31;

#line 678
    GrassGround_0 _S33 = grass_ground_under_0(world_2, &kernelContext_5);



    float3 root_1 = float3(world_2.x, _S33.height_0, world_2.y);



    float3 relative_0 = root_1 - (&kernelContext_5)->grass_0->camera_0.xyz;
    float _S34 = dot(relative_0, relative_0);

#line 687
    if(_S34 > ((&kernelContext_5)->grass_0->camera_0.w * (&kernelContext_5)->grass_0->camera_0.w))
    {
        return;
    }



    uint _S35 = (&kernelContext_5)->tile_1->slot_0.y * cells_0 + _S27;

#line 694
    uint _S36 = (&kernelContext_5)->tile_1->slot_0.z * cells_0;

#line 694
    uint _S37 = _S23 / cells_0;

#line 694
    uint2 global_2 = uint2(_S35, _S36 + _S37);
    GrassClump_0 clump_2 = grass_clump_of_0(global_2, jitter_lane_0);

#line 701
    float2 spread_0 = grass_unit_pair_0(grass_hash_0(cell_0 ^ 3266489909U));

#line 701
    float4 _S38 = float4(_S32->size_0) ;

#line 701
    float4 _S39 = float4(_S32->clump_1) ;


    float height_2 = _S38.x * (1.0f - _S38.z * spread_0.x) * (1.0f - _S39.y * grass_unit_pair_0(grass_hash_0((clump_2.id_0) ^ 3266489909U)).x);
    float half_width_0 = _S38.y * (1.0f - _S38.w * spread_0.y);

#line 705
    float2 _S40 = float2(2.0f) ;

#line 705
    float2 _S41 = float2(1.0f) ;

#line 717
    float2 own_0 = grass_unit_pair_0(grass_hash_0(cell_0 ^ 2246822507U)) * _S40 - _S41;

    float2 square_2 = own_0 + (grass_unit_pair_0(grass_hash_0((clump_2.id_0) ^ 2246822507U)) * _S40 - _S41 - own_0) * float2(_S39.x) ;
    float square_length_0 = dot(square_2, square_2);

#line 720
    float2 facing_1;
    if(square_length_0 > 9.99999993922529029e-09f)
    {

#line 721
        facing_1 = square_2 / float2(sqrt(square_length_0)) ;

#line 721
    }
    else
    {

#line 721
        facing_1 = float2(1.0f, 0.0f);

#line 721
    }

#line 721
    float3 _S42 = windSample_0(relative_0, &kernelContext_5);



    float3 lean_1 = grass_lean_0(_S42, height_2);

    thread GrassInstance_0 instance_0;
    (&instance_0)->root_0 = float4(root_1, height_2);
    (&instance_0)->facing_0 = float4(facing_1, half_width_0, grass_unit_pair_0(grass_hash_0(cell_0 ^ 668265263U)).x);

    (&instance_0)->lean_0 = float4(lean_1, 0.0f);
    (&instance_0)->ground_1 = float4(_S33.normal_0, 0.0f);


    (&instance_0)->clump_0 = float4(float2(clump_2.offset_0) * float2((side_0 * 0.00390625f)) , 0.0f, 0.0f);
    bool kept_0 = grass_kept_far_0(global_2);

#line 736
    uint _S43;
    if(kept_0)
    {

#line 737
        _S43 = 1U;

#line 737
    }
    else
    {

#line 737
        _S43 = 0U;

#line 737
    }

#line 737
    (&instance_0)->lanes_0 = uint4(cell_0, _S31, clump_2.id_0, _S43);

#line 737
    GrassInstance_natural_0 device* _S44 = (&kernelContext_5)->grassCells_0+cell_0;

#line 737
    _S44->root_0 = packed_float4(instance_0.root_0) ;

#line 737
    _S44->facing_0 = packed_float4(instance_0.facing_0) ;

#line 737
    _S44->lean_0 = packed_float4(instance_0.lean_0) ;

#line 737
    _S44->ground_1 = packed_float4(instance_0.ground_1) ;

#line 737
    _S44->clump_0 = packed_float4(instance_0.clump_0) ;

#line 737
    _S44->lanes_0 = packed_uint4(instance_0.lanes_0) ;



    uint slot_args_0 = (&kernelContext_5)->tile_1->slot_0.x * 5U;
    uint _S45 = (uint4(_S32->flags_0) ).x;

#line 742
    if(_S45 == 1U)
    {

#line 748
        atomic_store_explicit((&kernelContext_5)->drawArgs_0+((slot_args_0 + 1U) * 4U + 1U), (&kernelContext_5)->grass_0->looks_0.x, memory_order_relaxed);
        atomic_store_explicit((&kernelContext_5)->drawArgs_0+((slot_args_0 + 2U) * 4U + 1U), (&kernelContext_5)->grass_0->looks_0.y, memory_order_relaxed);
        return;
    }

    if(_S45 == 2U)
    {

#line 759
        if(_S34 < ((&kernelContext_5)->grass_0->lod_0.x * (&kernelContext_5)->grass_0->lod_0.x))
        {

            uint near_at_0 = atomic_fetch_add_explicit((&kernelContext_5)->drawArgs_0+((slot_args_0 + 3U) * 4U + 1U), 1U, memory_order_relaxed);

#line 762
            GrassInstance_natural_0 device* _S46 = (&kernelContext_5)->instances_0+((grass_2->limits_0.w + (&kernelContext_5)->tile_1->slot_0.x) * capacity_0 + near_at_0);

#line 762
            _S46->root_0 = packed_float4(instance_0.root_0) ;

#line 762
            _S46->facing_0 = packed_float4(instance_0.facing_0) ;

#line 762
            _S46->lean_0 = packed_float4(instance_0.lean_0) ;

#line 762
            _S46->ground_1 = packed_float4(instance_0.ground_1) ;

#line 762
            _S46->clump_0 = packed_float4(instance_0.clump_0) ;

#line 762
            _S46->lanes_0 = packed_uint4(instance_0.lanes_0) ;

#line 759
        }
        else
        {



            if(kept_0)
            {

                uint far_at_0 = atomic_fetch_add_explicit((&kernelContext_5)->drawArgs_0+((slot_args_0 + 4U) * 4U + 1U), 1U, memory_order_relaxed);

#line 768
                GrassInstance_natural_0 device* _S47 = (&kernelContext_5)->instances_0+((&kernelContext_5)->tile_1->slot_0.x * capacity_0 + capacity_0 - 1U - far_at_0);

#line 768
                _S47->root_0 = packed_float4(instance_0.root_0) ;

#line 768
                _S47->facing_0 = packed_float4(instance_0.facing_0) ;

#line 768
                _S47->lean_0 = packed_float4(instance_0.lean_0) ;

#line 768
                _S47->ground_1 = packed_float4(instance_0.ground_1) ;

#line 768
                _S47->clump_0 = packed_float4(instance_0.clump_0) ;

#line 768
                _S47->lanes_0 = packed_uint4(instance_0.lanes_0) ;

#line 765
            }

#line 759
        }

#line 771
        return;
    }

#line 778
    uint at_2 = atomic_fetch_add_explicit((&kernelContext_5)->drawArgs_0+(slot_args_0 * 4U + 1U), 1U, memory_order_relaxed);

#line 778
    GrassInstance_natural_0 device* _S48 = (&kernelContext_5)->instances_0+((&kernelContext_5)->tile_1->slot_0.x * capacity_0 + at_2);

#line 778
    _S48->root_0 = packed_float4(instance_0.root_0) ;

#line 778
    _S48->facing_0 = packed_float4(instance_0.facing_0) ;

#line 778
    _S48->lean_0 = packed_float4(instance_0.lean_0) ;

#line 778
    _S48->ground_1 = packed_float4(instance_0.ground_1) ;

#line 778
    _S48->clump_0 = packed_float4(instance_0.clump_0) ;

#line 778
    _S48->lanes_0 = packed_uint4(instance_0.lanes_0) ;

    return;
}

