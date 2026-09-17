struct GrassTile_std140_0
{
    @align(16) tile_0 : vec4<f32>,
    @align(16) slot_0 : vec4<u32>,
};

@binding(2) @group(0) var<uniform> tile_1 : GrassTile_std140_0;
struct GrassParams_std140_0
{
    @align(16) limits_0 : vec4<u32>,
    @align(16) screen_0 : vec4<f32>,
};

@binding(1) @group(0) var<uniform> grass_0 : GrassParams_std140_0;
struct GrassInstance_std430_0
{
    @align(16) root_0 : vec4<f32>,
    @align(16) facing_0 : vec4<f32>,
    @align(16) lean_0 : vec4<f32>,
    @align(16) ground_0 : vec4<f32>,
    @align(16) lanes_0 : vec4<u32>,
};

@binding(3) @group(0) var<storage, read> instances_0 : array<GrassInstance_std430_0>;

struct GrassBlade_std430_0
{
    @align(16) root_color_0 : vec4<f32>,
    @align(16) tip_color_0 : vec4<f32>,
    @align(16) size_0 : vec4<f32>,
    @align(16) occlusion_0 : vec4<f32>,
    @align(16) glow_0 : vec4<f32>,
    @align(16) patch_0 : vec4<f32>,
    @align(16) flags_0 : vec4<u32>,
};

@binding(4) @group(0) var<storage, read> blades_0 : array<GrassBlade_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0
{
    @align(16) data_1 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(2)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0
{
    @align(16) data_2 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(14)>,
};

struct FrameUniforms_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) ambient_0 : vec4<f32>,
    @align(16) shadow_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0,
    @align(16) cascade_far_0 : vec4<f32>,
    @align(16) shadow_params_0 : vec4<f32>,
    @align(16) cluster_grid_0 : vec4<u32>,
    @align(16) light_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0,
    @align(16) probe_counts_0 : vec4<u32>,
    @align(16) probe_levels_0 : vec4<u32>,
    @align(16) probe_level_origin_0 : array<vec4<f32>, i32(4)>,
    @align(16) probe_level_inv_spacing_0 : array<vec4<f32>, i32(4)>,
    @align(16) probe_level_offset_0 : array<vec4<u32>, i32(4)>,
    @align(16) lod_params_0 : vec4<f32>,
    @align(16) fog_params_0 : vec4<f32>,
    @align(16) fog_color_0 : vec4<f32>,
    @align(16) sky_sh_r_0 : vec4<f32>,
    @align(16) sky_sh_g_0 : vec4<f32>,
    @align(16) sky_sh_b_0 : vec4<f32>,
    @align(16) previous_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) vertex_pool_0 : vec4<u32>,
    @align(16) shadow_atlas_rect_0 : array<vec4<f32>, i32(16)>,
    @align(16) shadow_filter_0 : vec4<u32>,
};

@binding(0) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
struct GrassField_std140_0
{
    @align(16) origin_0 : vec4<f32>,
    @align(16) tiles_0 : vec4<u32>,
    @align(16) ground_1 : vec4<f32>,
    @align(16) maps_0 : vec4<u32>,
    @align(16) stack_0 : vec4<f32>,
    @align(16) layers_0 : array<vec4<f32>, i32(64)>,
};

@binding(11) @group(0) var<uniform> field_0 : GrassField_std140_0;
@binding(5) @group(0) var grassCard_0 : texture_2d_array<f32>;

@binding(6) @group(0) var grassCardSampler_0 : sampler;

@binding(10) @group(0) var<storage, read> cluster_lights_0 : array<u32>;

struct GpuLight_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) direction_0 : vec4<f32>,
    @align(16) tangent_0 : vec4<f32>,
    @align(16) kind_0 : u32,
    @align(4) cos_inner_0 : f32,
    @align(8) shadow_tile_0 : u32,
    @align(4) flags_1 : u32,
};

@binding(9) @group(0) var<storage, read> lights_0 : array<GpuLight_std430_0>;

@binding(7) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(8) @group(0) var shadow_sampler_0 : sampler_comparison;

@binding(13) @group(0) var grassGround_0 : texture_2d<f32>;

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

@binding(14) @group(0) var<uniform> wind_0 : WindParams_std140_0;
@binding(15) @group(0) var windDirectionLayer_0 : texture_2d<f32>;

@binding(17) @group(0) var windSampler_0 : sampler;

@binding(16) @group(0) var windIntensityLayer_0 : texture_2d<f32>;

@binding(12) @group(0) var<storage, read> grassCells_0 : array<GrassInstance_std430_0>;

var<private> SHADOW_SEARCH_DISC_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(0.17677700519561768f, 0.0f), vec2<f32>(-0.22577199339866638f, 0.20682600140571594f), vec2<f32>(0.0345579981803894f, -0.39377099275588989f), vec2<f32>(0.28457099199295044f, 0.37117299437522888f), vec2<f32>(-0.52222299575805664f, -0.09237399697303772f), vec2<f32>(0.49469500780105591f, -0.31468498706817627f), vec2<f32>(-0.16546599566936493f, 0.6155250072479248f), vec2<f32>(-0.31556099653244019f, -0.60759401321411133f), vec2<f32>(0.68464201688766479f, 0.25003001093864441f), vec2<f32>(-0.71225601434707642f, 0.2940090000629425f), vec2<f32>(0.3433539867401123f, -0.73372900485992432f), vec2<f32>(0.25372999906539917f, 0.80893200635910034f), vec2<f32>(-0.76474601030349731f, -0.44318601489067078f), vec2<f32>(0.89713400602340698f, -0.19723199307918549f), vec2<f32>(-0.54750698804855347f, 0.77877199649810791f), vec2<f32>(-0.12648700177669525f, -0.97609001398086548f) );
var<private> SHADOW_DISC_0 : array<vec2<f32>, i32(32)> = array<vec2<f32>, i32(32)>( vec2<f32>(0.125f, 0.0f), vec2<f32>(-0.15964500606060028f, 0.14624799787998199f), vec2<f32>(0.02443600073456764f, -0.27843800187110901f), vec2<f32>(0.2012220025062561f, 0.26245900988578796f), vec2<f32>(-0.36926800012588501f, -0.06531800329685211f), vec2<f32>(0.34980198740959167f, -0.22251600027084351f), vec2<f32>(-0.11700200289487839f, 0.43524199724197388f), vec2<f32>(-0.22313599288463593f, -0.42963400483131409f), vec2<f32>(0.48411500453948975f, 0.17679800093173981f), vec2<f32>(-0.50364100933074951f, 0.20789599418640137f), vec2<f32>(0.24278800189495087f, -0.51882398128509521f), vec2<f32>(0.17941400408744812f, 0.57200098037719727f), vec2<f32>(-0.54075700044631958f, -0.31338000297546387f), vec2<f32>(0.63437002897262573f, -0.13946400582790375f), vec2<f32>(-0.38714599609375f, 0.55067497491836548f), vec2<f32>(-0.0894400030374527f, -0.69019997119903564f), vec2<f32>(0.5490720272064209f, 0.46275800466537476f), vec2<f32>(-0.73887801170349121f, 0.0305550005286932f), vec2<f32>(0.5389549732208252f, -0.53633201122283936f), vec2<f32>(-0.03605800122022629f, 0.77979201078414917f), vec2<f32>(-0.51281797885894775f, -0.61452698707580566f), vec2<f32>(0.81235998868942261f, 0.10930199921131134f), vec2<f32>(-0.68831098079681396f, 0.47890898585319519f), vec2<f32>(0.18808600306510925f, -0.83606100082397461f), vec2<f32>(0.43503299355506897f, 0.75919097661972046f), vec2<f32>(-0.85044801235198975f, -0.27131599187850952f), vec2<f32>(0.82610201835632324f, -0.38168001174926758f), vec2<f32>(-0.35788801312446594f, 0.85515600442886353f), vec2<f32>(-0.31940698623657227f, -0.88803398609161377f), vec2<f32>(0.84990900754928589f, 0.44668799638748169f), vec2<f32>(-0.94403499364852905f, 0.24884499609470367f), vec2<f32>(0.53659600019454956f, -0.83452999591827393f) );
var<private> SHADOW_PROBE_INDEX_0 : array<u32, i32(5)> = array<u32, i32(5)>( u32(0), u32(23), u32(25), u32(27), u32(29) );
var<private> SHADOW_ROTATIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(1.0f, 0.0f), vec2<f32>(0.92387998104095459f, 0.38268300890922546f), vec2<f32>(0.70710700750350952f, 0.70710700750350952f), vec2<f32>(0.38268300890922546f, 0.92387998104095459f), vec2<f32>(0.0f, 1.0f), vec2<f32>(-0.38268300890922546f, 0.92387998104095459f), vec2<f32>(-0.70710700750350952f, 0.70710700750350952f), vec2<f32>(-0.92387998104095459f, 0.38268300890922546f), vec2<f32>(-1.0f, 0.0f), vec2<f32>(-0.92387998104095459f, -0.38268300890922546f), vec2<f32>(-0.70710700750350952f, -0.70710700750350952f), vec2<f32>(-0.38268300890922546f, -0.92387998104095459f), vec2<f32>(-0.0f, -1.0f), vec2<f32>(0.38268300890922546f, -0.92387998104095459f), vec2<f32>(0.70710700750350952f, -0.70710700750350952f), vec2<f32>(0.92387998104095459f, -0.38268300890922546f) );
var<private> SHADOW_DITHER_0 : array<u32, i32(16)> = array<u32, i32(16)>( u32(0), u32(8), u32(2), u32(10), u32(12), u32(4), u32(14), u32(6), u32(3), u32(11), u32(1), u32(9), u32(15), u32(7), u32(13), u32(5) );
var<private> GRASS_CARD_CORNERS_0 : array<vec2<f32>, i32(6)> = array<vec2<f32>, i32(6)>( vec2<f32>(0.0f, 0.0f), vec2<f32>(1.0f, 0.0f), vec2<f32>(0.0f, 1.0f), vec2<f32>(1.0f, 0.0f), vec2<f32>(1.0f, 1.0f), vec2<f32>(0.0f, 1.0f) );
struct GrassBlade_0
{
     root_color_0 : vec4<f32>,
     tip_color_0 : vec4<f32>,
     size_0 : vec4<f32>,
     occlusion_0 : vec4<f32>,
     glow_0 : vec4<f32>,
     patch_0 : vec4<f32>,
     flags_0 : vec4<u32>,
};

fn grass_row_0( index_0 : u32) -> GrassBlade_0
{
    var _S1 : GrassBlade_std430_0 = blades_0[min(index_0, max(grass_0.limits_0.y, u32(1)) - u32(1))];
    var _S2 : GrassBlade_0 = GrassBlade_0( _S1.root_color_0, _S1.tip_color_0, _S1.size_0, _S1.occlusion_0, _S1.glow_0, _S1.patch_0, _S1.flags_0 );
    return _S2;
}

fn grassCardLevel_0( pixels_0 : f32) -> f32
{
    var _S3 : f32 = 64.0f / max(pixels_0, 0.00009999999747379f);
    var level_0 : f32 = 0.0f;
    var step_0 : u32 = u32(1);
    for(;;)
    {
        if(step_0 < u32(7))
        {
        }
        else
        {
            break;
        }
        var _S4 : u32 = step_0 - u32(1);
        var lower_0 : f32 = f32((u32(1) << (_S4)));
        if(_S3 >= lower_0)
        {
            level_0 = f32(_S4) + min((_S3 - lower_0) / lower_0, 1.0f);
        }
        step_0 = step_0 + u32(1);
    }
    return min(floor(level_0 + 0.5f), 6.0f);
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

fn grass_patch_0( cell_0 : u32) -> f32
{
    var _S5 : u32 = max(field_0.tiles_0.w, u32(1));
    var _S6 : u32 = max(field_0.tiles_0.z, u32(1));
    var _S7 : u32 = max(field_0.tiles_0.x, u32(1));
    var slot_1 : u32 = cell_0 / _S5;
    var lane_1 : u32 = cell_0 % _S5;
    var _S8 : u32 = slot_1 % _S7;
    var _S9 : u32 = _S8 * _S6;
    var _S10 : u32 = lane_1 % _S6;
    var x_0 : u32 = _S9 + _S10;
    var _S11 : u32 = slot_1 / _S7;
    var _S12 : u32 = _S11 * _S6;
    var _S13 : u32 = lane_1 / _S6;
    return grass_unit_pair_0(grass_hash_0(((((x_0 / u32(8) * u32(2376512323)) ^ (((_S12 + _S13) / u32(8) * u32(3625334849))))) ^ (u32(374761393))))).x;
}

struct GrassVertex_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @location(1) normal_0 : vec3<f32>,
    @location(2) color_1 : vec3<f32>,
    @location(3) uv_0 : vec2<f32>,
    @location(4) level_1 : f32,
    @interpolate(flat) @location(5) row_0 : u32,
    @interpolate(flat) @location(6) patch_1 : f32,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_1 : u32, @builtin(instance_index) instance_id_0 : u32) -> GrassVertex_0
{
    var blade_0 : GrassInstance_std430_0 = instances_0[tile_1.slot_0.x * grass_0.limits_0.x + instance_id_0];
    var _S14 : u32 = blade_0.lanes_0.y;
    var row_1 : GrassBlade_0 = grass_row_0(_S14);
    var _S15 : u32 = index_1 % u32(6);
    var facing_1 : vec2<f32> = blade_0.facing_0.xy;
    var across_0 : vec2<f32>;
    if((index_1 / u32(6)) == u32(0))
    {
        across_0 = facing_1;
    }
    else
    {
        across_0 = vec2<f32>(- facing_1.y, facing_1.x);
    }
    var _S16 : f32 = blade_0.facing_0.z;
    var _S17 : f32 = GRASS_CARD_CORNERS_0[_S15].y;
    var world_0 : vec3<f32> = blade_0.root_0.xyz + vec3<f32>(across_0.x, 0.0f, across_0.y) * vec3<f32>((_S16 * (GRASS_CARD_CORNERS_0[_S15].x * 2.0f - 1.0f))) + vec3<f32>(0.0f, blade_0.root_0.w * _S17, 0.0f) + blade_0.lean_0.xyz * vec3<f32>((_S17 * _S17));
    var output_0 : GrassVertex_0;
    output_0.position_1 = (((vec4<f32>(world_0, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_0[i32(0)][i32(0)], frame_0.view_proj_0.data_0[i32(1)][i32(0)], frame_0.view_proj_0.data_0[i32(2)][i32(0)], frame_0.view_proj_0.data_0[i32(3)][i32(0)], frame_0.view_proj_0.data_0[i32(0)][i32(1)], frame_0.view_proj_0.data_0[i32(1)][i32(1)], frame_0.view_proj_0.data_0[i32(2)][i32(1)], frame_0.view_proj_0.data_0[i32(3)][i32(1)], frame_0.view_proj_0.data_0[i32(0)][i32(2)], frame_0.view_proj_0.data_0[i32(1)][i32(2)], frame_0.view_proj_0.data_0[i32(2)][i32(2)], frame_0.view_proj_0.data_0[i32(3)][i32(2)], frame_0.view_proj_0.data_0[i32(0)][i32(3)], frame_0.view_proj_0.data_0[i32(1)][i32(3)], frame_0.view_proj_0.data_0[i32(2)][i32(3)], frame_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    output_0.world_position_0 = world_0;
    var _S18 : vec3<f32>;
    if((row_1.flags_0.y) == u32(1))
    {
        _S18 = vec3<f32>(0.0f, 1.0f, 0.0f);
    }
    else
    {
        _S18 = blade_0.ground_0.xyz;
    }
    output_0.normal_0 = _S18;
    output_0.color_1 = mix(row_1.root_color_0.xyz, row_1.tip_color_0.xyz, vec3<f32>(_S17)) * vec3<f32>((1.0f - 0.18000000715255737f * blade_0.facing_0.w));
    output_0.uv_0 = GRASS_CARD_CORNERS_0[_S15];
    output_0.level_1 = grassCardLevel_0(2.0f * _S16 * grass_0.screen_0.x / max(abs(output_0.position_1.w), 0.00009999999747379f));
    output_0.row_0 = min(_S14, max(grass_0.limits_0.y, u32(1)) - u32(1));
    var _S19 : f32 = grass_patch_0(blade_0.lanes_0.x);
    output_0.patch_1 = _S19;
    return output_0;
}

fn grass_style_0( color_2 : vec3<f32>,  row_2 : ptr<function, GrassBlade_std430_0>,  along_0 : f32,  patch_2 : f32) -> vec3<f32>
{
    var _S20 : vec4<f32> = (*row_2).occlusion_0;
    var reach_0 : f32 = (*row_2).occlusion_0.w;
    var occluded_0 : f32;
    if(reach_0 > 0.0f)
    {
        occluded_0 = saturate(1.0f - along_0 / reach_0);
    }
    else
    {
        occluded_0 = 0.0f;
    }
    var start_0 : f32 = (*row_2).glow_0.w;
    return mix(mix(color_2, _S20.xyz, vec3<f32>(occluded_0)), (*row_2).patch_0.xyz, vec3<f32>(((*row_2).patch_0.w * patch_2))) + (*row_2).glow_0.xyz * vec3<f32>(saturate((along_0 - start_0) / max(1.0f - start_0, 0.00009999999747379f)));
}

fn grass_style_1( color_3 : vec3<f32>,  row_3 : GrassBlade_0,  along_1 : f32,  patch_3 : f32) -> vec3<f32>
{
    var reach_1 : f32 = row_3.occlusion_0.w;
    var occluded_1 : f32;
    if(reach_1 > 0.0f)
    {
        occluded_1 = saturate(1.0f - along_1 / reach_1);
    }
    else
    {
        occluded_1 = 0.0f;
    }
    var start_1 : f32 = row_3.glow_0.w;
    return mix(mix(color_3, row_3.occlusion_0.xyz, vec3<f32>(occluded_1)), row_3.patch_0.xyz, vec3<f32>((row_3.patch_0.w * patch_3))) + row_3.glow_0.xyz * vec3<f32>(saturate((along_1 - start_1) / max(1.0f - start_1, 0.00009999999747379f)));
}

fn froxel_of_0( pixel_0 : vec2<f32>,  depth_0 : f32) -> u32
{
    var _S21 : u32 = max(frame_0.cluster_grid_0.x, u32(1));
    var _S22 : u32 = max(frame_0.cluster_grid_0.y, u32(1));
    var _S23 : u32 = max(frame_0.cluster_grid_0.z, u32(1));
    var _S24 : u32 = max(frame_0.cluster_grid_0.w, u32(1));
    var _S25 : u32 = u32(pixel_0.x) / _S24;
    var _S26 : u32 = min(_S25, _S21 - u32(1));
    var _S27 : u32 = u32(pixel_0.y) / _S24;
    var scale_0 : f32 = 24.0f / log2(10000.0f);
    return (u32(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, f32(_S23 - u32(1)))) * _S22 + min(_S27, _S22 - u32(1))) * _S21 + _S26;
}

fn range_window_0( distance_0 : f32,  radius_0 : f32) -> f32
{
    var ratio_0 : f32 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    var window_0 : f32 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}

fn punctual_falloff_0( distance_1 : f32,  radius_1 : f32) -> f32
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}

fn spot_cone_0( to_light_0 : vec3<f32>,  axis_0 : vec3<f32>,  cos_outer_0 : f32,  cos_inner_1 : f32) -> f32
{
    return saturate((dot((vec3<f32>(0) - to_light_0), normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}

fn atlas_rect_0( tile_2 : u32) -> vec4<f32>
{
    return frame_0.shadow_atlas_rect_0[tile_2];
}

fn atlas_rect_is_empty_0( rect_0 : vec4<f32>) -> bool
{
    return !((rect_0.x) > 0.0f);
}

fn tile_texels_0( rect_1 : vec4<f32>) -> f32
{
    return rect_1.x / frame_0.shadow_params_0.x;
}

fn shadow_normal_offset_0( geometric_normal_0 : vec3<f32>,  to_light_1 : vec3<f32>) -> f32
{
    var cosine_0 : f32 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}

fn shadow_filter_mode_0( pixel_1 : vec2<f32>) -> u32
{
    var _S28 : u32;
    if(u32(pixel_1.x) < (frame_0.shadow_filter_0.z))
    {
        _S28 = frame_0.shadow_filter_0.x;
    }
    else
    {
        _S28 = frame_0.shadow_filter_0.y;
    }
    return _S28;
}

fn atlas_step_0( rect_2 : vec4<f32>) -> vec2<f32>
{
    return frame_0.shadow_params_0.xy / rect_2.xy;
}

fn atlas_uv_0( rect_3 : vec4<f32>,  tile_uv_0 : vec2<f32>) -> vec2<f32>
{
    return rect_3.zw + tile_uv_0 * rect_3.xy;
}

fn tile_tap_0( rect_4 : vec4<f32>,  texel_step_0 : vec2<f32>,  tile_uv_1 : vec2<f32>,  spoke_0 : vec2<f32>,  rotation_0 : vec2<f32>,  reference_0 : f32) -> f32
{
    var tile_min_0 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_step_0;
    var _S29 : f32 = spoke_0.x;
    var _S30 : f32 = rotation_0.x;
    var _S31 : f32 = spoke_0.y;
    var _S32 : f32 = rotation_0.y;
    return (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (atlas_uv_0(rect_4, clamp(tile_uv_1 + vec2<f32>(_S29 * _S30 - _S31 * _S32, _S29 * _S32 + _S31 * _S30) * texel_step_0, tile_min_0, vec2<f32>(1.0f) - tile_min_0))), (reference_0)));
}

fn tile_box_pcf_0( tile_3 : u32,  tile_uv_2 : vec2<f32>,  reference_1 : f32) -> f32
{
    var rect_5 : vec4<f32> = atlas_rect_0(tile_3);
    if(atlas_rect_is_empty_0(rect_5))
    {
        return 1.0f;
    }
    var _S33 : vec2<f32> = atlas_step_0(rect_5);
    var y_0 : i32 = i32(-1);
    var visibility_0 : f32 = 0.0f;
    for(;;)
    {
        if(y_0 <= i32(1))
        {
        }
        else
        {
            break;
        }
        var x_1 : i32 = i32(-1);
        for(;;)
        {
            if(x_1 <= i32(1))
            {
            }
            else
            {
                break;
            }
            var visibility_1 : f32 = visibility_0 + tile_tap_0(rect_5, _S33, tile_uv_2, vec2<f32>(f32(x_1), f32(y_0)), vec2<f32>(1.0f, 0.0f), reference_1);
            x_1 = x_1 + i32(1);
            visibility_0 = visibility_1;
        }
        y_0 = y_0 + i32(1);
    }
    return visibility_0 / 9.0f;
}

fn shadow_rotation_0( pixel_2 : vec2<f32>) -> vec2<f32>
{
    var cell_1 : vec2<u32> = (vec2<u32>(pixel_2) & (vec2<u32>(u32(3))));
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_1.y * u32(4) + cell_1.x]];
}

fn tile_pcf_0( tile_4 : u32,  tile_uv_3 : vec2<f32>,  reference_2 : f32,  pixel_3 : vec2<f32>,  radius_2 : f32) -> f32
{
    var _S34 : vec2<f32> = shadow_rotation_0(pixel_3);
    var rect_6 : vec4<f32> = atlas_rect_0(tile_4);
    if(atlas_rect_is_empty_0(rect_6))
    {
        return 1.0f;
    }
    var _S35 : vec2<f32> = atlas_step_0(rect_6);
    var spot_0 : u32 = u32(0);
    var probe_0 : f32 = 0.0f;
    for(;;)
    {
        if(spot_0 < u32(5))
        {
        }
        else
        {
            break;
        }
        var probe_1 : f32 = probe_0 + tile_tap_0(rect_6, _S35, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * vec2<f32>(radius_2), _S34, reference_2);
        spot_0 = spot_0 + u32(1);
        probe_0 = probe_1;
    }
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }
    var index_2 : u32 = u32(0);
    var visibility_2 : f32 = 0.0f;
    for(;;)
    {
        if(index_2 < u32(32))
        {
        }
        else
        {
            break;
        }
        var visibility_3 : f32 = visibility_2 + tile_tap_0(rect_6, _S35, tile_uv_3, SHADOW_DISC_0[index_2] * vec2<f32>(radius_2), _S34, reference_2);
        index_2 = index_2 + u32(1);
        visibility_2 = visibility_3;
    }
    return visibility_2 / 32.0f;
}

fn sun_penumbra_texels_0( cascade_0 : u32,  tile_uv_4 : vec2<f32>,  reference_3 : f32,  rotation_1 : vec2<f32>) -> f32
{
    var rect_7 : vec4<f32> = atlas_rect_0(cascade_0);
    var texel_step_1 : vec2<f32> = atlas_step_0(rect_7);
    var _S36 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_step_1;
    const _S37 : vec2<f32> = vec2<f32>(1.0f, 1.0f);
    var _S38 : vec2<f32> = _S37 / frame_0.shadow_params_0.xy;
    var index_3 : u32 = u32(0);
    var sum_0 : f32 = 0.0f;
    var found_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_3 < u32(16))
        {
        }
        else
        {
            break;
        }
        var spoke_1 : vec2<f32> = SHADOW_SEARCH_DISC_0[index_3] * vec2<f32>(8.0f);
        var _S39 : f32 = spoke_1.x;
        var _S40 : f32 = rotation_1.x;
        var _S41 : f32 = spoke_1.y;
        var _S42 : f32 = rotation_1.y;
        var _S43 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_7, clamp(tile_uv_4 + vec2<f32>(_S39 * _S40 - _S41 * _S42, _S39 * _S42 + _S41 * _S40) * texel_step_1, _S36, vec2<f32>(1.0f) - _S36)) * _S38, _S38 - _S37)), i32(0));
        var depth_1 : f32 = (textureLoad((shadow_atlas_0), ((_S43)).xy, ((_S43)).z));
        if(depth_1 > reference_3)
        {
            var found_1 : f32 = found_0 + 1.0f;
            sum_0 = sum_0 + depth_1;
            found_0 = found_1;
        }
        index_3 = index_3 + u32(1);
    }
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }
    var _S44 : f32 = 2.0f * frame_0.cascade_far_0[cascade_0];
    return clamp((sum_0 / found_0 - reference_3) * (_S44 + 40.0f) * 0.01999999955296516f / (_S44 / tile_texels_0(rect_7)), 2.0f, 8.0f);
}

fn cascade_visibility_0( cascade_1 : u32,  world_position_1 : vec3<f32>,  to_light_2 : vec3<f32>,  geometric_normal_1 : vec3<f32>,  pixel_4 : vec2<f32>) -> f32
{
    var rect_8 : vec4<f32> = atlas_rect_0(cascade_1);
    if(atlas_rect_is_empty_0(rect_8))
    {
        return 1.0f;
    }
    var texel_world_0 : f32 = 2.0f * frame_0.cascade_far_0[cascade_1] / tile_texels_0(rect_8);
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_1 + geometric_normal_1 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2))) + to_light_2 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.z)), 1.0f)) * (mat4x4<f32>(frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(0)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(1)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(2)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(0)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(1)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(2)][i32(3)], frame_0.shadow_view_proj_0.data_1[cascade_1].data_0[i32(3)][i32(3)]))));
    var ndc_0 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S45 : bool;
    if((any(((abs(ndc_0.xy)) > vec2<f32>(1.0f)))))
    {
        _S45 = true;
    }
    else
    {
        _S45 = (ndc_0.z) <= 0.0f;
    }
    if(_S45)
    {
        return 1.0f;
    }
    var tile_uv_5 : vec2<f32> = vec2<f32>(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);
    var mode_0 : u32 = shadow_filter_mode_0(pixel_4);
    if(mode_0 == u32(2))
    {
        return tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z);
    }
    if(mode_0 == u32(1))
    {
        return tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f);
    }
    var _S46 : f32 = ndc_0.z;
    return tile_pcf_0(cascade_1, tile_uv_5, _S46, pixel_4, sun_penumbra_texels_0(cascade_1, tile_uv_5, _S46, shadow_rotation_0(pixel_4)));
}

fn sun_visibility_0( world_position_2 : vec3<f32>,  to_light_3 : vec3<f32>,  n_dot_l_0 : f32,  geometric_normal_2 : vec3<f32>,  pixel_5 : vec2<f32>,  selected_0 : ptr<function, u32>,  fade_0 : ptr<function, f32>) -> f32
{
    var cascade_2 : u32;
    var covered_0 : bool;
    (*selected_0) = u32(2);
    (*fade_0) = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }
    var eye_distance_0 : f32 = length(world_position_2 - frame_0.camera_position_0.xyz);
    var index_4 : u32 = u32(0);
    for(;;)
    {
        if(index_4 < u32(2))
        {
        }
        else
        {
            covered_0 = false;
            cascade_2 = u32(1);
            break;
        }
        if(eye_distance_0 < (frame_0.cascade_far_0[index_4]))
        {
            covered_0 = true;
            cascade_2 = index_4;
            break;
        }
        index_4 = index_4 + u32(1);
    }
    if(covered_0)
    {
        (*selected_0) = cascade_2;
    }
    var visibility_4 : f32 = cascade_visibility_0(cascade_2, world_position_2, to_light_3, geometric_normal_2, pixel_5);
    var _S47 : u32 = cascade_2 + u32(1);
    if(_S47 >= u32(2))
    {
        return visibility_4;
    }
    var band_0 : f32 = frame_0.cascade_far_0[cascade_2] * 0.10000000149011612f;
    var blend_0 : f32 = saturate((eye_distance_0 - (frame_0.cascade_far_0[cascade_2] - band_0)) / band_0);
    (*fade_0) = blend_0;
    if(blend_0 <= 0.0f)
    {
        return visibility_4;
    }
    return mix(visibility_4, cascade_visibility_0(_S47, world_position_2, to_light_3, geometric_normal_2, pixel_5), blend_0);
}

fn point_face_0( from_light_0 : vec3<f32>) -> u32
{
    var axis_1 : vec3<f32> = abs(from_light_0);
    var _S48 : f32 = axis_1.x;
    var _S49 : f32 = axis_1.y;
    var _S50 : bool;
    if(_S48 >= _S49)
    {
        _S50 = _S48 >= (axis_1.z);
    }
    else
    {
        _S50 = false;
    }
    var _S51 : u32;
    if(_S50)
    {
        if((from_light_0.x) >= 0.0f)
        {
            _S51 = u32(0);
        }
        else
        {
            _S51 = u32(1);
        }
        return _S51;
    }
    if(_S49 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {
            _S51 = u32(2);
        }
        else
        {
            _S51 = u32(3);
        }
        return _S51;
    }
    if((from_light_0.z) >= 0.0f)
    {
        _S51 = u32(4);
    }
    else
    {
        _S51 = u32(5);
    }
    return _S51;
}

fn light_tile_0( tile_5 : u32) -> u32
{
    return u32(2) + tile_5;
}

fn punctual_visibility_0( tile_6 : u32,  world_position_3 : vec3<f32>,  to_light_4 : vec3<f32>,  n_dot_l_1 : f32,  map_world_0 : f32,  geometric_normal_3 : vec3<f32>,  pixel_6 : vec2<f32>) -> f32
{
    var atlas_0 : u32 = light_tile_0(tile_6);
    var rect_9 : vec4<f32> = atlas_rect_0(atlas_0);
    if(atlas_rect_is_empty_0(rect_9))
    {
        return 1.0f;
    }
    var texel_world_1 : f32 = map_world_0 / tile_texels_0(rect_9);
    var clip_1 : vec4<f32> = (((vec4<f32>(world_position_3 + geometric_normal_3 * vec3<f32>((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4))) + to_light_4 * vec3<f32>((texel_world_1 * 2.0f)), 1.0f)) * (mat4x4<f32>(frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(0)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(1)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(2)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(0)][i32(3)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(1)][i32(3)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(2)][i32(3)], frame_0.light_view_proj_0.data_2[tile_6].data_0[i32(3)][i32(3)]))));
    var _S52 : f32 = clip_1.w;
    if(_S52 <= 0.0f)
    {
        return 1.0f;
    }
    var ndc_1 : vec3<f32> = clip_1.xyz / vec3<f32>(_S52);
    var _S53 : bool;
    if((any(((abs(ndc_1.xy)) > vec2<f32>(1.0f)))))
    {
        _S53 = true;
    }
    else
    {
        _S53 = (ndc_1.z) <= 0.0f;
    }
    if(_S53)
    {
        _S53 = true;
    }
    else
    {
        _S53 = (ndc_1.z) > 1.0f;
    }
    if(_S53)
    {
        return 1.0f;
    }
    var tile_uv_6 : vec2<f32> = vec2<f32>(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);
    if((shadow_filter_mode_0(pixel_6)) == u32(2))
    {
        return tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z);
    }
    return tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f);
}

fn point_visibility_0( light_0 : ptr<function, GpuLight_std430_0>,  base_0 : u32,  world_position_4 : vec3<f32>,  to_light_5 : vec3<f32>,  n_dot_l_2 : f32,  geometric_normal_4 : vec3<f32>,  pixel_7 : vec2<f32>) -> f32
{
    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }
    var from_light_1 : vec3<f32> = world_position_4 - (*light_0).position_0.xyz;
    return punctual_visibility_0(base_0 + point_face_0(from_light_1), world_position_4, to_light_5, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7);
}

fn spot_visibility_0( light_1 : ptr<function, GpuLight_std430_0>,  tile_7 : u32,  world_position_5 : vec3<f32>,  to_light_6 : vec3<f32>,  n_dot_l_3 : f32,  geometric_normal_5 : vec3<f32>,  pixel_8 : vec2<f32>) -> f32
{
    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }
    var cos_outer_1 : f32 = (*light_1).direction_0.w;
    return punctual_visibility_0(tile_7, world_position_5, to_light_6, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (*light_1).position_0.xyz, normalize((*light_1).direction_0.xyz)), 0.0f), geometric_normal_5, pixel_8);
}

fn grass_light_0( world_position_6 : vec3<f32>,  normal_1 : vec3<f32>,  pixel_9 : vec2<f32>) -> vec3<f32>
{
    var _S54 : u32 = froxel_of_0(pixel_9, (((vec4<f32>(world_position_6, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_0[i32(0)][i32(0)], frame_0.view_proj_0.data_0[i32(1)][i32(0)], frame_0.view_proj_0.data_0[i32(2)][i32(0)], frame_0.view_proj_0.data_0[i32(3)][i32(0)], frame_0.view_proj_0.data_0[i32(0)][i32(1)], frame_0.view_proj_0.data_0[i32(1)][i32(1)], frame_0.view_proj_0.data_0[i32(2)][i32(1)], frame_0.view_proj_0.data_0[i32(3)][i32(1)], frame_0.view_proj_0.data_0[i32(0)][i32(2)], frame_0.view_proj_0.data_0[i32(1)][i32(2)], frame_0.view_proj_0.data_0[i32(2)][i32(2)], frame_0.view_proj_0.data_0[i32(3)][i32(2)], frame_0.view_proj_0.data_0[i32(0)][i32(3)], frame_0.view_proj_0.data_0[i32(1)][i32(3)], frame_0.view_proj_0.data_0[i32(2)][i32(3)], frame_0.view_proj_0.data_0[i32(3)][i32(3)])))).w);
    var base_1 : u32 = _S54 * u32(17);
    var _S55 : u32 = min(cluster_lights_0[base_1], u32(16));
    const _S56 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slot_2 : u32 = u32(0);
    var direct_0 : vec3<f32> = _S56;
    for(;;)
    {
        if(slot_2 < _S55)
        {
        }
        else
        {
            break;
        }
        var _S57 : GpuLight_std430_0 = lights_0[cluster_lights_0[base_1 + u32(1) + slot_2]];
        var _S58 : u32 = _S57.kind_0;
        if((_S57.kind_0) == u32(3))
        {
            slot_2 = slot_2 + u32(1);
            continue;
        }
        var _S59 : bool = _S58 == u32(0);
        var to_light_7 : vec3<f32>;
        var reach_2 : f32;
        if(_S59)
        {
            to_light_7 = normalize(_S57.direction_0.xyz);
            reach_2 = 1.0f;
        }
        else
        {
            var offset_0 : vec3<f32> = _S57.position_0.xyz - world_position_6;
            var distance_2 : f32 = length(offset_0);
            var to_light_8 : vec3<f32> = offset_0 / vec3<f32>(max(distance_2, 9.99999997475242708e-07f));
            var reach_3 : f32 = punctual_falloff_0(distance_2, _S57.position_0.w);
            if(_S58 == u32(2))
            {
                reach_2 = reach_3 * spot_cone_0(to_light_8, _S57.direction_0.xyz, _S57.direction_0.w, _S57.cos_inner_0);
            }
            else
            {
                reach_2 = reach_3;
            }
            to_light_7 = to_light_8;
        }
        var n_dot_l_4 : f32 = dot(normal_1, to_light_7);
        var reach_4 : f32;
        if(_S59)
        {
            var sun_cascade_0 : u32;
            var sun_fade_0 : f32;
            reach_4 = sun_visibility_0(world_position_6, to_light_7, n_dot_l_4, normal_1, pixel_9, &(sun_cascade_0), &(sun_fade_0));
        }
        else
        {
            if(_S58 == u32(1))
            {
                var _S60 : u32 = _S57.shadow_tile_0;
                if((_S57.shadow_tile_0) <= u32(8))
                {
                    var _S61 : f32 = point_visibility_0(&(_S57), _S60, world_position_6, to_light_7, n_dot_l_4, normal_1, pixel_9);
                    reach_4 = reach_2 * _S61;
                }
                else
                {
                    reach_4 = reach_2;
                }
            }
            else
            {
                var _S62 : u32 = _S57.shadow_tile_0;
                if((_S57.shadow_tile_0) < u32(14))
                {
                    var _S63 : f32 = spot_visibility_0(&(_S57), _S62, world_position_6, to_light_7, n_dot_l_4, normal_1, pixel_9);
                    reach_4 = reach_2 * _S63;
                }
                else
                {
                    reach_4 = reach_2;
                }
            }
        }
        direct_0 = direct_0 + _S57.color_0.xyz * vec3<f32>((max(n_dot_l_4, 0.0f) * reach_4));
        slot_2 = slot_2 + u32(1);
    }
    return direct_0 + frame_0.ambient_0.xyz;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_7 : vec3<f32>,
    @location(1) normal_2 : vec3<f32>,
    @location(2) color_4 : vec3<f32>,
    @location(3) uv_1 : vec2<f32>,
    @location(4) level_2 : f32,
    @interpolate(flat) @location(5) row_4 : u32,
    @interpolate(flat) @location(6) patch_4 : f32,
};

@fragment
fn fragmentMain( _S64 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var _S65 : f32 = _S64.uv_1.y;
    var _S66 : vec3<f32> = vec3<f32>(_S64.uv_1.x, 1.0f - _S65, 0.0f);
    if(((textureSampleLevel((grassCard_0), (grassCardSampler_0), ((_S66)).xy, i32(((_S66)).z), (_S64.level_2))).x) < 0.5f)
    {
        discard;
    }
    var _S67 : GrassBlade_std430_0 = blades_0[_S64.row_4];
    var _S68 : vec3<f32> = grass_style_0(_S64.color_4, &(_S67), _S65, _S64.patch_4);
    var _S69 : vec3<f32> = grass_light_0(_S64.world_position_7, normalize(_S64.normal_2), position_2.xy);
    var _S70 : pixelOutput_0 = pixelOutput_0( vec4<f32>(_S68 * _S69, 1.0f) );
    return _S70;
}

fn grass_shell_count_0() -> u32
{
    return clamp(u32(field_0.stack_0.y + 0.5f), u32(1), u32(64));
}

fn grass_ground_texel_0( texel_0 : vec2<i32>) -> f32
{
    var _S71 : vec3<i32> = vec3<i32>(clamp(texel_0, vec2<i32>(i32(0), i32(0)), vec2<i32>(i32(field_0.maps_0.x) - i32(1), i32(field_0.maps_0.y) - i32(1))), i32(0));
    return (textureLoad((grassGround_0), ((_S71)).xy, ((_S71)).z).x);
}

struct GrassGround_0
{
     height_0 : f32,
     normal_3 : vec3<f32>,
};

fn grass_ground_under_0( world_1 : vec2<f32>) -> GrassGround_0
{
    var at_0 : vec2<f32> = (world_1 - field_0.ground_1.xy) * vec2<f32>(field_0.ground_1.w);
    var base_2 : vec2<f32> = floor(at_0);
    var blend_1 : vec2<f32> = at_0 - base_2;
    var _S72 : vec2<i32> = vec2<i32>(base_2);
    var h00_0 : f32 = grass_ground_texel_0(_S72);
    var h10_0 : f32 = grass_ground_texel_0(_S72 + vec2<i32>(i32(1), i32(0)));
    var h01_0 : f32 = grass_ground_texel_0(_S72 + vec2<i32>(i32(0), i32(1)));
    var h11_0 : f32 = grass_ground_texel_0(_S72 + vec2<i32>(i32(1), i32(1)));
    var _S73 : f32 = blend_1.x;
    var _S74 : f32 = h10_0 - h00_0;
    var lower_1 : f32 = h00_0 + _S73 * _S74;
    var _S75 : f32 = h11_0 - h01_0;
    var under_0 : GrassGround_0;
    under_0.height_0 = lower_1 + blend_1.y * (h01_0 + _S73 * _S75 - lower_1);
    under_0.normal_3 = normalize(vec3<f32>(- (0.5f * (_S74 + _S75)), field_0.ground_1.z, - (0.5f * (h01_0 - h00_0 + (h11_0 - h10_0)))));
    return under_0;
}

fn windSmoothTriangle_0( u_0 : f32) -> f32
{
    var s_0 : f32 = abs(fract(u_0 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}

fn windSample_0( posRel_0 : vec3<f32>) -> vec3<f32>
{
    var _S76 : vec2<f32> = posRel_0.xz;
    var deflection_0 : vec2<f32> = (textureSampleLevel((windDirectionLayer_0), (windSampler_0), (wind_0.directionUv_0 + _S76 * wind_0.directionUvPerMetre_0), (0.0f))).xy * vec2<f32>(2.0f) - vec2<f32>(1.0f);
    var intensity_0 : f32 = (textureSampleLevel((windIntensityLayer_0), (windSampler_0), (wind_0.intensityUv_0 + _S76 * wind_0.intensityUvPerMetre_0), (0.0f))).x;
    var base_3 : vec2<f32> = wind_0.baseDirection_0;
    var _S77 : f32 = wind_0.baseDirection_0.x;
    var _S78 : f32 = deflection_0.x;
    var _S79 : f32 = wind_0.baseDirection_0.y;
    var _S80 : f32 = deflection_0.y;
    var turned_0 : vec2<f32> = vec2<f32>(_S77 * _S78 - _S79 * _S80, _S77 * _S80 + _S79 * _S78);
    var lengthSquared_0 : f32 = dot(turned_0, turned_0);
    var direction_1 : vec2<f32>;
    if(lengthSquared_0 > 9.999999960041972e-13f)
    {
        direction_1 = turned_0 / vec2<f32>(sqrt(lengthSquared_0));
    }
    else
    {
        direction_1 = base_3;
    }
    var speed_0 : f32 = intensity_0 * wind_0.baseSpeed_0 * (1.0f + wind_0.gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S76, base_3) * wind_0.invGustWavelength_0 + wind_0.gustPhase_0) - 1.0f));
    return vec3<f32>(direction_1.x * speed_0, 0.0f, direction_1.y * speed_0);
}

fn grass_lean_0( velocity_0 : vec3<f32>,  height_1 : f32) -> vec3<f32>
{
    var speed_1 : f32 = length(velocity_0);
    var bend_0 : f32 = height_1 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    var along_2 : vec2<f32> = velocity_0.xz * vec2<f32>((bend_0 / max(speed_1, 9.99999997475242708e-07f)));
    var _S81 : f32 = bend_0 * bend_0;
    return vec3<f32>(along_2.x, - (_S81 / (height_1 + sqrt(max(height_1 * height_1 - _S81, 0.0f)))), along_2.y);
}

struct GrassShellVertex_0
{
    @builtin(position) position_3 : vec4<f32>,
    @location(0) world_position_8 : vec3<f32>,
    @location(1) rest_0 : vec3<f32>,
    @location(2) normal_4 : vec3<f32>,
    @location(3) footprint_0 : f32,
    @location(4) occlusion_1 : f32,
    @interpolate(flat) @location(5) fin_0 : vec4<f32>,
};

fn grass_sheet_vertex_0( rest_1 : vec2<f32>,  share_0 : f32,  fin_1 : vec4<f32>,  occlusion_2 : f32) -> GrassShellVertex_0
{
    var under_1 : GrassGround_0 = grass_ground_under_0(rest_1);
    var lift_0 : f32 = share_0 * field_0.stack_0.x;
    var _S82 : f32 = rest_1.x;
    var _S83 : f32 = rest_1.y;
    var ground_2 : vec3<f32> = vec3<f32>(_S82, under_1.height_0, _S83);
    var world_2 : vec3<f32> = ground_2 + vec3<f32>(0.0f, lift_0, 0.0f) + grass_lean_0(windSample_0(ground_2 - frame_0.camera_position_0.xyz), field_0.stack_0.x) * vec3<f32>((share_0 * share_0));
    var output_2 : GrassShellVertex_0;
    var _S84 : vec4<f32> = (((vec4<f32>(world_2, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_0[i32(0)][i32(0)], frame_0.view_proj_0.data_0[i32(1)][i32(0)], frame_0.view_proj_0.data_0[i32(2)][i32(0)], frame_0.view_proj_0.data_0[i32(3)][i32(0)], frame_0.view_proj_0.data_0[i32(0)][i32(1)], frame_0.view_proj_0.data_0[i32(1)][i32(1)], frame_0.view_proj_0.data_0[i32(2)][i32(1)], frame_0.view_proj_0.data_0[i32(3)][i32(1)], frame_0.view_proj_0.data_0[i32(0)][i32(2)], frame_0.view_proj_0.data_0[i32(1)][i32(2)], frame_0.view_proj_0.data_0[i32(2)][i32(2)], frame_0.view_proj_0.data_0[i32(3)][i32(2)], frame_0.view_proj_0.data_0[i32(0)][i32(3)], frame_0.view_proj_0.data_0[i32(1)][i32(3)], frame_0.view_proj_0.data_0[i32(2)][i32(3)], frame_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    output_2.position_3 = _S84;
    output_2.world_position_8 = world_2;
    output_2.rest_0 = vec3<f32>(_S82, lift_0, _S83);
    output_2.normal_4 = under_1.normal_3;
    output_2.footprint_0 = max(abs(_S84.w), 0.00009999999747379f) / max(grass_0.screen_0.x, 0.00009999999747379f);
    output_2.occlusion_1 = occlusion_2;
    output_2.fin_0 = fin_1;
    return output_2;
}

@vertex
fn shellVertexMain(@builtin(vertex_index) index_5 : u32, @builtin(instance_index) instance_id_1 : u32) -> GrassShellVertex_0
{
    var _S85 : u32 = grass_shell_count_0() - u32(1);
    var quad_0 : u32 = index_5 / u32(6);
    return grass_sheet_vertex_0(tile_1.tile_0.xy + (vec2<f32>(f32(quad_0 % u32(16)), f32(quad_0 / u32(16))) + GRASS_CARD_CORNERS_0[index_5 % u32(6)]) * vec2<f32>((tile_1.tile_0.z / 16.0f)), field_0.layers_0[_S85 - min(instance_id_1, _S85)].x, vec4<f32>(0.0f, 1.0f, 0.0f, 0.0f), field_0.layers_0[_S85 - min(instance_id_1, _S85)].y);
}

fn grass_occlusion_at_0( share_1 : f32) -> f32
{
    var _S86 : u32 = grass_shell_count_0();
    var occlusion_3 : f32 = field_0.layers_0[i32(0)].y;
    var shell_0 : u32 = u32(0);
    for(;;)
    {
        if(shell_0 < _S86)
        {
        }
        else
        {
            break;
        }
        if((field_0.layers_0[shell_0].x) <= share_1)
        {
            occlusion_3 = field_0.layers_0[shell_0].y;
        }
        shell_0 = shell_0 + u32(1);
    }
    return occlusion_3;
}

@vertex
fn finVertexMain(@builtin(vertex_index) index_6 : u32) -> GrassShellVertex_0
{
    var quad_1 : u32 = index_6 / u32(6);
    var _S87 : u32 = index_6 % u32(6);
    var _S88 : u32 = max(field_0.tiles_0.z / u32(4), u32(1));
    var _S89 : u32 = quad_1 / (_S88 * u32(64));
    var _S90 : u32 = min(_S89, u32(1));
    var fin_line_0 : u32 = quad_1 / u32(64) % _S88;
    var band_1 : f32 = 4.0f * field_0.origin_0.w;
    var across_1 : f32 = (f32(fin_line_0) + 0.5f) * band_1;
    var stride_0 : f32 = tile_1.tile_0.z / 16.0f;
    var _S91 : f32 = f32(quad_1 / u32(4) % u32(16));
    var along_3 : f32 = (_S91 + GRASS_CARD_CORNERS_0[_S87].x) * stride_0;
    var middle_along_0 : f32 = (_S91 + 0.5f) * stride_0;
    var share_2 : f32 = (f32(quad_1 % u32(4)) + GRASS_CARD_CORNERS_0[_S87].y) / 4.0f;
    var along_z_0 : bool = _S90 == u32(0);
    var _S92 : vec2<f32> = tile_1.tile_0.xy;
    var side_0 : vec2<f32>;
    if(along_z_0)
    {
        side_0 = vec2<f32>(across_1, along_3);
    }
    else
    {
        side_0 = vec2<f32>(along_3, across_1);
    }
    var rest_2 : vec2<f32> = _S92 + side_0;
    var _S93 : vec2<f32> = tile_1.tile_0.xy;
    if(along_z_0)
    {
        side_0 = vec2<f32>(across_1, middle_along_0);
    }
    else
    {
        side_0 = vec2<f32>(middle_along_0, across_1);
    }
    var middle_0 : vec2<f32> = _S93 + side_0;
    if(along_z_0)
    {
        side_0 = vec2<f32>(0.5f * band_1, 0.0f);
    }
    else
    {
        side_0 = vec2<f32>(0.0f, 0.5f * band_1);
    }
    var eye_0 : vec3<f32> = frame_0.camera_position_0.xyz - vec3<f32>(middle_0.x, grass_ground_under_0(middle_0).height_0 + 0.5f * field_0.stack_0.x, middle_0.y);
    var view_0 : vec3<f32> = eye_0 / vec3<f32>(max(length(eye_0), 9.99999997475242708e-07f));
    var facing_near_0 : f32 = dot(grass_ground_under_0(middle_0 - side_0).normal_3, view_0);
    var facing_far_0 : f32 = dot(grass_ground_under_0(middle_0 + side_0).normal_3, view_0);
    var graze_0 : f32;
    if((facing_near_0 * facing_far_0) <= 0.0f)
    {
        graze_0 = 0.0f;
    }
    else
    {
        graze_0 = min(abs(facing_near_0), abs(facing_far_0));
    }
    var fade_1 : f32 = saturate((0.5f - graze_0) / 0.19999998807907104f);
    if(along_z_0)
    {
        graze_0 = tile_1.tile_0.x;
    }
    else
    {
        graze_0 = tile_1.tile_0.y;
    }
    var output_3 : GrassShellVertex_0 = grass_sheet_vertex_0(rest_2, share_2, vec4<f32>(f32(_S90) + 1.0f, fade_1, graze_0 + across_1, 0.0f), grass_occlusion_at_0(share_2));
    var _S94 : bool;
    if(fade_1 <= 0.0f)
    {
        _S94 = true;
    }
    else
    {
        _S94 = (field_0.stack_0.z) <= 0.0f;
    }
    if(_S94)
    {
        output_3.position_3 = vec4<f32>(0.0f, 0.0f, -1.0f, 1.0f);
    }
    return output_3;
}

struct GrassInstance_0
{
     root_0 : vec4<f32>,
     facing_0 : vec4<f32>,
     lean_0 : vec4<f32>,
     ground_0 : vec4<f32>,
     lanes_0 : vec4<u32>,
};

fn grass_no_blade_0() -> GrassInstance_0
{
    var none_0 : GrassInstance_0;
    const _S95 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    none_0.root_0 = _S95;
    none_0.facing_0 = _S95;
    none_0.lean_0 = _S95;
    none_0.ground_0 = _S95;
    none_0.lanes_0 = vec4<u32>(u32(0), u32(0), u32(0), u32(0));
    return none_0;
}

fn grass_cell_0( global_0 : vec2<i32>) -> GrassInstance_0
{
    var side_1 : i32 = i32(max(field_0.tiles_0.z, u32(1)));
    var extent_0 : vec2<i32> = vec2<i32>(i32(field_0.tiles_0.x), i32(field_0.tiles_0.y)) * vec2<i32>(side_1);
    var _S96 : i32 = global_0.x;
    var _S97 : bool;
    if(_S96 < i32(0))
    {
        _S97 = true;
    }
    else
    {
        _S97 = (global_0.y) < i32(0);
    }
    if(_S97)
    {
        _S97 = true;
    }
    else
    {
        _S97 = _S96 >= (extent_0.x);
    }
    if(_S97)
    {
        _S97 = true;
    }
    else
    {
        _S97 = (global_0.y) >= (extent_0.y);
    }
    if(_S97)
    {
        return grass_no_blade_0();
    }
    var _S98 : vec2<u32> = vec2<u32>(global_0);
    var cells_0 : u32 = u32(side_1);
    var _S99 : u32 = _S98.y;
    var _S100 : u32 = _S99 / cells_0;
    var _S101 : u32 = _S100 * field_0.tiles_0.x;
    var _S102 : u32 = _S98.x;
    var _S103 : u32 = _S102 / cells_0;
    var slot_3 : u32 = _S101 + _S103;
    var _S104 : u32 = _S99 % cells_0;
    var _S105 : u32 = _S104 * cells_0;
    var _S106 : u32 = _S102 % cells_0;
    var _S107 : GrassInstance_std430_0 = grassCells_0[slot_3 * field_0.tiles_0.w + (_S105 + _S106)];
    var _S108 : GrassInstance_0 = GrassInstance_0( _S107.root_0, _S107.facing_0, _S107.lean_0, _S107.ground_0, _S107.lanes_0 );
    return _S108;
}

fn grass_strand_reach_0( blade_1 : GrassInstance_0,  lift_1 : f32,  floor_reach_0 : f32,  fade_2 : f32) -> f32
{
    var _S109 : f32 = blade_1.root_0.w;
    var _S110 : bool;
    if(!(lift_1 < _S109))
    {
        _S110 = true;
    }
    else
    {
        _S110 = (grass_row_0(blade_1.lanes_0.y).flags_0.x) != u32(1);
    }
    if(_S110)
    {
        return -1.0f;
    }
    var bound_0 : f32 = 0.5f * field_0.origin_0.w;
    return min(max(min(blade_1.facing_0.z, bound_0) * (1.0f - lift_1 / _S109), floor_reach_0), bound_0) * fade_2;
}

struct pixelOutput_1
{
    @location(0) output_4 : vec4<f32>,
};

struct pixelInput_1
{
    @location(0) world_position_9 : vec3<f32>,
    @location(1) rest_3 : vec3<f32>,
    @location(2) normal_5 : vec3<f32>,
    @location(3) footprint_1 : f32,
    @location(4) occlusion_4 : f32,
    @interpolate(flat) @location(5) fin_2 : vec4<f32>,
};

@fragment
fn shellFragmentMain( _S111 : pixelInput_1, @builtin(position) position_4 : vec4<f32>) -> pixelOutput_1
{
    var cell_2 : f32 = field_0.origin_0.w;
    var _S112 : vec2<f32> = _S111.rest_3.xz;
    var from_corner_0 : vec2<f32> = (_S112 - field_0.origin_0.xy) / vec2<f32>(cell_2);
    var _S113 : vec2<f32> = floor(from_corner_0);
    var _S114 : vec2<i32> = vec2<i32>(_S113);
    var inside_0 : vec2<f32> = from_corner_0 - _S113;
    var _S115 : i32;
    if((inside_0.x) < 0.5f)
    {
        _S115 = i32(-1);
    }
    else
    {
        _S115 = i32(1);
    }
    var band_2 : i32;
    if((inside_0.y) < 0.5f)
    {
        band_2 = i32(-1);
    }
    else
    {
        band_2 = i32(1);
    }
    var lift_2 : f32 = _S111.rest_3.y;
    var _S116 : f32 = 0.5f * _S111.footprint_1;
    var _S117 : f32 = _S111.fin_2.y;
    var _S118 : GrassInstance_0 = grass_no_blade_0();
    var _S119 : f32 = _S111.fin_2.x;
    var dz_0 : i32;
    var dx_0 : i32;
    var nearest_0 : f32;
    var nearest_1 : f32;
    var found_2 : GrassInstance_0;
    var found_3 : GrassInstance_0;
    var _S120 : bool;
    if(_S119 < 0.5f)
    {
        nearest_1 = 2.0f;
        found_3 = _S118;
        dz_0 = i32(0);
        for(;;)
        {
            if(dz_0 < i32(2))
            {
            }
            else
            {
                break;
            }
            nearest_0 = nearest_1;
            found_2 = found_3;
            dx_0 = i32(0);
            for(;;)
            {
                if(dx_0 < i32(2))
                {
                }
                else
                {
                    break;
                }
                var blade_2 : GrassInstance_0 = grass_cell_0(_S114 + vec2<i32>(dx_0 * _S115, dz_0 * band_2));
                var reach_5 : f32 = grass_strand_reach_0(blade_2, lift_2, _S116, _S117);
                var offset_1 : vec2<f32> = _S112 - blade_2.root_0.xz;
                var apart_0 : f32 = sqrt(dot(offset_1, offset_1)) / max(reach_5, 9.99999997475242708e-07f);
                if(reach_5 > 0.0f)
                {
                    _S120 = apart_0 < nearest_0;
                }
                else
                {
                    _S120 = false;
                }
                if(_S120)
                {
                    nearest_0 = apart_0;
                    found_2 = blade_2;
                }
                dx_0 = dx_0 + i32(1);
            }
            var dz_1 : i32 = dz_0 + i32(1);
            nearest_1 = nearest_0;
            found_3 = found_2;
            dz_0 = dz_1;
        }
        nearest_0 = nearest_1;
        found_2 = found_3;
    }
    else
    {
        var along_z_1 : bool = _S119 < 1.5f;
        var _S121 : f32 = _S111.fin_2.z;
        if(along_z_1)
        {
            nearest_0 = field_0.origin_0.x;
        }
        else
        {
            nearest_0 = field_0.origin_0.y;
        }
        var _S122 : i32 = i32(floor((_S121 - nearest_0) / cell_2 + 0.5f)) - i32(2);
        if(along_z_1)
        {
            dz_0 = _S114.y;
        }
        else
        {
            dz_0 = _S114.x;
        }
        if(along_z_1)
        {
            _S115 = band_2;
        }
        nearest_0 = 2.0f;
        found_2 = _S118;
        band_2 = i32(0);
        for(;;)
        {
            if(band_2 < i32(4))
            {
            }
            else
            {
                break;
            }
            nearest_1 = nearest_0;
            found_3 = found_2;
            dx_0 = i32(0);
            for(;;)
            {
                if(dx_0 < i32(2))
                {
                }
                else
                {
                    break;
                }
                var along_4 : i32 = dz_0 + dx_0 * _S115;
                var _S123 : vec2<i32>;
                if(along_z_1)
                {
                    _S123 = vec2<i32>(_S122 + band_2, along_4);
                }
                else
                {
                    _S123 = vec2<i32>(along_4, _S122 + band_2);
                }
                var blade_3 : GrassInstance_0 = grass_cell_0(_S123);
                var reach_6 : f32 = grass_strand_reach_0(blade_3, lift_2, _S116, _S117);
                var offset_2 : f32;
                if(along_z_1)
                {
                    offset_2 = _S111.rest_3.z - blade_3.root_0.z;
                }
                else
                {
                    offset_2 = _S111.rest_3.x - blade_3.root_0.x;
                }
                var apart_1 : f32 = abs(offset_2) / max(reach_6, 9.99999997475242708e-07f);
                if(reach_6 > 0.0f)
                {
                    _S120 = apart_1 < nearest_1;
                }
                else
                {
                    _S120 = false;
                }
                if(_S120)
                {
                    nearest_1 = apart_1;
                    found_3 = blade_3;
                }
                dx_0 = dx_0 + i32(1);
            }
            var band_3 : i32 = band_2 + i32(1);
            nearest_0 = nearest_1;
            found_2 = found_3;
            band_2 = band_3;
        }
    }
    if(!(nearest_0 < 1.0f))
    {
        discard;
    }
    var row_5 : GrassBlade_0 = grass_row_0(found_2.lanes_0.y);
    var along_blade_0 : f32 = saturate(lift_2 / found_2.root_0.w);
    var color_5 : vec3<f32> = mix(row_5.root_color_0.xyz, row_5.tip_color_0.xyz, vec3<f32>(along_blade_0)) * vec3<f32>((1.0f - 0.18000000715255737f * found_2.facing_0.w));
    var _S124 : f32 = grass_patch_0(found_2.lanes_0.x);
    var color_6 : vec3<f32> = grass_style_1(color_5, row_5, along_blade_0, _S124) * vec3<f32>(_S111.occlusion_4);
    var normal_6 : vec3<f32>;
    if((row_5.flags_0.y) == u32(1))
    {
        normal_6 = vec3<f32>(0.0f, 1.0f, 0.0f);
    }
    else
    {
        normal_6 = normalize(_S111.normal_5);
    }
    var _S125 : vec3<f32> = grass_light_0(_S111.world_position_9, normal_6, position_4.xy);
    var _S126 : pixelOutput_1 = pixelOutput_1( vec4<f32>(color_6 * _S125, 1.0f) );
    return _S126;
}

