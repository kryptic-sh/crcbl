struct GrassGenParams_std140_0
{
    @align(16) camera_0 : vec4<f32>,
    @align(16) ground_0 : vec4<f32>,
    @align(16) cover_0 : vec4<f32>,
    @align(16) maps_0 : vec4<u32>,
    @align(16) limits_0 : vec4<u32>,
    @align(16) looks_0 : vec4<u32>,
    @align(16) lod_0 : vec4<f32>,
};

@binding(4) @group(0) var<uniform> grass_0 : GrassGenParams_std140_0;
@binding(10) @group(0) var<storage, read_write> drawArgs_0 : array<atomic<u32>>;

struct GrassTile_std140_0
{
    @align(16) tile_0 : vec4<f32>,
    @align(16) slot_0 : vec4<u32>,
};

@binding(5) @group(0) var<uniform> tile_1 : GrassTile_std140_0;
struct GrassInstance_std430_0
{
    @align(16) root_0 : vec4<f32>,
    @align(16) facing_0 : vec4<f32>,
    @align(16) lean_0 : vec4<f32>,
    @align(16) ground_1 : vec4<f32>,
    @align(16) clump_0 : vec4<f32>,
    @align(16) lanes_0 : vec4<u32>,
};

@binding(11) @group(0) var<storage, read_write> grassCells_0 : array<GrassInstance_std430_0>;

@binding(8) @group(0) var grassCover_0 : texture_2d<f32>;

struct GrassBlade_std430_0
{
    @align(16) root_color_0 : vec4<f32>,
    @align(16) tip_color_0 : vec4<f32>,
    @align(16) size_0 : vec4<f32>,
    @align(16) occlusion_0 : vec4<f32>,
    @align(16) glow_0 : vec4<f32>,
    @align(16) patch_0 : vec4<f32>,
    @align(16) shape_0 : vec4<f32>,
    @align(16) clump_1 : vec4<f32>,
    @align(16) flags_0 : vec4<u32>,
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
    var _S1 : u32 = u32(2) * (grass_0.limits_0.z / u32(4)) * u32(16) * u32(4) * u32(6);
    var draw_0 : u32 = u32(0);
    for(;;)
    {
        if(draw_0 < u32(5))
        {
        }
        else
        {
            break;
        }
        var at_0 : u32 = (slot_1 * u32(5) + draw_0) * u32(4);
        var vertices_0 : u32;
        if(draw_0 == u32(1))
        {
            vertices_0 = u32(1536);
        }
        else
        {
            if(draw_0 == u32(2))
            {
                vertices_0 = _S1;
            }
            else
            {
                if(draw_0 == u32(3))
                {
                    vertices_0 = u32(15);
                }
                else
                {
                    if(draw_0 == u32(4))
                    {
                        vertices_0 = u32(7);
                    }
                    else
                    {
                        vertices_0 = u32(12);
                    }
                }
            }
        }
        atomicStore(&(drawArgs_0[at_0]), vertices_0);
        atomicStore(&(drawArgs_0[at_0 + u32(1)]), u32(0));
        atomicStore(&(drawArgs_0[at_0 + u32(2)]), u32(0));
        atomicStore(&(drawArgs_0[at_0 + u32(3)]), u32(0));
        draw_0 = draw_0 + u32(1);
    }
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
    var _S2 : vec3<i32> = vec3<i32>(clamp(vec2<i32>(floor((world_0 - grass_0.cover_0.xy) * vec2<f32>(grass_0.cover_0.w) + vec2<f32>(0.5f))), vec2<i32>(i32(0), i32(0)), vec2<i32>(i32(grass_0.maps_0.z) - i32(1), i32(grass_0.maps_0.w) - i32(1))), i32(0));
    return (textureLoad((grassCover_0), ((_S2)).xy, ((_S2)).z));
}

fn grass_unorm8_0( value_1 : f32) -> u32
{
    return u32(value_1 * 255.0f + 0.5f);
}

fn grass_ground_texel_0( texel_0 : vec2<i32>) -> f32
{
    var _S3 : vec3<i32> = vec3<i32>(clamp(texel_0, vec2<i32>(i32(0), i32(0)), vec2<i32>(i32(grass_0.maps_0.x) - i32(1), i32(grass_0.maps_0.y) - i32(1))), i32(0));
    return (textureLoad((grassGround_0), ((_S3)).xy, ((_S3)).z).x);
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
    var _S4 : vec2<i32> = vec2<i32>(base_0);
    var h00_0 : f32 = grass_ground_texel_0(_S4);
    var h10_0 : f32 = grass_ground_texel_0(_S4 + vec2<i32>(i32(1), i32(0)));
    var h01_0 : f32 = grass_ground_texel_0(_S4 + vec2<i32>(i32(0), i32(1)));
    var h11_0 : f32 = grass_ground_texel_0(_S4 + vec2<i32>(i32(1), i32(1)));
    var _S5 : f32 = blend_0.x;
    var _S6 : f32 = h10_0 - h00_0;
    var lower_0 : f32 = h00_0 + _S5 * _S6;
    var _S7 : f32 = h11_0 - h01_0;
    var under_0 : GrassGround_0;
    under_0.height_0 = lower_0 + blend_0.y * (h01_0 + _S5 * _S7 - lower_0);
    under_0.normal_0 = normalize(vec3<f32>(- (0.5f * (_S6 + _S7)), grass_0.ground_0.z, - (0.5f * (h01_0 - h00_0 + (h11_0 - h10_0)))));
    return under_0;
}

fn grass_lattice_key_0( square_0 : vec2<i32>) -> u32
{
    return ((u32(square_0.x) * u32(2376512323)) ^ ((u32(square_0.y) * u32(3625334849))));
}

struct GrassClump_0
{
     id_0 : u32,
     offset_0 : vec2<i32>,
};

fn grass_clump_of_0( global_0 : vec2<u32>,  jitter_0 : u32) -> GrassClump_0
{
    var _S8 : vec2<i32> = vec2<i32>(global_0) * vec2<i32>(i32(256)) + vec2<i32>(i32((((jitter_0 & (u32(65535)))) >> (u32(8)))), i32((((((jitter_0 >> (u32(16)))) & (u32(65535)))) >> (u32(8)))));
    var _S9 : vec2<u32> = global_0 / vec2<u32>(u32(8));
    var _S10 : vec2<i32> = vec2<i32>(_S9);
    var nearest_0 : GrassClump_0;
    nearest_0.id_0 = u32(0);
    nearest_0.offset_0 = vec2<i32>(i32(0), i32(0));
    var best_0 : i32 = i32(2147483647);
    var dz_0 : i32 = i32(-1);
    for(;;)
    {
        if(dz_0 <= i32(1))
        {
        }
        else
        {
            break;
        }
        var best_1 : i32 = best_0;
        var dx_0 : i32 = i32(-1);
        for(;;)
        {
            if(dx_0 <= i32(1))
            {
            }
            else
            {
                break;
            }
            var square_1 : vec2<i32> = _S10 + vec2<i32>(dx_0, dz_0);
            var id_1 : u32 = grass_hash_0(((grass_lattice_key_0(square_1)) ^ (u32(739982445))));
            var offset_1 : vec2<i32> = _S8 - (square_1 * vec2<i32>(i32(2048)) + vec2<i32>(i32((id_1 & (u32(2047)))), i32((((id_1 >> (u32(16)))) & (u32(2047))))));
            var _S11 : i32 = offset_1.x;
            var _S12 : i32 = offset_1.y;
            var distance_0 : i32 = _S11 * _S11 + _S12 * _S12;
            if(distance_0 < best_1)
            {
                nearest_0.id_0 = id_1;
                nearest_0.offset_0 = offset_1;
                best_1 = distance_0;
            }
            dx_0 = dx_0 + i32(1);
        }
        var dz_1 : i32 = dz_0 + i32(1);
        best_0 = best_1;
        dz_0 = dz_1;
    }
    return nearest_0;
}

fn windSmoothTriangle_0( u_0 : f32) -> f32
{
    var s_0 : f32 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}

fn windSample_0( posRel_0 : vec3<f32>) -> vec3<f32>
{
    var _S13 : vec2<f32> = posRel_0.xz;
    var deflection_0 : vec2<f32> = (textureSampleLevel((windDirectionLayer_0), (windSampler_0), (wind_0.directionUv_0 + _S13 * wind_0.directionUvPerMetre_0), (0.0f))).xy * vec2<f32>(2.0f) - vec2<f32>(1.0f);
    var intensity_0 : f32 = (textureSampleLevel((windIntensityLayer_0), (windSampler_0), (wind_0.intensityUv_0 + _S13 * wind_0.intensityUvPerMetre_0), (0.0f))).x;
    var base_1 : vec2<f32> = wind_0.baseDirection_0;
    var _S14 : f32 = wind_0.baseDirection_0.x;
    var _S15 : f32 = deflection_0.x;
    var _S16 : f32 = wind_0.baseDirection_0.y;
    var _S17 : f32 = deflection_0.y;
    var turned_0 : vec2<f32> = vec2<f32>(_S14 * _S15 - _S16 * _S17, _S14 * _S17 + _S16 * _S15);
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
    var speed_0 : f32 = intensity_0 * wind_0.baseSpeed_0 * (1.0f + wind_0.gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S13, base_1) * wind_0.invGustWavelength_0 + wind_0.gustPhase_0) - 1.0f));
    return vec3<f32>(direction_0.x * speed_0, 0.0f, direction_0.y * speed_0);
}

fn grass_lean_0( velocity_0 : vec3<f32>,  height_1 : f32) -> vec3<f32>
{
    var speed_1 : f32 = length(velocity_0);
    var bend_0 : f32 = height_1 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    var along_0 : vec2<f32> = velocity_0.xz * vec2<f32>((bend_0 / max(speed_1, 9.99999997475242708e-07f)));
    var _S18 : f32 = bend_0 * bend_0;
    return vec3<f32>(along_0.x, - (_S18 / (height_1 + sqrt(max(height_1 * height_1 - _S18, 0.0f)))), along_0.y);
}

fn grass_kept_far_0( global_1 : vec2<u32>) -> bool
{
    return ((((global_1.x) & (u32(1)))) + u32(2) * (((global_1.y) & (u32(1))))) == (((grass_hash_0(((grass_lattice_key_0(vec2<i32>((global_1 >> (vec2<u32>(u32(1))))))) ^ (u32(695872825))))) & (u32(3))));
}

struct GrassInstance_0
{
     root_0 : vec4<f32>,
     facing_0 : vec4<f32>,
     lean_0 : vec4<f32>,
     ground_1 : vec4<f32>,
     clump_0 : vec4<f32>,
     lanes_0 : vec4<u32>,
};

@compute
@workgroup_size(64, 1, 1)
fn generateMain(@builtin(global_invocation_id) thread_1 : vec3<u32>)
{
    var cells_0 : u32 = grass_0.limits_0.z;
    var capacity_0 : u32 = grass_0.limits_0.x;
    var _S19 : u32 = thread_1.x;
    if(_S19 >= (min(cells_0 * cells_0, capacity_0)))
    {
        return;
    }
    var cell_0 : u32 = tile_1.slot_0.x * capacity_0 + _S19;
    var empty_0 : GrassInstance_0;
    const _S20 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    empty_0.root_0 = _S20;
    empty_0.facing_0 = _S20;
    empty_0.lean_0 = _S20;
    empty_0.ground_1 = _S20;
    empty_0.clump_0 = _S20;
    empty_0.lanes_0 = vec4<u32>(u32(0), u32(0), u32(0), u32(0));
    grassCells_0[cell_0].root_0 = empty_0.root_0;
    grassCells_0[cell_0].facing_0 = empty_0.facing_0;
    grassCells_0[cell_0].lean_0 = empty_0.lean_0;
    grassCells_0[cell_0].ground_1 = empty_0.ground_1;
    grassCells_0[cell_0].clump_0 = empty_0.clump_0;
    grassCells_0[cell_0].lanes_0 = empty_0.lanes_0;
    var side_0 : f32 = tile_1.tile_0.z / f32(cells_0);
    var jitter_lane_0 : u32 = grass_hash_0(cell_0);
    var jitter_1 : vec2<f32> = grass_unit_pair_0(jitter_lane_0);
    var _S21 : vec2<f32> = tile_1.tile_0.xy;
    var _S22 : u32 = _S19 % cells_0;
    var _S23 : f32 = f32(_S22);
    var _S24 : u32 = _S19 / cells_0;
    var world_2 : vec2<f32> = _S21 + (vec2<f32>(_S23, f32(_S24)) + jitter_1) * vec2<f32>(side_0);
    var cover_1 : vec4<f32> = grass_cover_under_0(world_2);
    if((((grass_hash_0((cell_0 ^ (u32(2654435769))))) & (u32(65535)))) >= (grass_unorm8_0(cover_1.x) * u32(257)))
    {
        return;
    }
    var _S25 : u32 = min(grass_unorm8_0(cover_1.y), max(grass_0.limits_0.y, u32(1)) - u32(1));
    var under_1 : GrassGround_0 = grass_ground_under_0(world_2);
    var root_1 : vec3<f32> = vec3<f32>(world_2.x, under_1.height_0, world_2.y);
    var relative_0 : vec3<f32> = root_1 - grass_0.camera_0.xyz;
    var _S26 : f32 = dot(relative_0, relative_0);
    if(_S26 > (grass_0.camera_0.w * grass_0.camera_0.w))
    {
        return;
    }
    var _S27 : u32 = tile_1.slot_0.y * cells_0 + _S22;
    var _S28 : u32 = tile_1.slot_0.z * cells_0;
    var _S29 : u32 = _S19 / cells_0;
    var global_2 : vec2<u32> = vec2<u32>(_S27, _S28 + _S29);
    var clump_2 : GrassClump_0 = grass_clump_of_0(global_2, jitter_lane_0);
    var spread_0 : vec2<f32> = grass_unit_pair_0(grass_hash_0((cell_0 ^ (u32(3266489909)))));
    var height_2 : f32 = blades_0[_S25].size_0.x * (1.0f - blades_0[_S25].size_0.z * spread_0.x) * (1.0f - blades_0[_S25].clump_1.y * grass_unit_pair_0(grass_hash_0(((clump_2.id_0) ^ (u32(3266489909))))).x);
    var half_width_0 : f32 = blades_0[_S25].size_0.y * (1.0f - blades_0[_S25].size_0.w * spread_0.y);
    var _S30 : vec2<f32> = vec2<f32>(2.0f);
    var _S31 : vec2<f32> = vec2<f32>(1.0f);
    var own_0 : vec2<f32> = grass_unit_pair_0(grass_hash_0((cell_0 ^ (u32(2246822507))))) * _S30 - _S31;
    var square_2 : vec2<f32> = own_0 + (grass_unit_pair_0(grass_hash_0(((clump_2.id_0) ^ (u32(2246822507))))) * _S30 - _S31 - own_0) * vec2<f32>(blades_0[_S25].clump_1.x);
    var square_length_0 : f32 = dot(square_2, square_2);
    var facing_1 : vec2<f32>;
    if(square_length_0 > 9.99999993922529029e-09f)
    {
        facing_1 = square_2 / vec2<f32>(sqrt(square_length_0));
    }
    else
    {
        facing_1 = vec2<f32>(1.0f, 0.0f);
    }
    var lean_1 : vec3<f32> = grass_lean_0(windSample_0(relative_0), height_2);
    var instance_0 : GrassInstance_0;
    instance_0.root_0 = vec4<f32>(root_1, height_2);
    instance_0.facing_0 = vec4<f32>(facing_1, half_width_0, grass_unit_pair_0(grass_hash_0((cell_0 ^ (u32(668265263))))).x);
    instance_0.lean_0 = vec4<f32>(lean_1, 0.0f);
    instance_0.ground_1 = vec4<f32>(under_1.normal_0, 0.0f);
    instance_0.clump_0 = vec4<f32>(vec2<f32>(clump_2.offset_0) * vec2<f32>((side_0 * 0.00390625f)), 0.0f, 0.0f);
    var kept_0 : bool = grass_kept_far_0(global_2);
    var _S32 : u32;
    if(kept_0)
    {
        _S32 = u32(1);
    }
    else
    {
        _S32 = u32(0);
    }
    instance_0.lanes_0 = vec4<u32>(cell_0, _S25, clump_2.id_0, _S32);
    grassCells_0[cell_0].root_0 = instance_0.root_0;
    grassCells_0[cell_0].facing_0 = instance_0.facing_0;
    grassCells_0[cell_0].lean_0 = instance_0.lean_0;
    grassCells_0[cell_0].ground_1 = instance_0.ground_1;
    grassCells_0[cell_0].clump_0 = instance_0.clump_0;
    grassCells_0[cell_0].lanes_0 = instance_0.lanes_0;
    var slot_args_0 : u32 = tile_1.slot_0.x * u32(5);
    var _S33 : u32 = blades_0[_S25].flags_0.x;
    if(_S33 == u32(1))
    {
        atomicStore(&(drawArgs_0[(slot_args_0 + u32(1)) * u32(4) + u32(1)]), grass_0.looks_0.x);
        atomicStore(&(drawArgs_0[(slot_args_0 + u32(2)) * u32(4) + u32(1)]), grass_0.looks_0.y);
        return;
    }
    if(_S33 == u32(2))
    {
        if(_S26 < (grass_0.lod_0.x * grass_0.lod_0.x))
        {
            var near_at_0 : u32 = atomicAdd(&(drawArgs_0[(slot_args_0 + u32(3)) * u32(4) + u32(1)]), u32(1));
            instances_0[(grass_0.limits_0.w + tile_1.slot_0.x) * capacity_0 + near_at_0].root_0 = instance_0.root_0;
            instances_0[(grass_0.limits_0.w + tile_1.slot_0.x) * capacity_0 + near_at_0].facing_0 = instance_0.facing_0;
            instances_0[(grass_0.limits_0.w + tile_1.slot_0.x) * capacity_0 + near_at_0].lean_0 = instance_0.lean_0;
            instances_0[(grass_0.limits_0.w + tile_1.slot_0.x) * capacity_0 + near_at_0].ground_1 = instance_0.ground_1;
            instances_0[(grass_0.limits_0.w + tile_1.slot_0.x) * capacity_0 + near_at_0].clump_0 = instance_0.clump_0;
            instances_0[(grass_0.limits_0.w + tile_1.slot_0.x) * capacity_0 + near_at_0].lanes_0 = instance_0.lanes_0;
        }
        else
        {
            if(kept_0)
            {
                var far_at_0 : u32 = atomicAdd(&(drawArgs_0[(slot_args_0 + u32(4)) * u32(4) + u32(1)]), u32(1));
                instances_0[tile_1.slot_0.x * capacity_0 + capacity_0 - u32(1) - far_at_0].root_0 = instance_0.root_0;
                instances_0[tile_1.slot_0.x * capacity_0 + capacity_0 - u32(1) - far_at_0].facing_0 = instance_0.facing_0;
                instances_0[tile_1.slot_0.x * capacity_0 + capacity_0 - u32(1) - far_at_0].lean_0 = instance_0.lean_0;
                instances_0[tile_1.slot_0.x * capacity_0 + capacity_0 - u32(1) - far_at_0].ground_1 = instance_0.ground_1;
                instances_0[tile_1.slot_0.x * capacity_0 + capacity_0 - u32(1) - far_at_0].clump_0 = instance_0.clump_0;
                instances_0[tile_1.slot_0.x * capacity_0 + capacity_0 - u32(1) - far_at_0].lanes_0 = instance_0.lanes_0;
            }
        }
        return;
    }
    var at_2 : u32 = atomicAdd(&(drawArgs_0[slot_args_0 * u32(4) + u32(1)]), u32(1));
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].root_0 = instance_0.root_0;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].facing_0 = instance_0.facing_0;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].lean_0 = instance_0.lean_0;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].ground_1 = instance_0.ground_1;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].clump_0 = instance_0.clump_0;
    instances_0[tile_1.slot_0.x * capacity_0 + at_2].lanes_0 = instance_0.lanes_0;
    return;
}

