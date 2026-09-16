#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 89 "shaders/grass_gen.slang"
struct GrassGenParams_0
{
    float4 camera_0;
    float4 ground_0;
    float4 cover_0;
    uint4 maps_0;
    uint4 limits_0;
};


#line 117
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 117
struct GrassBlade_natural_0
{
    packed_float4 root_color_0;
    packed_float4 tip_color_0;
    packed_float4 size_0;
};


#line 44
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


#line 44
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_1;
    packed_uint4 lanes_0;
};


#line 44
struct KernelContext_0
{
    GrassGenParams_0 constant* grass_0;
    atomic<uint> device* drawArgs_0;
    GrassTile_0 constant* tile_1;
    texture2d<float, access::sample> grassCover_0;
    GrassBlade_natural_0 device* blades_0;
    texture2d<float, access::sample> grassGround_0;
    WindParams_0 constant* wind_0;
    texture2d<float, access::sample> windDirectionLayer_0;
    sampler windSampler_0;
    texture2d<float, access::sample> windIntensityLayer_0;
    GrassInstance_natural_0 device* instances_0;
};


#line 382
[[kernel]] void clearMain(uint3 thread_0 [[thread_position_in_grid]], GrassGenParams_0 constant* grass_1 [[buffer(1)]], atomic<uint> device* drawArgs_1 [[buffer(5)]], GrassTile_0 constant* tile_2 [[buffer(2)]], texture2d<float, access::sample> grassCover_1 [[texture(3)]], GrassBlade_natural_0 device* blades_1 [[buffer(3)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(0)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(0)]], sampler windSampler_1 [[sampler(0)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(4)]])
{

#line 382
    thread KernelContext_0 kernelContext_0;

#line 382
    (&kernelContext_0)->grass_0 = grass_1;

#line 382
    (&kernelContext_0)->drawArgs_0 = drawArgs_1;

#line 382
    (&kernelContext_0)->tile_1 = tile_2;

#line 382
    (&kernelContext_0)->grassCover_0 = grassCover_1;

#line 382
    (&kernelContext_0)->blades_0 = blades_1;

#line 382
    (&kernelContext_0)->grassGround_0 = grassGround_1;

#line 382
    (&kernelContext_0)->wind_0 = wind_1;

#line 382
    (&kernelContext_0)->windDirectionLayer_0 = windDirectionLayer_1;

#line 382
    (&kernelContext_0)->windSampler_0 = windSampler_1;

#line 382
    (&kernelContext_0)->windIntensityLayer_0 = windIntensityLayer_1;

#line 382
    (&kernelContext_0)->instances_0 = instances_1;

    uint slot_1 = thread_0.x;
    if(slot_1 >= (grass_1->limits_0.w))
    {
        return;
    }
    uint at_0 = slot_1 * 4U;
    atomic_store_explicit((&kernelContext_0)->drawArgs_0+at_0, 12U, memory_order_relaxed);
    atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 1U), 0U, memory_order_relaxed);
    atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 2U), 0U, memory_order_relaxed);



    atomic_store_explicit((&kernelContext_0)->drawArgs_0+(at_0 + 3U), 0U, memory_order_relaxed);
    return;
}


#line 291
uint grass_hash_0(uint value_0)
{
    uint state_0 = value_0 * 747796405U + 2891336453U;
    uint word_0 = ((state_0 >> ((state_0 >> 28U) + 4U)) ^ state_0) * 277803737U;
    return (word_0 >> 22U) ^ word_0;
}


#line 304
float2 grass_unit_pair_0(uint lane_0)
{
    return float2(float(lane_0 & 65535U), float((lane_0 >> 16U) & 65535U)) * float2(0.0000152587890625f) ;
}


#line 356
float4 grass_cover_under_0(float2 world_0, KernelContext_0 thread* kernelContext_1)
{



    int3 _S1 = int3(clamp(int2(floor((world_0 - kernelContext_1->grass_0->cover_0.xy) * float2(kernelContext_1->grass_0->cover_0.w)  + float2(0.5f) )), int2(int(0), int(0)), int2(int(kernelContext_1->grass_0->maps_0.z) - int(1), int(kernelContext_1->grass_0->maps_0.w) - int(1))), int(0));

#line 361
    return ((kernelContext_1->grassCover_0).read(vec<uint,2>(((_S1)).xy), uint(((_S1)).z)));
}


#line 368
uint grass_unorm8_0(float value_1)
{
    return uint(value_1 * 255.0f + 0.5f);
}


#line 319
float grass_ground_texel_0(int2 texel_0, KernelContext_0 thread* kernelContext_2)
{

    int3 _S2 = int3(clamp(texel_0, int2(int(0), int(0)), int2(int(kernelContext_2->grass_0->maps_0.x) - int(1), int(kernelContext_2->grass_0->maps_0.y) - int(1))), int(0));

#line 322
    return ((kernelContext_2->grassGround_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)).x);
}


#line 310
struct GrassGround_0
{
    float height_0;
    float3 normal_0;
};


#line 328
GrassGround_0 grass_ground_under_0(float2 world_1, KernelContext_0 thread* kernelContext_3)
{
    float2 at_1 = (world_1 - kernelContext_3->grass_0->ground_0.xy) * float2(kernelContext_3->grass_0->ground_0.w) ;
    float2 base_0 = floor(at_1);
    float2 blend_0 = at_1 - base_0;
    int2 _S3 = int2(base_0);

#line 333
    float _S4 = grass_ground_texel_0(_S3, kernelContext_3);

#line 333
    float _S5 = grass_ground_texel_0(_S3 + int2(int(1), int(0)), kernelContext_3);

#line 333
    float _S6 = grass_ground_texel_0(_S3 + int2(int(0), int(1)), kernelContext_3);

#line 333
    float _S7 = grass_ground_texel_0(_S3 + int2(int(1), int(1)), kernelContext_3);

#line 340
    float _S8 = blend_0.x;

#line 340
    float _S9 = _S5 - _S4;

#line 340
    float lower_0 = _S4 + _S8 * _S9;
    float _S10 = _S7 - _S6;

#line 339
    thread GrassGround_0 under_0;


    (&under_0)->height_0 = lower_0 + blend_0.y * (_S6 + _S8 * _S10 - lower_0);

#line 349
    (&under_0)->normal_0 = normalize(float3(- (0.5f * (_S9 + _S10)), kernelContext_3->grass_0->ground_0.z, - (0.5f * (_S6 - _S4 + (_S7 - _S5)))));
    return under_0;
}


#line 207
float windSmoothTriangle_0(float u_0)
{
    float s_0 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}



float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_4)
{


    float2 _S11 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_4->windDirectionLayer_0).sample((kernelContext_4->windSampler_0), (kernelContext_4->wind_0->directionUv_0 + _S11 * kernelContext_4->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_4->windIntensityLayer_0).sample((kernelContext_4->windSampler_0), (kernelContext_4->wind_0->intensityUv_0 + _S11 * kernelContext_4->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;



    float2 base_1 = kernelContext_4->wind_0->baseDirection_0;
    float _S12 = kernelContext_4->wind_0->baseDirection_0.x;

#line 231
    float _S13 = deflection_0.x;

#line 231
    float _S14 = kernelContext_4->wind_0->baseDirection_0.y;

#line 231
    float _S15 = deflection_0.y;

#line 231
    float2 turned_0 = float2(_S12 * _S13 - _S14 * _S15, _S12 * _S15 + _S14 * _S13);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 233
    float2 direction_0;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 235
        direction_0 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 235
    }
    else
    {

#line 235
        direction_0 = base_1;

#line 235
    }

#line 240
    float speed_0 = intensity_0 * kernelContext_4->wind_0->baseSpeed_0 * (1.0f + kernelContext_4->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S11, base_1) * kernelContext_4->wind_0->invGustWavelength_0 + kernelContext_4->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_0.x * speed_0, 0.0f, direction_0.y * speed_0);
}


#line 150
struct GrassInstance_0
{
    float4 root_0;
    float4 facing_0;
    float4 lean_0;
    float4 ground_1;
    uint4 lanes_0;
};


#line 402
[[kernel]] void generateMain(uint3 thread_1 [[thread_position_in_grid]], GrassGenParams_0 constant* grass_2 [[buffer(1)]], atomic<uint> device* drawArgs_2 [[buffer(5)]], GrassTile_0 constant* tile_3 [[buffer(2)]], texture2d<float, access::sample> grassCover_2 [[texture(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(3)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(0)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(0)]], sampler windSampler_2 [[sampler(0)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(4)]])
{

#line 402
    thread KernelContext_0 kernelContext_5;

#line 402
    (&kernelContext_5)->grass_0 = grass_2;

#line 402
    (&kernelContext_5)->drawArgs_0 = drawArgs_2;

#line 402
    (&kernelContext_5)->tile_1 = tile_3;

#line 402
    (&kernelContext_5)->grassCover_0 = grassCover_2;

#line 402
    (&kernelContext_5)->blades_0 = blades_2;

#line 402
    (&kernelContext_5)->grassGround_0 = grassGround_2;

#line 402
    (&kernelContext_5)->wind_0 = wind_2;

#line 402
    (&kernelContext_5)->windDirectionLayer_0 = windDirectionLayer_2;

#line 402
    (&kernelContext_5)->windSampler_0 = windSampler_2;

#line 402
    (&kernelContext_5)->windIntensityLayer_0 = windIntensityLayer_2;

#line 402
    (&kernelContext_5)->instances_0 = instances_2;

    uint cells_0 = grass_2->limits_0.z;
    uint capacity_0 = grass_2->limits_0.x;

#line 411
    uint _S16 = thread_1.x;

#line 411
    if(_S16 >= (min(cells_0 * cells_0, capacity_0)))
    {
        return;
    }


    uint cell_0 = (&kernelContext_5)->tile_1->slot_0.x * capacity_0 + _S16;
    float side_0 = (&kernelContext_5)->tile_1->tile_0.z / float(cells_0);
    float2 jitter_0 = grass_unit_pair_0(grass_hash_0(cell_0));
    float2 _S17 = (&kernelContext_5)->tile_1->tile_0.xy;
    uint _S18 = _S16 % cells_0;

#line 421
    float _S19 = float(_S18);

#line 421
    uint _S20 = _S16 / cells_0;

#line 421
    float2 world_2 = _S17 + (float2(_S19, float(_S20)) + jitter_0) * float2(side_0) ;

#line 421
    float4 _S21 = grass_cover_under_0(world_2, &kernelContext_5);

#line 428
    if(((grass_hash_0(cell_0 ^ 2654435769U)) & 65535U) >= (grass_unorm8_0(_S21.x) * 257U))
    {
        return;
    }

    uint _S22 = min(grass_unorm8_0(_S21.y), max(grass_2->limits_0.y, 1U) - 1U);
    GrassBlade_natural_0 blade_0 = (&kernelContext_5)->blades_0[_S22];

#line 434
    GrassGround_0 _S23 = grass_ground_under_0(world_2, &kernelContext_5);


    float3 root_1 = float3(world_2.x, _S23.height_0, world_2.y);



    float3 relative_0 = root_1 - (&kernelContext_5)->grass_0->camera_0.xyz;
    if((dot(relative_0, relative_0)) > ((&kernelContext_5)->grass_0->camera_0.w * (&kernelContext_5)->grass_0->camera_0.w))
    {
        return;
    }



    float2 spread_0 = grass_unit_pair_0(grass_hash_0(cell_0 ^ 3266489909U));

#line 449
    float4 _S24 = float4(blade_0.size_0) ;
    float height_1 = _S24.x * (1.0f - _S24.z * spread_0.x);
    float half_width_0 = _S24.y * (1.0f - _S24.w * spread_0.y);

#line 458
    float2 square_0 = grass_unit_pair_0(grass_hash_0(cell_0 ^ 2246822507U)) * float2(2.0f)  - float2(1.0f) ;
    float square_length_0 = dot(square_0, square_0);

#line 459
    float2 facing_1;
    if(square_length_0 > 9.99999993922529029e-09f)
    {

#line 460
        facing_1 = square_0 / float2(sqrt(square_length_0)) ;

#line 460
    }
    else
    {

#line 460
        facing_1 = float2(1.0f, 0.0f);

#line 460
    }

#line 460
    float3 _S25 = windSample_0(relative_0, &kernelContext_5);

#line 467
    float speed_1 = length(_S25);
    float bend_0 = height_1 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_0 = _S25.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;

#line 476
    float _S26 = bend_0 * bend_0;

#line 476
    float drop_0 = _S26 / (height_1 + sqrt(max(height_1 * height_1 - _S26, 0.0f)));

    thread GrassInstance_0 instance_0;
    (&instance_0)->root_0 = float4(root_1, height_1);
    (&instance_0)->facing_0 = float4(facing_1, half_width_0, grass_unit_pair_0(grass_hash_0(cell_0 ^ 668265263U)).x);

    (&instance_0)->lean_0 = float4(along_0.x, - drop_0, along_0.y, 0.0f);
    (&instance_0)->ground_1 = float4(_S23.normal_0, 0.0f);
    (&instance_0)->lanes_0 = uint4(cell_0, _S22, 0U, 0U);

#line 490
    uint at_2 = atomic_fetch_add_explicit((&kernelContext_5)->drawArgs_0+((&kernelContext_5)->tile_1->slot_0.x * 4U + 1U), 1U, memory_order_relaxed);

#line 490
    GrassInstance_natural_0 device* _S27 = (&kernelContext_5)->instances_0+((&kernelContext_5)->tile_1->slot_0.x * capacity_0 + at_2);

#line 490
    _S27->root_0 = packed_float4(instance_0.root_0) ;

#line 490
    _S27->facing_0 = packed_float4(instance_0.facing_0) ;

#line 490
    _S27->lean_0 = packed_float4(instance_0.lean_0) ;

#line 490
    _S27->ground_1 = packed_float4(instance_0.ground_1) ;

#line 490
    _S27->lanes_0 = packed_uint4(instance_0.lanes_0) ;

    return;
}

