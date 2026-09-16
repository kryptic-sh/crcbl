struct GrassGenParams_std140_0
{
    @align(16) camera_0 : vec4<f32>,
    @align(16) ground_0 : vec4<f32>,
    @align(16) cover_0 : vec4<f32>,
    @align(16) maps_0 : vec4<u32>,
    @align(16) limits_0 : vec4<u32>,
};

@binding(4) @group(0) var<uniform> grass_0 : GrassGenParams_std140_0;
@binding(10) @group(0) var<storage, read_write> drawArgs_0 : array<atomic<u32>>;

struct GrassTile_std140_0
{
    @align(16) tile_0 : vec4<f32>,
    @align(16) slot_0 : vec4<u32>,
};

@binding(5) @group(0) var<uniform> tile_1 : GrassTile_std140_0;
@binding(8) @group(0) var grassCover_0 : texture_2d<f32>;

struct GrassBlade_std430_0
{
    @align(16) root_color_0 : vec4<f32>,
    @align(16) tip_color_0 : vec4<f32>,
    @align(16) size_0 : vec4<f32>,
};

@binding(6) @group(0) var<storage, read> blades_0 : array<GrassBlade_std430_0>;

@binding(7) @group(0) var grassGround_0 : texture_2d<f32>;

struct WindParams_std140_0
{
    @align(16) baseDirection_0 : vec2<f32>,
    @align(8) baseSpeed_0 : f32,
    @align(4) gustAmplitude_0 : f32,
    @align(16) gustPhase_0 : f32,
    @align(4) invGustWavelength_0 : f32,
    @align(8) pad0_0 : vec2<f32>,
    @align(16) directionUv_0 : vec2<f32>,
    @align(8) directionUvPerMetre_0 : vec2<f32>,
    @align(16) intensityUv_0 : vec2<f32>,
    @align(8) intensityUvPerMetre_0 : vec2<f32>,
};

@binding(0) @group(0) var<uniform> wind_0 : WindParams_std140_0;
@binding(1) @group(0) var windDirectionLayer_0 : texture_2d<f32>;

@binding(3) @group(0) var windSampler_0 : sampler;

@binding(2) @group(0) var windIntensityLayer_0 : texture_2d<f32>;

struct GrassInstance_std430_0
{
    @align(16) root_0 : vec4<f32>,
    @align(16) facing_0 : vec4<f32>,
    @align(16) lean_0 : vec4<f32>,
    @align(16) ground_1 : vec4<f32>,
    @align(16) lanes_0 : vec4<u32>,
};

@binding(9) @group(0) var<storage, read_write> instances_0 : array<GrassInstance_std430_0>;

@compute
@workgroup_size(64, 1, 1)
fn clearMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var slot_1 : u32 = thread_0.x;
    if(slot_1 >= (grass_0.limits_0.w))
    {
        return;
    }
    var at_0 : u32 = slot_1 * u32(4);
    atomicStore(&(drawArgs_0[at_0]), u32(12));
    atomicStore(&(drawArgs_0[at_0 + u32(1)]), u32(0));
    atomicStore(&(drawArgs_0[at_0 + u32(2)]), u32(0));
    atomicStore(&(drawArgs_0[at_0 + u32(3)]), u32(0));
    return;
}

fn grass_hash_0( value_0 : u32) -> u32
{
    var state_0 : u32 = value_0 * u32(747796405) + u32(2891336453);
    var word_0 : u32 = ((((state_0 >> ((((state_0 >> (u32(28)))) + u32(4))))) ^ (state_0))) * u32(277803737);
    return (((word_0 >> (u32(22)))) ^ (word_0));
}

fn grass_unit_pair_0( lane_0 : u32) -> vec2<f32>
{
    return vec2<f32>(f32((lane_0 & (u32(65535)))), f32((((lane_0 >> (u32(16)))) & (u32(65535))))) * vec2<f32>(0.0000152587890625f);
}

fn grass_cover_under_0( world_0 : vec2<f32>) -> vec4<f32>
{
    var _S1 : vec3<i32> = vec3<i32>(clamp(vec2<i32>(floor((world_0 - grass_0.cover_0.xy) * vec2<f32>(grass_0.cover_0.w) + vec2<f32>(0.5f))), vec2<i32>(i32(0), i32(0)), vec2<i32>(i32(grass_0.maps_0.z) - i32(1), i32(grass_0.maps_0.w) - i32(1))), i32(0));
    return (textureLoad((grassCover_0), ((_S1)).xy, ((_S1)).z));
}

fn grass_unorm8_0( value_1 : f32) -> u32
{
    return u32(value_1 * 255.0f + 0.5f);
}

fn grass_ground_texel_0( texel_0 : vec2<i32>) -> f32
{
    var _S2 : vec3<i32> = vec3<i32>(clamp(texel_0, vec2<i32>(i32(0), i32(0)), vec2<i32>(i32(grass_0.maps_0.x) - i32(1), i32(grass_0.maps_0.y) - i32(1))), i32(0));
    return (textureLoad((grassGround_0), ((_S2)).xy, ((_S2)).z).x);
}

struct GrassGround_0
{
     height_0 : f32,
     normal_0 : vec3<f32>,
};

fn grass_ground_under_0( world_1 : vec2<f32>) -> GrassGround_0
{
    var at_1 : vec2<f32> = (world_1 - grass_0.ground_0.xy) * vec2<f32>(grass_0.ground_0.w);
    var base_0 : vec2<f32> = floor(at_1);
    var blend_0 : vec2<f32> = at_1 - base_0;
    var _S3 : vec2<i32> = vec2<i32>(base_0);
    var h00_0 : f32 = grass_ground_texel_0(_S3);
    var h10_0 : f32 = grass_ground_texel_0(_S3 + vec2<i32>(i32(1), i32(0)));
    var h01_0 : f32 = grass_ground_texel_0(_S3 + vec2<i32>(i32(0), i32(1)));
    var h11_0 : f32 = grass_ground_texel_0(_S3 + vec2<i32>(i32(1), i32(1)));
    var _S4 : f32 = blend_0.x;
    var _S5 : f32 = h10_0 - h00_0;
    var lower_0 : f32 = h00_0 + _S4 * _S5;
    var _S6 : f32 = h11_0 - h01_0;
    var under_0 : GrassGround_0;
    under_0.height_0 = lower_0 + blend_0.y * (h01_0 + _S4 * _S6 - lower_0);
    under_0.normal_0 = normalize(vec3<f32>(- (0.5f * (_S5 + _S6)), grass_0.ground_0.z, - (0.5f * (h01_0 - h00_0 + (h11_0 - h10_0)))));
    return under_0;
}

fn windSmoothTriangle_0( u_0 : f32) -> f32
{
    var s_0 : f32 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}

fn windSample_0( posRel_0 : vec3<f32>) -> vec3<f32>
{
    var _S7 : vec2<f32> = posRel_0.xz;
    var deflection_0 : vec2<f32> = (textureSampleLevel((windDirectionLayer_0), (windSampler_0), (wind_0.directionUv_0 + _S7 * wind_0.directionUvPerMetre_0), (0.0f))).xy * vec2<f32>(2.0f) - vec2<f32>(1.0f);
    var intensity_0 : f32 = (textureSampleLevel((windIntensityLayer_0), (windSampler_0), (wind_0.intensityUv_0 + _S7 * wind_0.intensityUvPerMetre_0), (0.0f))).x;
    var base_1 : vec2<f32> = wind_0.baseDirection_0;
    var _S8 : f32 = wind_0.baseDirection_0.x;
    var _S9 : f32 = deflection_0.x;
    var _S10 : f32 = wind_0.baseDirection_0.y;
    var _S11 : f32 = deflection_0.y;
    var turned_0 : vec2<f32> = vec2<f32>(_S8 * _S9 - _S10 * _S11, _S8 * _S11 + _S10 * _S9);
    var lengthSquared_0 : f32 = dot(turned_0, turned_0);
    var direction_0 : vec2<f32>;
    if(lengthSquared_0 > 9.999999960041972e-13f)
    {
        direction_0 = turned_0 / vec2<f32>(sqrt(lengthSquared_0));
    }
    else
    {
        direction_0 = base_1;
    }
    var speed_0 : f32 = intensity_0 * wind_0.baseSpeed_0 * (1.0f + wind_0.gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S7, base_1) * wind_0.invGustWavelength_0 + wind_0.gustPhase_0) - 1.0f));
    return vec3<f32>(direction_0.x * speed_0, 0.0f, direction_0.y * speed_0);
}

struct GrassInstance_0
{
     root_0 : vec4<f32>,
     facing_0 : vec4<f32>,
     lean_0 : vec4<f32>,
     ground_1 : vec4<f32>,
     lanes_0 : vec4<u32>,
};

@compute
@workgroup_size(64, 1, 1)
fn generateMain(@builtin(global_invocation_id) thread_1 : vec3<u32>)
{
    var cells_0 : u32 = grass_0.limits_0.z;
    var capacity_0 : u32 = grass_0.limits_0.x;
    var _S12 : u32 = thread_1.x;
    if(_S12 >= (min(cells_0 * cells_0, capacity_0)))
    {
        return;
    }
    var cell_0 : u32 = tile_1.slot_0.x * capacity_0 + _S12;
    var side_0 : f32 = tile_1.tile_0.z / f32(cells_0);
    var jitter_0 : vec2<f32> = grass_unit_pair_0(grass_hash_0(cell_0));
    var _S13 : vec2<f32> = tile_1.tile_0.xy;
    var _S14 : u32 = _S12 % cells_0;
    var _S15 : f32 = f32(_S14);
    var _S16 : u32 = _S12 / cells_0;
    var world_2 : vec2<f32> = _S13 + (vec2<f32>(_S15, f32(_S16)) + jitter_0) * vec2<f32>(side_0);
    var cover_1 : vec4<f32> = grass_cover_under_0(world_2);
    if((((grass_hash_0((cell_0 ^ (u32(2654435769))))) & (u32(65535)))) >= (grass_unorm8_0(cover_1.x) * u32(257)))
    {
        return;
    }
    var _S17 : u32 = min(grass_unorm8_0(cover_1.y), max(grass_0.limits_0.y, u32(1)) - u32(1));
    var blade_0 : GrassBlade_std430_0 = blades_0[_S17];
    var under_1 : GrassGround_0 = grass_ground_under_0(world_2);
    var root_1 : vec3<f32> = vec3<f32>(world_2.x, under_1.height_0, world_2.y);
    var relative_0 : vec3<f32> = root_1 - grass_0.camera_0.xyz;
    if((dot(relative_0, relative_0)) > (grass_0.camera_0.w * grass_0.camera_0.w))
    {
        return;
    }
    var spread_0 : vec2<f32> = grass_unit_pair_0(grass_hash_0((cell_0 ^ (u32(3266489909)))));
    var height_1 : f32 = blade_0.size_0.x * (1.0f - blade_0.size_0.z * spread_0.x);
    var half_width_0 : f32 = blade_0.size_0.y * (1.0f - blade_0.size_0.w * spread_0.y);
    var square_0 : vec2<f32> = grass_unit_pair_0(grass_hash_0((cell_0 ^ (u32(2246822507))))) * vec2<f32>(2.0f) - vec2<f32>(1.0f);
    var square_length_0 : f32 = dot(square_0, square_0);
    var facing_1 : vec2<f32>;
    if(square_length_0 > 9.99999993922529029e-09f)
    {
        facing_1 = square_0 / vec2<f32>(sqrt(square_length_0));
    }
    else
    {
        facing_1 = vec2<f32>(1.0f, 0.0f);
    }
    var velocity_0 : vec3<f32> = windSample_0(relative_0);
    var speed_1 : f32 = length(velocity_0);
    var bend_0 : f32 = height_1 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    var along_0 : vec2<f32> = velocity_0.xz * vec2<f32>((bend_0 / max(speed_1, 9.99999997475242708e-07f)));
    var _S18 : f32 = bend_0 * bend_0;
    var drop_0 : f32 = _S18 / (height_1 + sqrt(max(height_1 * height_1 - _S18, 0.0f)));
    var instance_0 : GrassInstance_0;
    instance_0.root_0 = vec4<f32>(root_1, height_1);
    instance_0.facing_0 = vec4<f32>(facing_1, half_width_0, grass_unit_pair_0(grass_hash_0((cell_0 ^ (u32(668265263))))).x);
    instance_0.lean_0 = vec4<f32>(along_0.x, - drop_0, along_0.y, 0.0f);
    instance_0.ground_1 = vec4<f32>(under_1.normal_0, 0.0f);
    instance_0.lanes_0 = vec4<u32>(cell_0, _S17, u32(0), u32(0));
    var at_2 : u32 = atomicAdd(&(drawArgs_0[tile_1.slot_0.x * u32(4) + u32(1)]), u32(1));
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].root_0 = instance_0.root_0;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].facing_0 = instance_0.facing_0;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].lean_0 = instance_0.lean_0;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].ground_1 = instance_0.ground_1;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].lanes_0 = instance_0.lanes_0;
    return;
}

