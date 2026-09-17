#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 743 "shaders/grass.slang"
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 697
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 718
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 731
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 885
constant array<float2, int(6)> GRASS_CARD_CORNERS_0 = { float2(0.0f, 0.0f), float2(1.0f, 0.0f), float2(0.0f, 1.0f), float2(1.0f, 0.0f), float2(1.0f, 1.0f), float2(0.0f, 1.0f) };

#line 493
struct GrassBlade_0
{
    float4 root_color_0;
    float4 tip_color_0;
    float4 size_0;
    float4 occlusion_0;
    float4 glow_0;
    float4 patch_0;
    float4 shape_0;
    float4 clump_0;
    uint4 flags_0;
};


#line 478
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 463
struct GrassParams_0
{
    uint4 limits_0;
    float4 screen_0;
};


#line 463
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_0;
    packed_float4 clump_1;
    packed_uint4 lanes_0;
};


#line 463
struct GrassBlade_natural_0
{
    packed_float4 root_color_0;
    packed_float4 tip_color_0;
    packed_float4 size_0;
    packed_float4 occlusion_0;
    packed_float4 glow_0;
    packed_float4 patch_0;
    packed_float4 shape_0;
    packed_float4 clump_0;
    packed_uint4 flags_0;
};


#line 463
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 463
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 463
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 158
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 view_proj_0;
    float4 camera_position_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint4 cluster_grid_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_0;
    uint4 probe_counts_0;
    uint4 probe_levels_0;
    array<float4, int(4)> probe_level_origin_0;
    array<float4, int(4)> probe_level_inv_spacing_0;
    array<uint4, int(4)> probe_level_offset_0;
    float4 lod_params_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sky_sh_r_0;
    float4 sky_sh_g_0;
    float4 sky_sh_b_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_view_proj_0;
    uint4 vertex_pool_0;
    array<float4, int(16)> shadow_atlas_rect_0;
    uint4 shadow_filter_0;
};


#line 612
struct GrassField_0
{
    float4 origin_0;
    uint4 tiles_0;
    float4 ground_1;
    uint4 maps_0;
    float4 stack_0;
    float4 blades_0;
    array<float4, int(64)> layers_0;
};


#line 1548
struct GpuLight_natural_0
{
    packed_float4 position_0;
    packed_float4 color_0;
    packed_float4 direction_0;
    packed_float4 tangent_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint flags_1;
};


#line 652
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


#line 652
struct KernelContext_0
{
    GrassTile_0 constant* tile_1;
    GrassParams_0 constant* grass_0;
    GrassInstance_natural_0 device* instances_0;
    GrassBlade_natural_0 device* blades_1;
    FrameUniforms_natural_0 constant* frame_0;
    GrassField_0 constant* field_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d_array<float, access::sample> grassCard_0;
    sampler grassCardSampler_0;
    texture2d<float, access::sample> grassGround_0;
    WindParams_0 constant* wind_0;
    texture2d<float, access::sample> windDirectionLayer_0;
    sampler windSampler_0;
    texture2d<float, access::sample> windIntensityLayer_0;
    GrassInstance_natural_0 device* grassCells_0;
};


#line 1508
GrassBlade_0 grass_row_0(uint index_0, KernelContext_0 thread* kernelContext_0)
{
    GrassBlade_natural_0 _S1 = kernelContext_0->blades_1[min(index_0, max(kernelContext_0->grass_0->limits_0.y, 1U) - 1U)];

#line 1510
    GrassBlade_0 _S2 = { float4(_S1.root_color_0) , float4(_S1.tip_color_0) , float4(_S1.size_0) , float4(_S1.occlusion_0) , float4(_S1.glow_0) , float4(_S1.patch_0) , float4(_S1.shape_0) , float4(_S1.clump_0) , uint4(_S1.flags_0)  };

#line 1510
    return _S2;
}


#line 815
float grassCardLevel_0(float pixels_0)
{
    float _S3 = 64.0f / max(pixels_0, 0.00009999999747379f);

#line 817
    float level_0 = 0.0f;

#line 817
    uint step_0 = 1U;

    for(;;)
    {

#line 819
        if(step_0 < 7U)
        {
        }
        else
        {

#line 819
            break;
        }
        uint _S4 = step_0 - 1U;

#line 821
        float lower_0 = float(1U << _S4);
        if(_S3 >= lower_0)
        {

#line 822
            level_0 = float(_S4) + min((_S3 - lower_0) / lower_0, 1.0f);

#line 822
        }

#line 819
        step_0 = step_0 + 1U;

#line 819
    }

#line 835
    return min(floor(level_0 + 0.5f), 6.0f);
}


#line 1447
uint grass_hash_0(uint value_0)
{
    uint state_0 = value_0 * 747796405U + 2891336453U;
    uint word_0 = ((state_0 >> ((state_0 >> 28U) + 4U)) ^ state_0) * 277803737U;
    return (word_0 >> 22U) ^ word_0;
}


float2 grass_unit_pair_0(uint lane_0)
{
    return float2(float(lane_0 & 65535U), float((lane_0 >> 16U) & 65535U)) * float2(0.0000152587890625f) ;
}


#line 1502
float grass_patch_0(uint clump_2)
{
    return grass_unit_pair_0(grass_hash_0(clump_2 ^ 374761393U)).x;
}


#line 1733
float2 grass_blade_lod_0(float distance_0, KernelContext_0 thread* kernelContext_1)
{
    float band_0 = kernelContext_1->field_0->blades_0.y;
    if(!(band_0 > 0.0f))
    {
        return float2(0.0f, 0.0f);
    }
    float half_band_0 = 0.5f * band_0;

    float _S5 = distance_0 - (kernelContext_1->field_0->blades_0.x - band_0);

#line 1742
    return float2(saturate(_S5 / half_band_0), saturate((_S5 - half_band_0) / half_band_0));
}


#line 1636
struct GrassBladeCurve_0
{
    float3 p0_0;
    float3 p1_0;
    float3 p2_0;
    float3 p3_0;
};


#line 1654
GrassBladeCurve_0 grass_blade_curve_0(const GrassInstance_natural_0 thread* blade_0, const GrassBlade_0 thread* row_0)
{

#line 1654
    float4 _S6 = float4(blade_0->root_0) ;

    float height_0 = _S6.w;

#line 1656
    float4 _S7 = float4(blade_0->facing_0) ;
    float _S8 = _S7.x;

#line 1657
    float _S9 = _S7.y;
    float tilt_0 = row_0->shape_0.x;
    float upright_0 = sqrt(max(1.0f - tilt_0 * tilt_0, 0.0f));
    float3 chord_0 = float3(0.0f, height_0 * upright_0, 0.0f) + float3(_S8, 0.0f, _S9) * float3((height_0 * tilt_0)) ;



    float3 bow_0 = float3(_S8 * upright_0, - tilt_0, _S9 * upright_0) * float3((row_0->shape_0.y * height_0)) ;

    thread GrassBladeCurve_0 curve_0;
    float3 _S10 = _S6.xyz;

#line 1667
    (&curve_0)->p0_0 = _S10;

#line 1667
    float3 _S11 = float3(0.3333333432674408f) ;
    (&curve_0)->p1_0 = _S10 + chord_0 * _S11 - bow_0;
    float3 _S12 = (float4(blade_0->lean_0) ).xyz;

#line 1669
    (&curve_0)->p2_0 = _S10 + chord_0 * float3(0.66666668653488159f)  - bow_0 + _S12 * _S11;
    (&curve_0)->p3_0 = _S10 + chord_0 + _S12;
    return curve_0;
}


struct GrassBladePoint_0
{
    float3 position_1;
    float3 normal_0;
};


#line 1691
GrassBladePoint_0 grass_blade_point_0(const GrassBladeCurve_0 thread* curve_1, const GrassInstance_natural_0 thread* blade_1, const GrassBlade_0 thread* row_1, float t_0, float edge_0, float scale_0, KernelContext_0 thread* kernelContext_2)
{

    float u_0 = 1.0f - t_0;
    float _S13 = 3.0f * u_0;

#line 1695
    float _S14 = _S13 * u_0;
    float _S15 = t_0 * t_0;

#line 1696
    float3 centre_0 = curve_1->p0_0 * float3((u_0 * u_0 * u_0))  + curve_1->p1_0 * float3((_S14 * t_0))  + curve_1->p2_0 * float3((_S13 * t_0 * t_0))  + curve_1->p3_0 * float3((_S15 * t_0)) ;

    float3 tangent_1 = (curve_1->p1_0 - curve_1->p0_0) * float3(_S14)  + (curve_1->p2_0 - curve_1->p1_0) * float3((6.0f * u_0 * t_0))  + (curve_1->p3_0 - curve_1->p2_0) * float3((3.0f * t_0 * t_0)) ;

#line 1698
    float4 _S16 = float4(blade_1->facing_0) ;
    float3 side_0 = float3(- _S16.y, 0.0f, _S16.x);

    float3 to_eye_0 = kernelContext_2->frame_0->camera_position_0.xyz - centre_0;
    float3 view_0 = to_eye_0 / float3(max(length(to_eye_0), 9.99999997475242708e-07f)) ;
    float3 perpendicular_0 = cross(tangent_1, view_0);
    float perpendicular_length_0 = length(perpendicular_0);

#line 1704
    float3 screen_side_0;
    if(perpendicular_length_0 > 9.99999997475242708e-07f)
    {

#line 1705
        screen_side_0 = perpendicular_0 / float3(perpendicular_length_0) ;

#line 1705
    }
    else
    {

#line 1705
        screen_side_0 = side_0;

#line 1705
    }
    if((dot(screen_side_0, side_0)) < 0.0f)
    {

#line 1706
        screen_side_0 = - screen_side_0;

#line 1706
    }
    float along_0 = dot(side_0, view_0);
    float side_seen_0 = sqrt(saturate(1.0f - along_0 * along_0));
    float3 direction_1 = side_0 * float3(side_seen_0)  + screen_side_0 * float3((1.0f - side_seen_0)) ;
    float seen_0 = length(direction_1 - view_0 * float3(dot(direction_1, view_0)) );

#line 1715
    float half_width_0 = _S16.z * (1.0f - _S15);
    float least_0 = 0.5f * (max(abs((((float4(centre_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w), 0.00009999999747379f) / max(kernelContext_2->grass_0->screen_0.x, 0.00009999999747379f));

#line 1716
    float widened_0;
    if((half_width_0 * seen_0) < least_0)
    {

#line 1717
        widened_0 = least_0 / max(seen_0, 0.00009999999747379f);

#line 1717
    }
    else
    {

#line 1717
        widened_0 = half_width_0;

#line 1717
    }

    thread GrassBladePoint_0 here_0;
    (&here_0)->position_1 = centre_0 + direction_1 * float3((edge_0 * widened_0 * scale_0)) ;


    float3 normal_1 = cross(tangent_1, side_0);

    (&here_0)->normal_0 = normalize(normal_1 / float3(max(length(normal_1), 9.99999997475242708e-07f))  + side_0 * float3((edge_0 * row_1->shape_0.z)) );
    return here_0;
}


#line 1611
struct GrassVertex_0
{
    float4 position_2;
    float3 world_position_0;
    float3 normal_2;
    float3 color_1;
    float2 uv_0;
    float level_1;
    [[flat]] uint row_2;
    [[flat]] float patch_1;
};


#line 1755
GrassVertex_0 grass_blade_vertex_0(const GrassInstance_natural_0 thread* blade_2, uint index_1, uint segments_0, float2 lod_0, KernelContext_0 thread* kernelContext_3)
{

#line 1755
    uint4 _S17 = uint4(blade_2->lanes_0) ;

    uint _S18 = _S17.y;

#line 1757
    GrassBlade_0 _S19 = grass_row_0(_S18, kernelContext_3);

#line 1757
    thread GrassBlade_0 _S20 = _S19;

#line 1757
    GrassBladeCurve_0 _S21 = grass_blade_curve_0(blade_2, &_S20);


    float drop_0 = lod_0.x;
    float morph_0 = lod_0.y;

#line 1761
    float scale_1;


    if((_S17.w) != 0U)
    {

#line 1764
        scale_1 = 1.0f + drop_0;

#line 1764
    }
    else
    {

#line 1764
        scale_1 = 1.0f - drop_0;

#line 1764
    }

    bool tip_0 = index_1 >= (2U * segments_0);
    uint _S22 = min(index_1 / 2U, segments_0);

#line 1767
    float edge_1;
    if(tip_0)
    {

#line 1768
        edge_1 = 0.0f;

#line 1768
    }
    else
    {

#line 1768
        if((index_1 % 2U) == 0U)
        {

#line 1768
            edge_1 = -1.0f;

#line 1768
        }
        else
        {

#line 1768
            edge_1 = 1.0f;

#line 1768
        }

#line 1768
    }

#line 1768
    float t_1;
    if(tip_0)
    {

#line 1769
        t_1 = 1.0f;

#line 1769
    }
    else
    {

#line 1769
        t_1 = float(_S22) / float(segments_0);

#line 1769
    }

#line 1769
    thread GrassBladeCurve_0 _S23 = _S21;

#line 1769
    thread GrassBlade_0 _S24 = _S19;

#line 1769
    GrassBladePoint_0 _S25 = grass_blade_point_0(&_S23, blade_2, &_S24, t_1, edge_1, scale_1, kernelContext_3);
    thread GrassBladePoint_0 here_1 = _S25;

#line 1770
    bool _S26;

    if(morph_0 > 0.0f)
    {

#line 1772
        _S26 = !tip_0;

#line 1772
    }
    else
    {

#line 1772
        _S26 = false;

#line 1772
    }

#line 1772
    if(_S26)
    {

#line 1772
        _S26 = segments_0 != 3U;

#line 1772
    }
    else
    {

#line 1772
        _S26 = false;

#line 1772
    }

#line 1772
    if(_S26)
    {
        uint scaled_0 = _S22 * 3U;
        uint lower_1 = scaled_0 / segments_0;
        float within_0 = float(scaled_0 - lower_1 * segments_0) / float(segments_0);

        float _S27 = float(lower_1) / 3.0f;

#line 1778
        thread GrassBladeCurve_0 _S28 = _S21;

#line 1778
        thread GrassBlade_0 _S29 = _S19;

#line 1778
        GrassBladePoint_0 _S30 = grass_blade_point_0(&_S28, blade_2, &_S29, _S27, edge_1, scale_1, kernelContext_3);

        float _S31 = float(lower_1 + 1U) / 3.0f;

#line 1780
        thread GrassBladeCurve_0 _S32 = _S21;

#line 1780
        thread GrassBlade_0 _S33 = _S19;

#line 1780
        GrassBladePoint_0 _S34 = grass_blade_point_0(&_S32, blade_2, &_S33, _S31, edge_1, scale_1, kernelContext_3);

#line 1780
        float3 _S35 = float3((1.0f - within_0)) ;

#line 1780
        float3 _S36 = float3(within_0) ;

        float3 far_normal_0 = _S30.normal_0 * _S35 + _S34.normal_0 * _S36;

#line 1782
        float3 _S37 = float3((1.0f - morph_0)) ;

#line 1782
        float3 _S38 = float3(morph_0) ;
        (&here_1)->position_1 = (&here_1)->position_1 * _S37 + (_S30.position_1 * _S35 + _S34.position_1 * _S36) * _S38;
        (&here_1)->normal_0 = (&here_1)->normal_0 * _S37 + far_normal_0 * _S38;

#line 1772
    }

#line 1772
    float3 blade_normal_0;

#line 1789
    if((dot((&here_1)->normal_0, kernelContext_3->frame_0->camera_position_0.xyz - (&here_1)->position_1)) < 0.0f)
    {

#line 1789
        blade_normal_0 = - (&here_1)->normal_0;

#line 1789
    }
    else
    {

#line 1789
        blade_normal_0 = (&here_1)->normal_0;

#line 1789
    }

#line 1789
    float3 base_0;



    if((_S19.flags_0.y) == 1U)
    {

#line 1793
        base_0 = float3(0.0f, 1.0f, 0.0f);

#line 1793
    }
    else
    {

#line 1793
        base_0 = (float4(blade_2->ground_0) ).xyz;

#line 1793
    }

#line 1793
    float4 _S39 = float4(blade_2->clump_1) ;


    float3 clump_normal_0 = normalize(base_0 + float3(_S39.x, 0.0f, _S39.y) * float3((0.5f / max(8.0f * kernelContext_3->field_0->origin_0.w, 0.00009999999747379f))) );

    thread GrassVertex_0 output_0;
    (&output_0)->position_2 = (((float4((&here_1)->position_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_3->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_3->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_3->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_3->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_3->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_3->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_3->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_3->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_3->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_3->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_3->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_3->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_3->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_3->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_3->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_3->frame_0->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_0)->world_position_0 = (&here_1)->position_1;
    (&output_0)->normal_2 = normalize(normalize(blade_normal_0) * float3((1.0f - morph_0))  + clump_normal_0 * float3(morph_0) );

    (&output_0)->color_1 = mix(_S19.root_color_0.xyz, _S19.tip_color_0.xyz, float3(t_1) ) * float3((1.0f - 0.18000000715255737f * (float4(blade_2->facing_0) ).w)) ;
    (&output_0)->uv_0 = float2(0.5f + 0.5f * edge_1, t_1);
    (&output_0)->level_1 = 0.0f;
    (&output_0)->row_2 = min(_S18, max(kernelContext_3->grass_0->limits_0.y, 1U) - 1U);
    (&output_0)->patch_1 = grass_patch_0(_S17.z);
    return output_0;
}


#line 1520
float3 grass_style_0(float3 color_2, const GrassBlade_0 thread* row_3, float along_1, float patch_2)
{

#line 1520
    float4 _S40 = row_3->occlusion_0;

    float reach_0 = row_3->occlusion_0.w;

#line 1522
    float occluded_0;
    if(reach_0 > 0.0f)
    {

#line 1523
        occluded_0 = saturate(1.0f - along_1 / reach_0);

#line 1523
    }
    else
    {

#line 1523
        occluded_0 = 0.0f;

#line 1523
    }


    float start_0 = row_3->glow_0.w;

    return mix(mix(color_2, _S40.xyz, float3(occluded_0) ), row_3->patch_0.xyz, float3((row_3->patch_0.w * patch_2)) ) + row_3->glow_0.xyz * float3(saturate((along_1 - start_0) / max(1.0f - start_0, 0.00009999999747379f))) ;
}


#line 1374
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_4)
{
    uint _S41 = max(kernelContext_4->frame_0->cluster_grid_0.x, 1U);
    uint _S42 = max(kernelContext_4->frame_0->cluster_grid_0.y, 1U);
    uint _S43 = max(kernelContext_4->frame_0->cluster_grid_0.z, 1U);
    uint _S44 = max(kernelContext_4->frame_0->cluster_grid_0.w, 1U);

#line 1384
    uint _S45 = uint(pixel_0.x) / _S44;

#line 1384
    uint _S46 = min(_S45, _S41 - 1U);
    uint _S47 = uint(pixel_0.y) / _S44;

    float scale_2 = 24.0f / log2(10000.0f);

#line 1395
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_2 + - scale_2 * log2(0.10000000149011612f)), 0.0f, float(_S43 - 1U))) * _S42 + min(_S47, _S42 - 1U)) * _S41 + _S46;
}


#line 1352
float range_window_0(float distance_1, float radius_0)
{
    float ratio_0 = distance_1 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}

float punctual_falloff_0(float distance_2, float radius_1)
{
    return range_window_0(distance_2, radius_1) / (distance_2 * distance_2 + 1.0f);
}

float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 1371
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 918
float4 atlas_rect_0(uint tile_2, KernelContext_0 thread* kernelContext_5)
{
    return kernelContext_5->frame_0->shadow_atlas_rect_0[tile_2];
}


#line 918
float4 atlas_rect_1(uint tile_3, KernelContext_0 thread* kernelContext_6)
{
    return kernelContext_6->frame_0->shadow_atlas_rect_0[tile_3];
}


#line 933
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 928
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_7)
{
    return rect_1.x / kernelContext_7->frame_0->shadow_params_0.x;
}


#line 895
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 907
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_8)
{

#line 907
    uint _S48;

    if(uint(pixel_1.x) < (kernelContext_8->frame_0->shadow_filter_0.z))
    {

#line 909
        _S48 = kernelContext_8->frame_0->shadow_filter_0.x;

#line 909
    }
    else
    {

#line 909
        _S48 = kernelContext_8->frame_0->shadow_filter_0.y;

#line 909
    }

#line 909
    return _S48;
}


#line 923
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_9)
{
    return kernelContext_9->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 923
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 913
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 938
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_11)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S49 = spoke_0.x;

#line 943
    float _S50 = rotation_0.x;

#line 943
    float _S51 = spoke_0.y;

#line 943
    float _S52 = rotation_0.y;


    float _S53 = ((kernelContext_11->shadow_atlas_0).sample_compare((kernelContext_11->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S49 * _S50 - _S51 * _S52, _S49 * _S52 + _S51 * _S50) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 946
    return _S53;
}


#line 987
float tile_box_pcf_0(uint tile_4, float2 tile_uv_2, float reference_1, KernelContext_0 thread* kernelContext_12)
{

#line 987
    float4 _S54 = atlas_rect_1(tile_4, kernelContext_12);


    if(atlas_rect_is_empty_0(_S54))
    {
        return 1.0f;
    }

#line 992
    float2 _S55 = atlas_step_1(_S54, kernelContext_12);

#line 992
    int y_0 = int(-1);

#line 992
    float visibility_0 = 0.0f;

#line 997
    for(;;)
    {

#line 997
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 997
            break;
        }

#line 997
        int x_0 = int(-1);

        for(;;)
        {

#line 999
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 999
                break;
            }

#line 999
            float _S56 = tile_tap_0(_S54, _S55, tile_uv_2, float2(float(x_0), float(y_0)), float2(1.0f, 0.0f), reference_1, kernelContext_12);

            float visibility_1 = visibility_0 + _S56;

#line 999
            x_0 = x_0 + int(1);

#line 999
            visibility_0 = visibility_1;

#line 999
        }

#line 997
        y_0 = y_0 + int(1);

#line 997
    }

#line 1005
    return visibility_0 / 9.0f;
}


#line 901
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 949
float tile_pcf_0(uint tile_5, float2 tile_uv_3, float reference_2, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_13)
{
    float2 _S57 = shadow_rotation_0(pixel_3);

#line 951
    float4 _S58 = atlas_rect_1(tile_5, kernelContext_13);

    if(atlas_rect_is_empty_0(_S58))
    {
        return 1.0f;
    }

#line 955
    float2 _S59 = atlas_step_1(_S58, kernelContext_13);

#line 955
    uint spot_0 = 0U;

#line 955
    float probe_0 = 0.0f;

#line 960
    for(;;)
    {

#line 960
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 960
            break;
        }

#line 960
        float _S60 = tile_tap_0(_S58, _S59, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , _S57, reference_2, kernelContext_13);

        float probe_1 = probe_0 + _S60;

#line 960
        spot_0 = spot_0 + 1U;

#line 960
        probe_0 = probe_1;

#line 960
    }

#line 969
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 975
    uint index_2 = 0U;

#line 975
    float visibility_2 = 0.0f;



    for(;;)
    {

#line 979
        if(index_2 < 32U)
        {
        }
        else
        {

#line 979
            break;
        }

#line 979
        float _S61 = tile_tap_0(_S58, _S59, tile_uv_3, SHADOW_DISC_0[index_2] * float2(radius_2) , _S57, reference_2, kernelContext_13);

        float visibility_3 = visibility_2 + _S61;

#line 979
        index_2 = index_2 + 1U;

#line 979
        visibility_2 = visibility_3;

#line 979
    }

#line 984
    return visibility_2 / 32.0f;
}


#line 1008
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_1, KernelContext_0 thread* kernelContext_14)
{
    float2 texel_0 = kernelContext_14->frame_0->shadow_params_0.xy;

#line 1010
    float4 _S62 = atlas_rect_0(cascade_0, kernelContext_14);

#line 1010
    float2 _S63 = atlas_step_0(_S62, kernelContext_14);


    float2 _S64 = float2(0.5f, 0.5f) * _S63;


    float2 _S65 = float2(1.0f, 1.0f);

#line 1016
    float2 _S66 = _S65 / texel_0;

#line 1016
    uint index_3 = 0U;

#line 1016
    float sum_0 = 0.0f;

#line 1016
    float found_0 = 0.0f;



    for(;;)
    {

#line 1020
        if(index_3 < 16U)
        {
        }
        else
        {

#line 1020
            break;
        }
        float2 spoke_1 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S67 = spoke_1.x;

#line 1023
        float _S68 = rotation_1.x;

#line 1023
        float _S69 = spoke_1.y;

#line 1023
        float _S70 = rotation_1.y;

#line 1031
        int3 _S71 = int3(int2(min(atlas_uv_0(_S62, clamp(tile_uv_4 + float2(_S67 * _S68 - _S69 * _S70, _S67 * _S70 + _S69 * _S68) * _S63, _S64, float2(1.0f)  - _S64)) * _S66, _S66 - _S65)), int(0));

#line 1031
        float depth_1 = ((kernelContext_14->shadow_atlas_0).read(vec<uint,2>(((_S71)).xy), uint(((_S71)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 1035
            sum_0 = sum_0 + depth_1;

#line 1035
            found_0 = found_1;

#line 1032
        }

#line 1020
        index_3 = index_3 + 1U;

#line 1020
    }

#line 1039
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 1050
    float _S72 = 2.0f * kernelContext_14->frame_0->cascade_far_0[cascade_0];

#line 1050
    float separation_0 = (sum_0 / found_0 - reference_3) * (_S72 + 40.0f);

#line 1050
    float _S73 = tile_texels_0(_S62, kernelContext_14);

    return clamp(separation_0 * 0.01999999955296516f / (_S72 / _S73), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_1, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_15)
{

#line 1058
    float4 _S74 = atlas_rect_0(cascade_1, kernelContext_15);

#line 1092
    if(atlas_rect_is_empty_0(_S74))
    {


        return 1.0f;
    }
    float _S75 = 2.0f * kernelContext_15->frame_0->cascade_far_0[cascade_1];

#line 1098
    float _S76 = tile_texels_0(_S74, kernelContext_15);

#line 1098
    float texel_world_0 = _S75 / _S76;

#line 1105
    float4 clip_0 = (((float4(world_position_1 + geometric_normal_1 * float3((texel_world_0 * kernelContext_15->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_15->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_15->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1109
    bool _S77;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1110
        _S77 = true;

#line 1110
    }
    else
    {

#line 1110
        _S77 = (ndc_0.z) <= 0.0f;

#line 1110
    }

#line 1110
    if(_S77)
    {



        return 1.0f;
    }



    float2 tile_uv_5 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 1120
    uint _S78 = shadow_filter_mode_0(pixel_4, kernelContext_15);

#line 1137
    if(_S78 == 2U)
    {

#line 1137
        float _S79 = tile_box_pcf_0(cascade_1, tile_uv_5, ndc_0.z, kernelContext_15);

        return _S79;
    }
    if(_S78 == 1U)
    {

#line 1141
        float _S80 = tile_pcf_0(cascade_1, tile_uv_5, ndc_0.z, pixel_4, 2.0f, kernelContext_15);



        return _S80;
    }

    float _S81 = ndc_0.z;

#line 1148
    float _S82 = sun_penumbra_texels_0(cascade_1, tile_uv_5, _S81, shadow_rotation_0(pixel_4), kernelContext_15);

#line 1148
    float _S83 = tile_pcf_0(cascade_1, tile_uv_5, _S81, pixel_4, _S82, kernelContext_15);
    return _S83;
}

float sun_visibility_0(float3 world_position_2, float3 to_light_3, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_16)
{
    uint cascade_2;

#line 1154
    bool covered_0;

#line 1163
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 1175
    float eye_distance_0 = length(world_position_2 - kernelContext_16->frame_0->camera_position_0.xyz);

#line 1175
    uint index_4 = 0U;

#line 1183
    for(;;)
    {

#line 1183
        if(index_4 < 2U)
        {
        }
        else
        {

#line 1183
            covered_0 = false;

#line 1183
            cascade_2 = 1U;

#line 1183
            break;
        }
        if(eye_distance_0 < kernelContext_16->frame_0->cascade_far_0[index_4])
        {

#line 1185
            covered_0 = true;

#line 1185
            cascade_2 = index_4;



            break;
        }

#line 1183
        index_4 = index_4 + 1U;

#line 1183
    }

#line 1192
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 1192
    }

#line 1192
    float _S84 = cascade_visibility_0(cascade_2, world_position_2, to_light_3, geometric_normal_2, pixel_5, kernelContext_16);

#line 1199
    uint _S85 = cascade_2 + 1U;

#line 1199
    if(_S85 >= 2U)
    {



        return _S84;
    }

#line 1212
    float band_1 = kernelContext_16->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_16->frame_0->cascade_far_0[cascade_2] - band_1)) / band_1);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S84;
    }

#line 1220
    float _S86 = cascade_visibility_0(_S85, world_position_2, to_light_3, geometric_normal_2, pixel_5, kernelContext_16);

#line 1231
    return mix(_S84, _S86, blend_0);
}


#line 1320
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S87 = axis_1.x;

#line 1323
    float _S88 = axis_1.y;

#line 1323
    bool _S89;

#line 1323
    if(_S87 >= _S88)
    {

#line 1323
        _S89 = _S87 >= (axis_1.z);

#line 1323
    }
    else
    {

#line 1323
        _S89 = false;

#line 1323
    }

#line 1323
    uint _S90;

#line 1323
    if(_S89)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1325
            _S90 = 0U;

#line 1325
        }
        else
        {

#line 1325
            _S90 = 1U;

#line 1325
        }

#line 1325
        return _S90;
    }
    if(_S88 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1329
            _S90 = 2U;

#line 1329
        }
        else
        {

#line 1329
            _S90 = 3U;

#line 1329
        }

#line 1329
        return _S90;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1331
        _S90 = 4U;

#line 1331
    }
    else
    {

#line 1331
        _S90 = 5U;

#line 1331
    }

#line 1331
    return _S90;
}


#line 890
uint light_tile_0(uint tile_6)
{
    return 2U + tile_6;
}


#line 1234
float punctual_visibility_0(uint tile_7, float3 world_position_3, float3 to_light_4, float n_dot_l_1, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_17)
{

    uint atlas_0 = light_tile_0(tile_7);

#line 1237
    float4 _S91 = atlas_rect_0(atlas_0, kernelContext_17);

    if(atlas_rect_is_empty_0(_S91))
    {


        return 1.0f;
    }

#line 1243
    float _S92 = tile_texels_0(_S91, kernelContext_17);

    float texel_world_1 = map_world_0 / _S92;

#line 1255
    float4 clip_1 = (((float4(world_position_3 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(0)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(0)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(0)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(0)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(1)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(1)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(1)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(1)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(2)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(2)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(2)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(2)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(3)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(3)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(3)], (&kernelContext_17->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(3)]))));

#line 1262
    float _S93 = clip_1.w;

#line 1262
    if(_S93 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S93) ;

#line 1266
    bool _S94;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1267
        _S94 = true;

#line 1267
    }
    else
    {

#line 1267
        _S94 = (ndc_1.z) <= 0.0f;

#line 1267
    }

#line 1267
    if(_S94)
    {

#line 1267
        _S94 = true;

#line 1267
    }
    else
    {

#line 1267
        _S94 = (ndc_1.z) > 1.0f;

#line 1267
    }

#line 1267
    if(_S94)
    {

#line 1274
        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 1279
    uint _S95 = shadow_filter_mode_0(pixel_6, kernelContext_17);

#line 1288
    if(_S95 == 2U)
    {

#line 1288
        float _S96 = tile_box_pcf_0(atlas_0, tile_uv_6, ndc_1.z, kernelContext_17);

        return _S96;
    }

#line 1290
    float _S97 = tile_pcf_0(atlas_0, tile_uv_6, ndc_1.z, pixel_6, 2.0f, kernelContext_17);

    return _S97;
}


#line 1334
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_4, float3 to_light_5, float n_dot_l_2, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_18)
{

    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_4 - (float4(light_0->position_0) ).xyz;

#line 1342
    float _S98 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_4, to_light_5, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_18);

#line 1348
    return _S98;
}


#line 1295
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_8, float3 world_position_5, float3 to_light_6, float n_dot_l_3, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_19)
{

    if(n_dot_l_3 <= 0.0f)
    {


        return 1.0f;
    }

#line 1302
    float4 _S99 = float4(light_1->direction_0) ;

#line 1309
    float cos_outer_1 = _S99.w;

#line 1309
    float _S100 = punctual_visibility_0(tile_8, world_position_5, to_light_6, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (float4(light_1->position_0) ).xyz, normalize(_S99.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_19);

#line 1316
    return _S100;
}


#line 1536
float3 grass_light_0(float3 world_position_6, float3 normal_3, float2 pixel_9, KernelContext_0 thread* kernelContext_20)
{

#line 1536
    uint _S101 = froxel_of_0(pixel_9, (((float4(world_position_6, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_20->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_20->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_20->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_20->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_20->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_20->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_20->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_20->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_20->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_20->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_20->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_20->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_20->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_20->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_20->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_20->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w, kernelContext_20);

#line 1547
    uint base_2 = _S101 * 17U;
    uint _S102 = min(kernelContext_20->cluster_lights_0[base_2], 16U);

    float3 _S103 = float3(0.0f, 0.0f, 0.0f);

#line 1550
    uint slot_1 = 0U;

#line 1550
    float3 direct_0 = _S103;
    for(;;)
    {

#line 1551
        if(slot_1 < _S102)
        {
        }
        else
        {

#line 1551
            break;
        }

#line 1551
        thread GpuLight_natural_0 _S104 = kernelContext_20->lights_0[kernelContext_20->cluster_lights_0[base_2 + 1U + slot_1]];

#line 1551
        uint _S105 = (&_S104)->kind_0;


        if(((&_S104)->kind_0) == 3U)
        {



            slot_1 = slot_1 + 1U;

#line 1551
            continue;
        }

#line 1564
        bool _S106 = _S105 == 0U;

#line 1564
        float3 to_light_7;

#line 1564
        float reach_1;

#line 1564
        if(_S106)
        {

#line 1564
            to_light_7 = normalize((float4((&_S104)->direction_0) ).xyz);

#line 1564
            reach_1 = 1.0f;

#line 1564
        }
        else
        {

#line 1564
            float4 _S107 = float4((&_S104)->position_0) ;

#line 1571
            float3 offset_0 = _S107.xyz - world_position_6;
            float distance_3 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;
            float reach_2 = punctual_falloff_0(distance_3, _S107.w);
            if(_S105 == 2U)
            {

#line 1575
                float4 _S108 = float4((&_S104)->direction_0) ;

#line 1575
                reach_1 = reach_2 * spot_cone_0(to_light_8, _S108.xyz, _S108.w, (&_S104)->cos_inner_0);

#line 1575
            }
            else
            {

#line 1575
                reach_1 = reach_2;

#line 1575
            }

#line 1575
            to_light_7 = to_light_8;

#line 1564
        }

#line 1582
        float n_dot_l_4 = dot(normal_3, to_light_7);

#line 1582
        float reach_3;
        if(_S106)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 1586
            float _S109 = sun_visibility_0(world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, &sun_cascade_0, &sun_fade_0, kernelContext_20);

#line 1586
            reach_3 = _S109;

#line 1583
        }
        else
        {

#line 1590
            if(_S105 == 1U)
            {

#line 1590
                uint _S110 = (&_S104)->shadow_tile_0;

                if(((&_S104)->shadow_tile_0) <= 8U)
                {

#line 1592
                    float _S111 = point_visibility_0(&_S104, _S110, world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, kernelContext_20);

#line 1592
                    reach_3 = reach_1 * _S111;

#line 1592
                }
                else
                {

#line 1592
                    reach_3 = reach_1;

#line 1592
                }

#line 1590
            }
            else
            {

#line 1590
                uint _S112 = (&_S104)->shadow_tile_0;

#line 1598
                if(((&_S104)->shadow_tile_0) < 14U)
                {

#line 1598
                    float _S113 = spot_visibility_0(&_S104, _S112, world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, kernelContext_20);

#line 1598
                    reach_3 = reach_1 * _S113;

#line 1598
                }
                else
                {

#line 1598
                    reach_3 = reach_1;

#line 1598
                }

#line 1590
            }

#line 1583
        }

#line 1583
        direct_0 = direct_0 + (float4((&_S104)->color_0) ).xyz * float3((max(n_dot_l_4, 0.0f) * reach_3)) ;

#line 1551
        slot_1 = slot_1 + 1U;

#line 1551
    }

#line 1607
    return direct_0 + kernelContext_20->frame_0->ambient_0.xyz;
}


#line 1607
struct pixelOutput_0
{
    float4 output_1 [[color(0)]];
};


#line 1607
struct pixelInput_0
{
    float3 world_position_7 [[user(WORLD_POSITION)]];
    float3 normal_4 [[user(NORMAL)]];
    float3 color_3 [[user(COLOR)]];
    float2 uv_1 [[user(TEXCOORD)]];
    float level_2 [[user(CARD_LEVEL)]];
    [[flat]] uint row_4 [[user(BLADE_ROW)]];
    [[flat]] float patch_3 [[user(PATCH)]];
};


#line 1607
float3 grass_style_1(float3 _S114, uint _S115, float _S116, float _S117, KernelContext_0 thread* kernelContext_21)
{

#line 1607
    GrassBlade_natural_0 device* _S118 = kernelContext_21->blades_1+_S115;

#line 1607
    float4 _S119 = float4(_S118->occlusion_0) ;

#line 1522
    float reach_4 = _S119.w;

#line 1522
    float occluded_1;
    if(reach_4 > 0.0f)
    {

#line 1523
        occluded_1 = saturate(1.0f - _S116 / reach_4);

#line 1523
    }
    else
    {

#line 1523
        occluded_1 = 0.0f;

#line 1523
    }

#line 1523
    float4 _S120 = float4(_S118->patch_0) ;

#line 1523
    float4 _S121 = float4(_S118->glow_0) ;


    float start_1 = _S121.w;

    return mix(mix(_S114, _S119.xyz, float3(occluded_1) ), _S120.xyz, float3((_S120.w * _S117)) ) + _S121.xyz * float3(saturate((_S116 - start_1) / max(1.0f - start_1, 0.00009999999747379f))) ;
}


#line 1883
[[fragment]] pixelOutput_0 bladeFragmentMain(pixelInput_0 _S122 [[stage_in]], float4 position_3 [[position]], GrassTile_0 constant* tile_9 [[buffer(2)]], GrassParams_0 constant* grass_1 [[buffer(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GrassField_0 constant* field_1 [[buffer(7)]], uint device* cluster_lights_1 [[buffer(6)]], GpuLight_natural_0 device* lights_1 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_1 [[texture(0)]], sampler grassCardSampler_1 [[sampler(0)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(3)]], sampler windSampler_1 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(4)]], GrassInstance_natural_0 device* grassCells_1 [[buffer(8)]])
{

#line 1883
    thread KernelContext_0 kernelContext_22;

#line 1883
    (&kernelContext_22)->tile_1 = tile_9;

#line 1883
    (&kernelContext_22)->grass_0 = grass_1;

#line 1883
    (&kernelContext_22)->instances_0 = instances_1;

#line 1883
    (&kernelContext_22)->blades_1 = blades_2;

#line 1883
    (&kernelContext_22)->frame_0 = frame_1;

#line 1883
    (&kernelContext_22)->field_0 = field_1;

#line 1883
    (&kernelContext_22)->cluster_lights_0 = cluster_lights_1;

#line 1883
    (&kernelContext_22)->lights_0 = lights_1;

#line 1883
    (&kernelContext_22)->shadow_atlas_0 = shadow_atlas_1;

#line 1883
    (&kernelContext_22)->shadow_sampler_0 = shadow_sampler_1;

#line 1883
    (&kernelContext_22)->grassCard_0 = grassCard_1;

#line 1883
    (&kernelContext_22)->grassCardSampler_0 = grassCardSampler_1;

#line 1883
    (&kernelContext_22)->grassGround_0 = grassGround_1;

#line 1883
    (&kernelContext_22)->wind_0 = wind_1;

#line 1883
    (&kernelContext_22)->windDirectionLayer_0 = windDirectionLayer_1;

#line 1883
    (&kernelContext_22)->windSampler_0 = windSampler_1;

#line 1883
    (&kernelContext_22)->windIntensityLayer_0 = windIntensityLayer_1;

#line 1883
    (&kernelContext_22)->grassCells_0 = grassCells_1;

#line 1883
    float3 _S123 = grass_style_1(_S122.color_3, _S122.row_4, _S122.uv_1.y, _S122.patch_3, &kernelContext_22);

#line 1883
    float3 _S124 = grass_light_0(_S122.world_position_7, normalize(_S122.normal_4), position_3.xy, &kernelContext_22);

#line 1883
    pixelOutput_0 _S125 = { float4(_S123 * _S124, 1.0f) };



    return _S125;
}


#line 1887
struct pixelOutput_1
{
    float4 output_2 [[color(0)]];
};


#line 1887
struct pixelInput_1
{
    float3 world_position_8 [[user(WORLD_POSITION)]];
    float3 normal_5 [[user(NORMAL)]];
    float3 color_4 [[user(COLOR)]];
    float2 uv_2 [[user(TEXCOORD)]];
    float level_3 [[user(CARD_LEVEL)]];
    [[flat]] uint row_5 [[user(BLADE_ROW)]];
    [[flat]] float patch_4 [[user(PATCH)]];
};


#line 1892
[[fragment]] pixelOutput_1 fragmentMain(pixelInput_1 _S126 [[stage_in]], float4 position_4 [[position]], GrassTile_0 constant* tile_10 [[buffer(2)]], GrassParams_0 constant* grass_2 [[buffer(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(3)]], GrassBlade_natural_0 device* blades_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GrassField_0 constant* field_2 [[buffer(7)]], uint device* cluster_lights_2 [[buffer(6)]], GpuLight_natural_0 device* lights_2 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_2 [[texture(0)]], sampler grassCardSampler_2 [[sampler(0)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(3)]], sampler windSampler_2 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(4)]], GrassInstance_natural_0 device* grassCells_2 [[buffer(8)]])
{

#line 1892
    thread KernelContext_0 kernelContext_23;

#line 1892
    (&kernelContext_23)->tile_1 = tile_10;

#line 1892
    (&kernelContext_23)->grass_0 = grass_2;

#line 1892
    (&kernelContext_23)->instances_0 = instances_2;

#line 1892
    (&kernelContext_23)->blades_1 = blades_3;

#line 1892
    (&kernelContext_23)->frame_0 = frame_2;

#line 1892
    (&kernelContext_23)->field_0 = field_2;

#line 1892
    (&kernelContext_23)->cluster_lights_0 = cluster_lights_2;

#line 1892
    (&kernelContext_23)->lights_0 = lights_2;

#line 1892
    (&kernelContext_23)->shadow_atlas_0 = shadow_atlas_2;

#line 1892
    (&kernelContext_23)->shadow_sampler_0 = shadow_sampler_2;

#line 1892
    (&kernelContext_23)->grassCard_0 = grassCard_2;

#line 1892
    (&kernelContext_23)->grassCardSampler_0 = grassCardSampler_2;

#line 1892
    (&kernelContext_23)->grassGround_0 = grassGround_2;

#line 1892
    (&kernelContext_23)->wind_0 = wind_2;

#line 1892
    (&kernelContext_23)->windDirectionLayer_0 = windDirectionLayer_2;

#line 1892
    (&kernelContext_23)->windSampler_0 = windSampler_2;

#line 1892
    (&kernelContext_23)->windIntensityLayer_0 = windIntensityLayer_2;

#line 1892
    (&kernelContext_23)->grassCells_0 = grassCells_2;

#line 1898
    float _S127 = _S126.uv_2.y;

#line 1898
    float3 _S128 = float3(_S126.uv_2.x, 1.0f - _S127, 0.0f);

    if((((grassCard_2).sample((grassCardSampler_2), ((_S128)).xy, uint(((_S128)).z), level((_S126.level_3)))).x) < 0.5f)
    {
        discard_fragment();

#line 1900
    }

#line 1900
    float3 _S129 = grass_style_1(_S126.color_4, _S126.row_5, _S127, _S126.patch_4, &kernelContext_23);

#line 1900
    float3 _S130 = grass_light_0(_S126.world_position_8, normalize(_S126.normal_5), position_4.xy, &kernelContext_23);

#line 1900
    pixelOutput_1 _S131 = { float4(_S129 * _S130, 1.0f) };

#line 1908
    return _S131;
}


#line 1963
uint grass_shell_count_0(KernelContext_0 thread* kernelContext_24)
{
    return clamp(uint(kernelContext_24->field_0->stack_0.y + 0.5f), 1U, 64U);
}


#line 1470
float grass_ground_texel_0(int2 texel_1, KernelContext_0 thread* kernelContext_25)
{

    int3 _S132 = int3(clamp(texel_1, int2(int(0), int(0)), int2(int(kernelContext_25->field_0->maps_0.x) - int(1), int(kernelContext_25->field_0->maps_0.y) - int(1))), int(0));

#line 1473
    return ((kernelContext_25->grassGround_0).read(vec<uint,2>(((_S132)).xy), uint(((_S132)).z)).x);
}


#line 1461
struct GrassGround_0
{
    float height_1;
    float3 normal_6;
};


#line 1478
GrassGround_0 grass_ground_under_0(float2 world_0, KernelContext_0 thread* kernelContext_26)
{
    float2 at_0 = (world_0 - kernelContext_26->field_0->ground_1.xy) * float2(kernelContext_26->field_0->ground_1.w) ;
    float2 base_3 = floor(at_0);
    float2 blend_1 = at_0 - base_3;
    int2 _S133 = int2(base_3);

#line 1483
    float _S134 = grass_ground_texel_0(_S133, kernelContext_26);

#line 1483
    float _S135 = grass_ground_texel_0(_S133 + int2(int(1), int(0)), kernelContext_26);

#line 1483
    float _S136 = grass_ground_texel_0(_S133 + int2(int(0), int(1)), kernelContext_26);

#line 1483
    float _S137 = grass_ground_texel_0(_S133 + int2(int(1), int(1)), kernelContext_26);

#line 1490
    float _S138 = blend_1.x;

#line 1490
    float _S139 = _S135 - _S134;

#line 1490
    float lower_2 = _S134 + _S138 * _S139;
    float _S140 = _S137 - _S136;

#line 1489
    thread GrassGround_0 under_0;


    (&under_0)->height_1 = lower_2 + blend_1.y * (_S136 + _S138 * _S140 - lower_2);


    (&under_0)->normal_6 = normalize(float3(- (0.5f * (_S139 + _S140)), kernelContext_26->field_0->ground_1.z, - (0.5f * (_S136 - _S134 + (_S137 - _S135)))));
    return under_0;
}


#line 1400
float windSmoothTriangle_0(float u_1)
{
    float s_0 = abs(fract(u_1 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}


float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_27)
{


    float2 _S141 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_27->windDirectionLayer_0).sample((kernelContext_27->windSampler_0), (kernelContext_27->wind_0->directionUv_0 + _S141 * kernelContext_27->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_27->windIntensityLayer_0).sample((kernelContext_27->windSampler_0), (kernelContext_27->wind_0->intensityUv_0 + _S141 * kernelContext_27->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;

    float2 base_4 = kernelContext_27->wind_0->baseDirection_0;
    float _S142 = kernelContext_27->wind_0->baseDirection_0.x;

#line 1421
    float _S143 = deflection_0.x;

#line 1421
    float _S144 = kernelContext_27->wind_0->baseDirection_0.y;

#line 1421
    float _S145 = deflection_0.y;

#line 1421
    float2 turned_0 = float2(_S142 * _S143 - _S144 * _S145, _S142 * _S145 + _S144 * _S143);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 1423
    float2 direction_2;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 1425
        direction_2 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 1425
    }
    else
    {

#line 1425
        direction_2 = base_4;

#line 1425
    }

#line 1430
    float speed_0 = intensity_0 * kernelContext_27->wind_0->baseSpeed_0 * (1.0f + kernelContext_27->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S141, base_4) * kernelContext_27->wind_0->invGustWavelength_0 + kernelContext_27->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_2.x * speed_0, 0.0f, direction_2.y * speed_0);
}



float3 grass_lean_0(float3 velocity_0, float height_2)
{
    float speed_1 = length(velocity_0);
    float bend_0 = height_2 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_2 = velocity_0.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;
    float _S146 = bend_0 * bend_0;
    return float3(along_2.x, - (_S146 / (height_2 + sqrt(max(height_2 * height_2 - _S146, 0.0f)))), along_2.y);
}


#line 1912
struct GrassShellVertex_0
{
    float4 position_5;
    float3 world_position_9;
    float3 rest_0;
    float3 normal_7;
    float footprint_0;
    float occlusion_1;
    [[flat]] float4 fin_0;
};


#line 1936
GrassShellVertex_0 grass_sheet_vertex_0(float2 rest_1, float share_0, float4 fin_1, float occlusion_2, KernelContext_0 thread* kernelContext_28)
{

#line 1936
    GrassGround_0 _S147 = grass_ground_under_0(rest_1, kernelContext_28);


    float lift_0 = share_0 * kernelContext_28->field_0->stack_0.x;
    float _S148 = rest_1.x;

#line 1940
    float _S149 = rest_1.y;

#line 1940
    float3 ground_2 = float3(_S148, _S147.height_1, _S149);

#line 1940
    float3 _S150 = windSample_0(ground_2 - kernelContext_28->frame_0->camera_position_0.xyz, kernelContext_28);

#line 1947
    float3 world_1 = ground_2 + float3(0.0f, lift_0, 0.0f) + grass_lean_0(_S150, kernelContext_28->field_0->stack_0.x) * float3((share_0 * share_0)) ;

    thread GrassShellVertex_0 output_3;
    float4 _S151 = (((float4(world_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_28->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_28->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_28->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_28->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_28->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_28->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_28->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_28->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_28->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_28->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_28->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_28->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_28->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_28->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_28->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_28->frame_0->view_proj_0.data_0[int(3)][int(3)]))));

#line 1950
    (&output_3)->position_5 = _S151;
    (&output_3)->world_position_9 = world_1;
    (&output_3)->rest_0 = float3(_S148, lift_0, _S149);
    (&output_3)->normal_7 = _S147.normal_6;


    (&output_3)->footprint_0 = max(abs(_S151.w), 0.00009999999747379f) / max(kernelContext_28->grass_0->screen_0.x, 0.00009999999747379f);
    (&output_3)->occlusion_1 = occlusion_2;
    (&output_3)->fin_0 = fin_1;
    return output_3;
}


#line 1985
float grass_occlusion_at_0(float share_1, KernelContext_0 thread* kernelContext_29)
{

#line 1985
    uint _S152 = grass_shell_count_0(kernelContext_29);

#line 1985
    float occlusion_3 = kernelContext_29->field_0->layers_0[int(0)].y;

#line 1985
    uint shell_0 = 0U;



    for(;;)
    {

#line 1989
        if(shell_0 < _S152)
        {
        }
        else
        {

#line 1989
            break;
        }
        if((kernelContext_29->field_0->layers_0[shell_0].x) <= share_1)
        {

#line 1991
            occlusion_3 = kernelContext_29->field_0->layers_0[shell_0].y;

#line 1991
        }

#line 1989
        shell_0 = shell_0 + 1U;

#line 1989
    }

#line 1996
    return occlusion_3;
}


#line 529
struct GrassInstance_0
{
    float4 root_0;
    float4 facing_0;
    float4 lean_0;
    float4 ground_0;
    float4 clump_1;
    uint4 lanes_0;
};


#line 2063
GrassInstance_0 grass_no_blade_0()
{
    thread GrassInstance_0 none_0;
    float4 _S153 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 2066
    (&none_0)->root_0 = _S153;
    (&none_0)->facing_0 = _S153;
    (&none_0)->lean_0 = _S153;
    (&none_0)->ground_0 = _S153;
    (&none_0)->lanes_0 = uint4(0U, 0U, 0U, 0U);
    return none_0;
}



GrassInstance_0 grass_cell_0(int2 global_0, KernelContext_0 thread* kernelContext_30)
{
    int side_1 = int(max(kernelContext_30->field_0->tiles_0.z, 1U));
    int2 extent_0 = int2(int(kernelContext_30->field_0->tiles_0.x), int(kernelContext_30->field_0->tiles_0.y)) * int2(side_1) ;
    int _S154 = global_0.x;

#line 2080
    bool _S155;

#line 2080
    if(_S154 < int(0))
    {

#line 2080
        _S155 = true;

#line 2080
    }
    else
    {

#line 2080
        _S155 = (global_0.y) < int(0);

#line 2080
    }

#line 2080
    if(_S155)
    {

#line 2080
        _S155 = true;

#line 2080
    }
    else
    {

#line 2080
        _S155 = _S154 >= (extent_0.x);

#line 2080
    }

#line 2080
    if(_S155)
    {

#line 2080
        _S155 = true;

#line 2080
    }
    else
    {

#line 2080
        _S155 = (global_0.y) >= (extent_0.y);

#line 2080
    }

#line 2080
    if(_S155)
    {
        return grass_no_blade_0();
    }
    uint2 _S156 = uint2(global_0);
    uint cells_0 = uint(side_1);
    uint _S157 = _S156.y;

#line 2086
    uint _S158 = _S157 / cells_0;

#line 2086
    uint _S159 = _S158 * kernelContext_30->field_0->tiles_0.x;

#line 2086
    uint _S160 = _S156.x;

#line 2086
    uint _S161 = _S160 / cells_0;

#line 2086
    uint slot_2 = _S159 + _S161;
    uint _S162 = _S157 % cells_0;

#line 2087
    uint _S163 = _S162 * cells_0;

#line 2087
    uint _S164 = _S160 % cells_0;
    GrassInstance_natural_0 _S165 = kernelContext_30->grassCells_0[slot_2 * kernelContext_30->field_0->tiles_0.w + (_S163 + _S164)];

#line 2088
    GrassInstance_0 _S166 = { float4(_S165.root_0) , float4(_S165.facing_0) , float4(_S165.lean_0) , float4(_S165.ground_0) , float4(_S165.clump_1) , uint4(_S165.lanes_0)  };

#line 2088
    return _S166;
}




float grass_strand_reach_0(const GrassInstance_0 thread* blade_3, float lift_1, float floor_reach_0, float fade_1, KernelContext_0 thread* kernelContext_31)
{
    float _S167 = blade_3->root_0.w;

#line 2096
    bool _S168;

#line 2096
    if(!(lift_1 < _S167))
    {

#line 2096
        _S168 = true;

#line 2096
    }
    else
    {

#line 2096
        GrassBlade_0 _S169 = grass_row_0(blade_3->lanes_0.y, kernelContext_31);

#line 2096
        _S168 = (_S169.flags_0.x) != 1U;

#line 2096
    }

#line 2096
    if(_S168)
    {
        return -1.0f;
    }
    float bound_0 = 0.5f * kernelContext_31->field_0->origin_0.w;

    return min(max(min(blade_3->facing_0.z, bound_0) * (1.0f - lift_1 / _S167), floor_reach_0), bound_0) * fade_1;
}


#line 2102
struct pixelOutput_2
{
    float4 output_4 [[color(0)]];
};


#line 2102
struct pixelInput_2
{
    float3 world_position_10 [[user(WORLD_POSITION)]];
    float3 rest_2 [[user(REST_POSITION)]];
    float3 normal_8 [[user(NORMAL)]];
    float footprint_1 [[user(FOOTPRINT)]];
    float occlusion_4 [[user(OCCLUSION)]];
    [[flat]] float4 fin_2 [[user(FIN)]];
};


#line 2107
[[fragment]] pixelOutput_2 shellFragmentMain(pixelInput_2 _S170 [[stage_in]], float4 position_6 [[position]], GrassTile_0 constant* tile_11 [[buffer(2)]], GrassParams_0 constant* grass_3 [[buffer(1)]], GrassInstance_natural_0 device* instances_3 [[buffer(3)]], GrassBlade_natural_0 device* blades_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], GrassField_0 constant* field_3 [[buffer(7)]], uint device* cluster_lights_3 [[buffer(6)]], GpuLight_natural_0 device* lights_3 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_3 [[texture(0)]], sampler grassCardSampler_3 [[sampler(0)]], texture2d<float, access::sample> grassGround_3 [[texture(2)]], WindParams_0 constant* wind_3 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_3 [[texture(3)]], sampler windSampler_3 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_3 [[texture(4)]], GrassInstance_natural_0 device* grassCells_3 [[buffer(8)]])
{

#line 2107
    thread KernelContext_0 kernelContext_32;

#line 2107
    (&kernelContext_32)->tile_1 = tile_11;

#line 2107
    (&kernelContext_32)->grass_0 = grass_3;

#line 2107
    (&kernelContext_32)->instances_0 = instances_3;

#line 2107
    (&kernelContext_32)->blades_1 = blades_4;

#line 2107
    (&kernelContext_32)->frame_0 = frame_3;

#line 2107
    (&kernelContext_32)->field_0 = field_3;

#line 2107
    (&kernelContext_32)->cluster_lights_0 = cluster_lights_3;

#line 2107
    (&kernelContext_32)->lights_0 = lights_3;

#line 2107
    (&kernelContext_32)->shadow_atlas_0 = shadow_atlas_3;

#line 2107
    (&kernelContext_32)->shadow_sampler_0 = shadow_sampler_3;

#line 2107
    (&kernelContext_32)->grassCard_0 = grassCard_3;

#line 2107
    (&kernelContext_32)->grassCardSampler_0 = grassCardSampler_3;

#line 2107
    (&kernelContext_32)->grassGround_0 = grassGround_3;

#line 2107
    (&kernelContext_32)->wind_0 = wind_3;

#line 2107
    (&kernelContext_32)->windDirectionLayer_0 = windDirectionLayer_3;

#line 2107
    (&kernelContext_32)->windSampler_0 = windSampler_3;

#line 2107
    (&kernelContext_32)->windIntensityLayer_0 = windIntensityLayer_3;

#line 2107
    (&kernelContext_32)->grassCells_0 = grassCells_3;

    float cell_1 = field_3->origin_0.w;
    float2 _S171 = _S170.rest_2.xz;

#line 2110
    float2 from_corner_0 = (_S171 - field_3->origin_0.xy) / float2(cell_1) ;
    float2 _S172 = floor(from_corner_0);

#line 2111
    int2 _S173 = int2(_S172);
    float2 inside_0 = from_corner_0 - _S172;

#line 2112
    int _S174;
    if((inside_0.x) < 0.5f)
    {

#line 2113
        _S174 = int(-1);

#line 2113
    }
    else
    {

#line 2113
        _S174 = int(1);

#line 2113
    }

#line 2113
    int band_2;

#line 2113
    if((inside_0.y) < 0.5f)
    {

#line 2113
        band_2 = int(-1);

#line 2113
    }
    else
    {

#line 2113
        band_2 = int(1);

#line 2113
    }
    float lift_2 = _S170.rest_2.y;


    float _S175 = 0.5f * _S170.footprint_1;
    float _S176 = _S170.fin_2.y;

    GrassInstance_0 _S177 = grass_no_blade_0();

    float _S178 = _S170.fin_2.x;

#line 2122
    int dz_0;

#line 2122
    int dx_0;

#line 2122
    float nearest_0;

#line 2122
    float nearest_1;

#line 2122
    GrassInstance_0 found_2;

#line 2122
    GrassInstance_0 found_3;

#line 2122
    bool _S179;

#line 2122
    if(_S178 < 0.5f)
    {

#line 2122
        nearest_1 = 2.0f;

#line 2122
        found_3 = _S177;

#line 2122
        dz_0 = int(0);


        for(;;)
        {

#line 2125
            if(dz_0 < int(2))
            {
            }
            else
            {

#line 2125
                break;
            }

#line 2125
            nearest_0 = nearest_1;

#line 2125
            found_2 = found_3;

#line 2125
            dx_0 = int(0);

            for(;;)
            {

#line 2127
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2127
                    break;
                }

#line 2127
                GrassInstance_0 _S180 = grass_cell_0(_S173 + int2(dx_0 * _S174, dz_0 * band_2), &kernelContext_32);

#line 2127
                thread GrassInstance_0 _S181 = _S180;

#line 2127
                float _S182 = grass_strand_reach_0(&_S181, lift_2, _S175, _S176, &kernelContext_32);



                float2 offset_1 = _S171 - _S180.root_0.xz;
                float apart_0 = sqrt(dot(offset_1, offset_1)) / max(_S182, 9.99999997475242708e-07f);
                if(_S182 > 0.0f)
                {

#line 2133
                    _S179 = apart_0 < nearest_0;

#line 2133
                }
                else
                {

#line 2133
                    _S179 = false;

#line 2133
                }

#line 2133
                if(_S179)
                {

#line 2133
                    nearest_0 = apart_0;

#line 2133
                    found_2 = _S180;

#line 2133
                }

#line 2127
                dx_0 = dx_0 + int(1);

#line 2127
            }

#line 2125
            int dz_1 = dz_0 + int(1);

#line 2125
            nearest_1 = nearest_0;

#line 2125
            found_3 = found_2;

#line 2125
            dz_0 = dz_1;

#line 2125
        }

#line 2125
        nearest_0 = nearest_1;

#line 2125
        found_2 = found_3;

#line 2122
    }
    else
    {

#line 2145
        bool along_z_0 = _S178 < 1.5f;
        float _S183 = _S170.fin_2.z;

#line 2146
        if(along_z_0)
        {

#line 2146
            nearest_0 = field_3->origin_0.x;

#line 2146
        }
        else
        {

#line 2146
            nearest_0 = field_3->origin_0.y;

#line 2146
        }
        int _S184 = int(floor((_S183 - nearest_0) / cell_1 + 0.5f)) - int(2);
        if(along_z_0)
        {

#line 2148
            dz_0 = _S173.y;

#line 2148
        }
        else
        {

#line 2148
            dz_0 = _S173.x;

#line 2148
        }
        if(along_z_0)
        {

#line 2149
            _S174 = band_2;

#line 2149
        }

#line 2149
        nearest_0 = 2.0f;

#line 2149
        found_2 = _S177;

#line 2149
        band_2 = int(0);
        for(;;)
        {

#line 2150
            if(band_2 < int(4))
            {
            }
            else
            {

#line 2150
                break;
            }

#line 2150
            nearest_1 = nearest_0;

#line 2150
            found_3 = found_2;

#line 2150
            dx_0 = int(0);

            for(;;)
            {

#line 2152
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2152
                    break;
                }
                int along_3 = dz_0 + dx_0 * _S174;

#line 2154
                int2 _S185;
                if(along_z_0)
                {

#line 2155
                    _S185 = int2(_S184 + band_2, along_3);

#line 2155
                }
                else
                {

#line 2155
                    _S185 = int2(along_3, _S184 + band_2);

#line 2155
                }

#line 2155
                GrassInstance_0 _S186 = grass_cell_0(_S185, &kernelContext_32);

#line 2155
                thread GrassInstance_0 _S187 = _S186;

#line 2155
                float _S188 = grass_strand_reach_0(&_S187, lift_2, _S175, _S176, &kernelContext_32);

#line 2155
                float offset_2;



                if(along_z_0)
                {

#line 2159
                    offset_2 = _S170.rest_2.z - _S186.root_0.z;

#line 2159
                }
                else
                {

#line 2159
                    offset_2 = _S170.rest_2.x - _S186.root_0.x;

#line 2159
                }
                float apart_1 = abs(offset_2) / max(_S188, 9.99999997475242708e-07f);
                if(_S188 > 0.0f)
                {

#line 2161
                    _S179 = apart_1 < nearest_1;

#line 2161
                }
                else
                {

#line 2161
                    _S179 = false;

#line 2161
                }

#line 2161
                if(_S179)
                {

#line 2161
                    nearest_1 = apart_1;

#line 2161
                    found_3 = _S186;

#line 2161
                }

#line 2152
                dx_0 = dx_0 + int(1);

#line 2152
            }

#line 2150
            int band_3 = band_2 + int(1);

#line 2150
            nearest_0 = nearest_1;

#line 2150
            found_2 = found_3;

#line 2150
            band_2 = band_3;

#line 2150
        }

#line 2122
    }

#line 2169
    if(!(nearest_0 < 1.0f))
    {
        discard_fragment();

#line 2169
    }

#line 2169
    GrassBlade_0 _S189 = grass_row_0(found_2.lanes_0.y, &kernelContext_32);

#line 2175
    float along_blade_0 = saturate(lift_2 / found_2.root_0.w);

    float3 color_5 = mix(_S189.root_color_0.xyz, _S189.tip_color_0.xyz, float3(along_blade_0) ) * float3((1.0f - 0.18000000715255737f * found_2.facing_0.w)) ;
    float _S190 = grass_patch_0(found_2.lanes_0.z);

#line 2178
    thread GrassBlade_0 _S191 = _S189;

#line 2178
    float3 _S192 = grass_style_0(color_5, &_S191, along_blade_0, _S190);

#line 2178
    float3 color_6 = _S192 * float3(_S170.occlusion_4) ;

#line 2178
    float3 normal_9;

    if((_S189.flags_0.y) == 1U)
    {

#line 2180
        normal_9 = float3(0.0f, 1.0f, 0.0f);

#line 2180
    }
    else
    {

#line 2180
        normal_9 = normalize(_S170.normal_8);

#line 2180
    }

#line 2180
    float3 _S193 = grass_light_0(_S170.world_position_10, normal_9, position_6.xy, &kernelContext_32);

#line 2180
    pixelOutput_2 _S194 = { float4(color_6 * _S193, 1.0f) };
    return _S194;
}


#line 2181
struct vertexMain_Result_0
{
    float4 position_7 [[position]];
    float3 world_position_11 [[user(WORLD_POSITION)]];
    float3 normal_10 [[user(NORMAL)]];
    float3 color_7 [[user(COLOR)]];
    float2 uv_3 [[user(TEXCOORD)]];
    float level_4 [[user(CARD_LEVEL)]];
    uint row_6 [[user(BLADE_ROW)]];
    float patch_5 [[user(PATCH)]];
};


#line 2181
[[vertex]] vertexMain_Result_0 vertexMain(uint index_5 [[vertex_id]], uint instance_id_0 [[instance_id]], GrassTile_0 constant* tile_12 [[buffer(2)]], GrassParams_0 constant* grass_4 [[buffer(1)]], GrassInstance_natural_0 device* instances_4 [[buffer(3)]], GrassBlade_natural_0 device* blades_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], GrassField_0 constant* field_4 [[buffer(7)]], uint device* cluster_lights_4 [[buffer(6)]], GpuLight_natural_0 device* lights_4 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_4 [[texture(0)]], sampler grassCardSampler_4 [[sampler(0)]], texture2d<float, access::sample> grassGround_4 [[texture(2)]], WindParams_0 constant* wind_4 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_4 [[texture(3)]], sampler windSampler_4 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_4 [[texture(4)]], GrassInstance_natural_0 device* grassCells_4 [[buffer(8)]])
{

#line 2181
    thread KernelContext_0 kernelContext_33;

#line 2181
    (&kernelContext_33)->tile_1 = tile_12;

#line 2181
    (&kernelContext_33)->grass_0 = grass_4;

#line 2181
    (&kernelContext_33)->instances_0 = instances_4;

#line 2181
    (&kernelContext_33)->blades_1 = blades_5;

#line 2181
    (&kernelContext_33)->frame_0 = frame_4;

#line 2181
    (&kernelContext_33)->field_0 = field_4;

#line 2181
    (&kernelContext_33)->cluster_lights_0 = cluster_lights_4;

#line 2181
    (&kernelContext_33)->lights_0 = lights_4;

#line 2181
    (&kernelContext_33)->shadow_atlas_0 = shadow_atlas_4;

#line 2181
    (&kernelContext_33)->shadow_sampler_0 = shadow_sampler_4;

#line 2181
    (&kernelContext_33)->grassCard_0 = grassCard_4;

#line 2181
    (&kernelContext_33)->grassCardSampler_0 = grassCardSampler_4;

#line 2181
    (&kernelContext_33)->grassGround_0 = grassGround_4;

#line 2181
    (&kernelContext_33)->wind_0 = wind_4;

#line 2181
    (&kernelContext_33)->windDirectionLayer_0 = windDirectionLayer_4;

#line 2181
    (&kernelContext_33)->windSampler_0 = windSampler_4;

#line 2181
    (&kernelContext_33)->windIntensityLayer_0 = windIntensityLayer_4;

#line 2181
    (&kernelContext_33)->grassCells_0 = grassCells_4;

#line 1817
    GrassInstance_natural_0 blade_4 = instances_4[tile_12->slot_0.x * grass_4->limits_0.x + instance_id_0];

#line 1817
    uint4 _S195 = uint4(blade_4.lanes_0) ;
    uint _S196 = _S195.y;

#line 1818
    GrassBlade_0 _S197 = grass_row_0(_S196, &kernelContext_33);


    uint _S198 = index_5 % 6U;

#line 1821
    float4 _S199 = float4(blade_4.facing_0) ;

#line 1826
    float2 facing_1 = _S199.xy;

#line 1826
    float2 across_0;
    if((index_5 / 6U) == 0U)
    {

#line 1827
        across_0 = facing_1;

#line 1827
    }
    else
    {

#line 1827
        across_0 = float2(- facing_1.y, facing_1.x);

#line 1827
    }

    float _S200 = _S199.z;

#line 1834
    float _S201 = GRASS_CARD_CORNERS_0[_S198].y;

#line 1834
    float4 _S202 = float4(blade_4.root_0) ;
    float3 world_2 = _S202.xyz + float3(across_0.x, 0.0f, across_0.y) * float3((_S200 * (GRASS_CARD_CORNERS_0[_S198].x * 2.0f - 1.0f)))  + float3(0.0f, _S202.w * _S201, 0.0f) + (float4(blade_4.lean_0) ).xyz * float3((_S201 * _S201)) ;

    thread GrassVertex_0 output_5;
    (&output_5)->position_2 = (((float4(world_2, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_33)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_33)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_5)->world_position_0 = world_2;

#line 1839
    float3 _S203;
    if((_S197.flags_0.y) == 1U)
    {

#line 1840
        _S203 = float3(0.0f, 1.0f, 0.0f);

#line 1840
    }
    else
    {

#line 1840
        _S203 = (float4(blade_4.ground_0) ).xyz;

#line 1840
    }

#line 1840
    (&output_5)->normal_2 = _S203;

#line 1845
    (&output_5)->color_1 = mix(_S197.root_color_0.xyz, _S197.tip_color_0.xyz, float3(_S201) ) * float3((1.0f - 0.18000000715255737f * _S199.w)) ;
    (&output_5)->uv_0 = GRASS_CARD_CORNERS_0[_S198];

#line 1851
    (&output_5)->level_1 = grassCardLevel_0(2.0f * _S200 * (&kernelContext_33)->grass_0->screen_0.x / max(abs((&output_5)->position_2.w), 0.00009999999747379f));

    (&output_5)->row_2 = min(_S196, max(grass_4->limits_0.y, 1U) - 1U);
    (&output_5)->patch_1 = grass_patch_0(_S195.z);
    GrassVertex_0 _S204 = output_5;

#line 1855
    thread vertexMain_Result_0 _S205;

#line 1855
    (&_S205)->position_7 = _S204.position_2;

#line 1855
    (&_S205)->world_position_11 = _S204.world_position_0;

#line 1855
    (&_S205)->normal_10 = _S204.normal_2;

#line 1855
    (&_S205)->color_7 = _S204.color_1;

#line 1855
    (&_S205)->uv_3 = _S204.uv_0;

#line 1855
    (&_S205)->level_4 = _S204.level_1;

#line 1855
    (&_S205)->row_6 = _S204.row_2;

#line 1855
    (&_S205)->patch_5 = _S204.patch_1;

#line 1855
    return _S205;
}


#line 1855
struct bladeNearVertexMain_Result_0
{
    float4 position_8 [[position]];
    float3 world_position_12 [[user(WORLD_POSITION)]];
    float3 normal_11 [[user(NORMAL)]];
    float3 color_8 [[user(COLOR)]];
    float2 uv_4 [[user(TEXCOORD)]];
    float level_5 [[user(CARD_LEVEL)]];
    uint row_7 [[user(BLADE_ROW)]];
    float patch_6 [[user(PATCH)]];
};


#line 1855
[[vertex]] bladeNearVertexMain_Result_0 bladeNearVertexMain(uint index_6 [[vertex_id]], uint instance_id_1 [[instance_id]], GrassTile_0 constant* tile_13 [[buffer(2)]], GrassParams_0 constant* grass_5 [[buffer(1)]], GrassInstance_natural_0 device* instances_5 [[buffer(3)]], GrassBlade_natural_0 device* blades_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], GrassField_0 constant* field_5 [[buffer(7)]], uint device* cluster_lights_5 [[buffer(6)]], GpuLight_natural_0 device* lights_5 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_5 [[texture(0)]], sampler grassCardSampler_5 [[sampler(0)]], texture2d<float, access::sample> grassGround_5 [[texture(2)]], WindParams_0 constant* wind_5 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_5 [[texture(3)]], sampler windSampler_5 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_5 [[texture(4)]], GrassInstance_natural_0 device* grassCells_5 [[buffer(8)]])
{

#line 1855
    thread KernelContext_0 kernelContext_34;

#line 1855
    (&kernelContext_34)->tile_1 = tile_13;

#line 1855
    (&kernelContext_34)->grass_0 = grass_5;

#line 1855
    (&kernelContext_34)->instances_0 = instances_5;

#line 1855
    (&kernelContext_34)->blades_1 = blades_6;

#line 1855
    (&kernelContext_34)->frame_0 = frame_5;

#line 1855
    (&kernelContext_34)->field_0 = field_5;

#line 1855
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_5;

#line 1855
    (&kernelContext_34)->lights_0 = lights_5;

#line 1855
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_5;

#line 1855
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_5;

#line 1855
    (&kernelContext_34)->grassCard_0 = grassCard_5;

#line 1855
    (&kernelContext_34)->grassCardSampler_0 = grassCardSampler_5;

#line 1855
    (&kernelContext_34)->grassGround_0 = grassGround_5;

#line 1855
    (&kernelContext_34)->wind_0 = wind_5;

#line 1855
    (&kernelContext_34)->windDirectionLayer_0 = windDirectionLayer_5;

#line 1855
    (&kernelContext_34)->windSampler_0 = windSampler_5;

#line 1855
    (&kernelContext_34)->windIntensityLayer_0 = windIntensityLayer_5;

#line 1855
    (&kernelContext_34)->grassCells_0 = grassCells_5;

#line 1865
    GrassInstance_natural_0 blade_5 = instances_5[(field_5->tiles_0.x * field_5->tiles_0.y + tile_13->slot_0.x) * grass_5->limits_0.x + instance_id_1];

#line 1865
    thread GrassInstance_natural_0 _S206 = blade_5;

#line 1865
    float2 _S207 = grass_blade_lod_0(length((float4((&_S206)->root_0) ).xyz - frame_5->camera_position_0.xyz), &kernelContext_34);

#line 1865
    _S206 = blade_5;

#line 1865
    GrassVertex_0 _S208 = grass_blade_vertex_0(&_S206, index_6, 7U, _S207, &kernelContext_34);

#line 1865
    thread bladeNearVertexMain_Result_0 _S209;

#line 1865
    (&_S209)->position_8 = _S208.position_2;

#line 1865
    (&_S209)->world_position_12 = _S208.world_position_0;

#line 1865
    (&_S209)->normal_11 = _S208.normal_2;

#line 1865
    (&_S209)->color_8 = _S208.color_1;

#line 1865
    (&_S209)->uv_4 = _S208.uv_0;

#line 1865
    (&_S209)->level_5 = _S208.level_1;

#line 1865
    (&_S209)->row_7 = _S208.row_2;

#line 1865
    (&_S209)->patch_6 = _S208.patch_1;

#line 1865
    return _S209;
}


#line 1865
struct bladeFarVertexMain_Result_0
{
    float4 position_9 [[position]];
    float3 world_position_13 [[user(WORLD_POSITION)]];
    float3 normal_12 [[user(NORMAL)]];
    float3 color_9 [[user(COLOR)]];
    float2 uv_5 [[user(TEXCOORD)]];
    float level_6 [[user(CARD_LEVEL)]];
    uint row_8 [[user(BLADE_ROW)]];
    float patch_7 [[user(PATCH)]];
};


#line 1865
[[vertex]] bladeFarVertexMain_Result_0 bladeFarVertexMain(uint index_7 [[vertex_id]], uint instance_id_2 [[instance_id]], GrassTile_0 constant* tile_14 [[buffer(2)]], GrassParams_0 constant* grass_6 [[buffer(1)]], GrassInstance_natural_0 device* instances_6 [[buffer(3)]], GrassBlade_natural_0 device* blades_7 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_6 [[buffer(0)]], GrassField_0 constant* field_6 [[buffer(7)]], uint device* cluster_lights_6 [[buffer(6)]], GpuLight_natural_0 device* lights_6 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_6 [[texture(0)]], sampler grassCardSampler_6 [[sampler(0)]], texture2d<float, access::sample> grassGround_6 [[texture(2)]], WindParams_0 constant* wind_6 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_6 [[texture(3)]], sampler windSampler_6 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_6 [[texture(4)]], GrassInstance_natural_0 device* grassCells_6 [[buffer(8)]])
{

#line 1865
    thread KernelContext_0 kernelContext_35;

#line 1865
    (&kernelContext_35)->tile_1 = tile_14;

#line 1865
    (&kernelContext_35)->grass_0 = grass_6;

#line 1865
    (&kernelContext_35)->instances_0 = instances_6;

#line 1865
    (&kernelContext_35)->blades_1 = blades_7;

#line 1865
    (&kernelContext_35)->frame_0 = frame_6;

#line 1865
    (&kernelContext_35)->field_0 = field_6;

#line 1865
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_6;

#line 1865
    (&kernelContext_35)->lights_0 = lights_6;

#line 1865
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_6;

#line 1865
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_6;

#line 1865
    (&kernelContext_35)->grassCard_0 = grassCard_6;

#line 1865
    (&kernelContext_35)->grassCardSampler_0 = grassCardSampler_6;

#line 1865
    (&kernelContext_35)->grassGround_0 = grassGround_6;

#line 1865
    (&kernelContext_35)->wind_0 = wind_6;

#line 1865
    (&kernelContext_35)->windDirectionLayer_0 = windDirectionLayer_6;

#line 1865
    (&kernelContext_35)->windSampler_0 = windSampler_6;

#line 1865
    (&kernelContext_35)->windIntensityLayer_0 = windIntensityLayer_6;

#line 1865
    (&kernelContext_35)->grassCells_0 = grassCells_6;

#line 1876
    uint capacity_0 = grass_6->limits_0.x;

    float2 _S210 = float2(1.0f, 1.0f);

#line 1878
    thread GrassInstance_natural_0 _S211 = instances_6[tile_14->slot_0.x * capacity_0 + capacity_0 - 1U - instance_id_2];

#line 1878
    GrassVertex_0 _S212 = grass_blade_vertex_0(&_S211, index_7, 3U, _S210, &kernelContext_35);

#line 1878
    thread bladeFarVertexMain_Result_0 _S213;

#line 1878
    (&_S213)->position_9 = _S212.position_2;

#line 1878
    (&_S213)->world_position_13 = _S212.world_position_0;

#line 1878
    (&_S213)->normal_12 = _S212.normal_2;

#line 1878
    (&_S213)->color_9 = _S212.color_1;

#line 1878
    (&_S213)->uv_5 = _S212.uv_0;

#line 1878
    (&_S213)->level_6 = _S212.level_1;

#line 1878
    (&_S213)->row_8 = _S212.row_2;

#line 1878
    (&_S213)->patch_7 = _S212.patch_1;

#line 1878
    return _S213;
}


#line 1878
struct shellVertexMain_Result_0
{
    float4 position_10 [[position]];
    float3 world_position_14 [[user(WORLD_POSITION)]];
    float3 rest_3 [[user(REST_POSITION)]];
    float3 normal_13 [[user(NORMAL)]];
    float footprint_2 [[user(FOOTPRINT)]];
    float occlusion_5 [[user(OCCLUSION)]];
    float4 fin_3 [[user(FIN)]];
};


#line 1878
[[vertex]] shellVertexMain_Result_0 shellVertexMain(uint index_8 [[vertex_id]], uint instance_id_3 [[instance_id]], GrassTile_0 constant* tile_15 [[buffer(2)]], GrassParams_0 constant* grass_7 [[buffer(1)]], GrassInstance_natural_0 device* instances_7 [[buffer(3)]], GrassBlade_natural_0 device* blades_8 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], GrassField_0 constant* field_7 [[buffer(7)]], uint device* cluster_lights_7 [[buffer(6)]], GpuLight_natural_0 device* lights_7 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_7 [[texture(1)]], sampler shadow_sampler_7 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_7 [[texture(0)]], sampler grassCardSampler_7 [[sampler(0)]], texture2d<float, access::sample> grassGround_7 [[texture(2)]], WindParams_0 constant* wind_7 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_7 [[texture(3)]], sampler windSampler_7 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_7 [[texture(4)]], GrassInstance_natural_0 device* grassCells_7 [[buffer(8)]])
{

#line 1878
    thread KernelContext_0 kernelContext_36;

#line 1878
    (&kernelContext_36)->tile_1 = tile_15;

#line 1878
    (&kernelContext_36)->grass_0 = grass_7;

#line 1878
    (&kernelContext_36)->instances_0 = instances_7;

#line 1878
    (&kernelContext_36)->blades_1 = blades_8;

#line 1878
    (&kernelContext_36)->frame_0 = frame_7;

#line 1878
    (&kernelContext_36)->field_0 = field_7;

#line 1878
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_7;

#line 1878
    (&kernelContext_36)->lights_0 = lights_7;

#line 1878
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_7;

#line 1878
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_7;

#line 1878
    (&kernelContext_36)->grassCard_0 = grassCard_7;

#line 1878
    (&kernelContext_36)->grassCardSampler_0 = grassCardSampler_7;

#line 1878
    (&kernelContext_36)->grassGround_0 = grassGround_7;

#line 1878
    (&kernelContext_36)->wind_0 = wind_7;

#line 1878
    (&kernelContext_36)->windDirectionLayer_0 = windDirectionLayer_7;

#line 1878
    (&kernelContext_36)->windSampler_0 = windSampler_7;

#line 1878
    (&kernelContext_36)->windIntensityLayer_0 = windIntensityLayer_7;

#line 1878
    (&kernelContext_36)->grassCells_0 = grassCells_7;

#line 1878
    uint _S214 = grass_shell_count_0(&kernelContext_36);

#line 1974
    uint _S215 = _S214 - 1U;
    uint quad_0 = index_8 / 6U;

#line 1975
    GrassShellVertex_0 _S216 = grass_sheet_vertex_0((&kernelContext_36)->tile_1->tile_0.xy + (float2(float(quad_0 % 16U), float(quad_0 / 16U)) + GRASS_CARD_CORNERS_0[index_8 % 6U]) * float2(((&kernelContext_36)->tile_1->tile_0.z / 16.0f)) , (&kernelContext_36)->field_0->layers_0[_S215 - min(instance_id_3, _S215)].x, float4(0.0f, 1.0f, 0.0f, 0.0f), (&kernelContext_36)->field_0->layers_0[_S215 - min(instance_id_3, _S215)].y, &kernelContext_36);

#line 1975
    thread shellVertexMain_Result_0 _S217;

#line 1975
    (&_S217)->position_10 = _S216.position_5;

#line 1975
    (&_S217)->world_position_14 = _S216.world_position_9;

#line 1975
    (&_S217)->rest_3 = _S216.rest_0;

#line 1975
    (&_S217)->normal_13 = _S216.normal_7;

#line 1975
    (&_S217)->footprint_2 = _S216.footprint_0;

#line 1975
    (&_S217)->occlusion_5 = _S216.occlusion_1;

#line 1975
    (&_S217)->fin_3 = _S216.fin_0;

#line 1975
    return _S217;
}


#line 1975
struct finVertexMain_Result_0
{
    float4 position_11 [[position]];
    float3 world_position_15 [[user(WORLD_POSITION)]];
    float3 rest_4 [[user(REST_POSITION)]];
    float3 normal_14 [[user(NORMAL)]];
    float footprint_3 [[user(FOOTPRINT)]];
    float occlusion_6 [[user(OCCLUSION)]];
    float4 fin_4 [[user(FIN)]];
};


#line 1975
[[vertex]] finVertexMain_Result_0 finVertexMain(uint index_9 [[vertex_id]], GrassTile_0 constant* tile_16 [[buffer(2)]], GrassParams_0 constant* grass_8 [[buffer(1)]], GrassInstance_natural_0 device* instances_8 [[buffer(3)]], GrassBlade_natural_0 device* blades_9 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_8 [[buffer(0)]], GrassField_0 constant* field_8 [[buffer(7)]], uint device* cluster_lights_8 [[buffer(6)]], GpuLight_natural_0 device* lights_8 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_8 [[texture(1)]], sampler shadow_sampler_8 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_8 [[texture(0)]], sampler grassCardSampler_8 [[sampler(0)]], texture2d<float, access::sample> grassGround_8 [[texture(2)]], WindParams_0 constant* wind_8 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_8 [[texture(3)]], sampler windSampler_8 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_8 [[texture(4)]], GrassInstance_natural_0 device* grassCells_8 [[buffer(8)]])
{

#line 1975
    thread KernelContext_0 kernelContext_37;

#line 1975
    (&kernelContext_37)->tile_1 = tile_16;

#line 1975
    (&kernelContext_37)->grass_0 = grass_8;

#line 1975
    (&kernelContext_37)->instances_0 = instances_8;

#line 1975
    (&kernelContext_37)->blades_1 = blades_9;

#line 1975
    (&kernelContext_37)->frame_0 = frame_8;

#line 1975
    (&kernelContext_37)->field_0 = field_8;

#line 1975
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_8;

#line 1975
    (&kernelContext_37)->lights_0 = lights_8;

#line 1975
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_8;

#line 1975
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_8;

#line 1975
    (&kernelContext_37)->grassCard_0 = grassCard_8;

#line 1975
    (&kernelContext_37)->grassCardSampler_0 = grassCardSampler_8;

#line 1975
    (&kernelContext_37)->grassGround_0 = grassGround_8;

#line 1975
    (&kernelContext_37)->wind_0 = wind_8;

#line 1975
    (&kernelContext_37)->windDirectionLayer_0 = windDirectionLayer_8;

#line 1975
    (&kernelContext_37)->windSampler_0 = windSampler_8;

#line 1975
    (&kernelContext_37)->windIntensityLayer_0 = windIntensityLayer_8;

#line 1975
    (&kernelContext_37)->grassCells_0 = grassCells_8;

#line 2012
    uint quad_1 = index_9 / 6U;
    uint _S218 = index_9 % 6U;
    uint _S219 = max(field_8->tiles_0.z / 4U, 1U);

    uint _S220 = quad_1 / (_S219 * 64U);

#line 2016
    uint _S221 = min(_S220, 1U);
    uint fin_line_0 = quad_1 / 64U % _S219;



    float band_4 = 4.0f * (&kernelContext_37)->field_0->origin_0.w;
    float across_1 = (float(fin_line_0) + 0.5f) * band_4;
    float stride_0 = (&kernelContext_37)->tile_1->tile_0.z / 16.0f;
    float _S222 = float(quad_1 / 4U % 16U);

#line 2024
    float along_4 = (_S222 + GRASS_CARD_CORNERS_0[_S218].x) * stride_0;
    float middle_along_0 = (_S222 + 0.5f) * stride_0;
    float share_2 = (float(quad_1 % 4U) + GRASS_CARD_CORNERS_0[_S218].y) / 4.0f;



    bool along_z_1 = _S221 == 0U;
    float2 _S223 = (&kernelContext_37)->tile_1->tile_0.xy;

#line 2031
    float2 side_2;

#line 2031
    if(along_z_1)
    {

#line 2031
        side_2 = float2(across_1, along_4);

#line 2031
    }
    else
    {

#line 2031
        side_2 = float2(along_4, across_1);

#line 2031
    }

#line 2031
    float2 rest_5 = _S223 + side_2;

    float2 _S224 = (&kernelContext_37)->tile_1->tile_0.xy;

#line 2033
    if(along_z_1)
    {

#line 2033
        side_2 = float2(across_1, middle_along_0);

#line 2033
    }
    else
    {

#line 2033
        side_2 = float2(middle_along_0, across_1);

#line 2033
    }

#line 2033
    float2 middle_0 = _S224 + side_2;
    if(along_z_1)
    {

#line 2034
        side_2 = float2(0.5f * band_4, 0.0f);

#line 2034
    }
    else
    {

#line 2034
        side_2 = float2(0.0f, 0.5f * band_4);

#line 2034
    }

#line 2034
    GrassGround_0 _S225 = grass_ground_under_0(middle_0, &kernelContext_37);

#line 2034
    GrassGround_0 _S226 = grass_ground_under_0(middle_0 - side_2, &kernelContext_37);

#line 2034
    GrassGround_0 _S227 = grass_ground_under_0(middle_0 + side_2, &kernelContext_37);

#line 2040
    float3 eye_0 = (&kernelContext_37)->frame_0->camera_position_0.xyz - float3(middle_0.x, _S225.height_1 + 0.5f * (&kernelContext_37)->field_0->stack_0.x, middle_0.y);
    float3 view_1 = eye_0 / float3(max(length(eye_0), 9.99999997475242708e-07f)) ;
    float facing_near_0 = dot(_S226.normal_6, view_1);
    float facing_far_0 = dot(_S227.normal_6, view_1);

#line 2043
    float graze_0;

    if((facing_near_0 * facing_far_0) <= 0.0f)
    {

#line 2045
        graze_0 = 0.0f;

#line 2045
    }
    else
    {

#line 2045
        graze_0 = min(abs(facing_near_0), abs(facing_far_0));

#line 2045
    }

    float fade_2 = saturate((0.5f - graze_0) / 0.19999998807907104f);


    if(along_z_1)
    {

#line 2050
        graze_0 = (&kernelContext_37)->tile_1->tile_0.x;

#line 2050
    }
    else
    {

#line 2050
        graze_0 = (&kernelContext_37)->tile_1->tile_0.y;

#line 2050
    }

    float4 _S228 = float4(float(_S221) + 1.0f, fade_2, graze_0 + across_1, 0.0f);

#line 2052
    float _S229 = grass_occlusion_at_0(share_2, &kernelContext_37);

#line 2052
    GrassShellVertex_0 _S230 = grass_sheet_vertex_0(rest_5, share_2, _S228, _S229, &kernelContext_37);

#line 2051
    thread GrassShellVertex_0 output_6 = _S230;

#line 2051
    bool _S231;

    if(fade_2 <= 0.0f)
    {

#line 2053
        _S231 = true;

#line 2053
    }
    else
    {

#line 2053
        _S231 = ((&kernelContext_37)->field_0->stack_0.z) <= 0.0f;

#line 2053
    }

#line 2053
    if(_S231)
    {


        (&output_6)->position_5 = float4(0.0f, 0.0f, -1.0f, 1.0f);

#line 2053
    }

#line 2059
    GrassShellVertex_0 _S232 = output_6;

#line 2059
    thread finVertexMain_Result_0 _S233;

#line 2059
    (&_S233)->position_11 = _S232.position_5;

#line 2059
    (&_S233)->world_position_15 = _S232.world_position_9;

#line 2059
    (&_S233)->rest_4 = _S232.rest_0;

#line 2059
    (&_S233)->normal_14 = _S232.normal_7;

#line 2059
    (&_S233)->footprint_3 = _S232.footprint_0;

#line 2059
    (&_S233)->occlusion_6 = _S232.occlusion_1;

#line 2059
    (&_S233)->fin_4 = _S232.fin_0;

#line 2059
    return _S233;
}

