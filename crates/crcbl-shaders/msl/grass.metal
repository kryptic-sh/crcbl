#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 709 "shaders/grass.slang"
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 730
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 743
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 897
constant array<float2, int(6)> GRASS_CARD_CORNERS_0 = { float2(0.0f, 0.0f), float2(1.0f, 0.0f), float2(0.0f, 1.0f), float2(1.0f, 0.0f), float2(1.0f, 1.0f), float2(0.0f, 1.0f) };

#line 505
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


#line 490
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 475
struct GrassParams_0
{
    uint4 limits_0;
    float4 screen_0;
};


#line 475
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_0;
    packed_float4 clump_1;
    packed_uint4 lanes_0;
};


#line 475
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


#line 475
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 475
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 475
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 170
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


#line 624
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


#line 1585
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


#line 664
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


#line 664
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


#line 1545
GrassBlade_0 grass_row_0(uint index_0, KernelContext_0 thread* kernelContext_0)
{
    GrassBlade_natural_0 _S1 = kernelContext_0->blades_1[min(index_0, max(kernelContext_0->grass_0->limits_0.y, 1U) - 1U)];

#line 1547
    GrassBlade_0 _S2 = { float4(_S1.root_color_0) , float4(_S1.tip_color_0) , float4(_S1.size_0) , float4(_S1.occlusion_0) , float4(_S1.glow_0) , float4(_S1.patch_0) , float4(_S1.shape_0) , float4(_S1.clump_0) , uint4(_S1.flags_0)  };

#line 1547
    return _S2;
}


#line 827
float grassCardLevel_0(float pixels_0)
{
    float _S3 = 64.0f / max(pixels_0, 0.00009999999747379f);

#line 829
    float level_0 = 0.0f;

#line 829
    uint step_0 = 1U;

    for(;;)
    {

#line 831
        if(step_0 < 7U)
        {
        }
        else
        {

#line 831
            break;
        }
        uint _S4 = step_0 - 1U;

#line 833
        float lower_0 = float(1U << _S4);
        if(_S3 >= lower_0)
        {

#line 834
            level_0 = float(_S4) + min((_S3 - lower_0) / lower_0, 1.0f);

#line 834
        }

#line 831
        step_0 = step_0 + 1U;

#line 831
    }

#line 847
    return min(floor(level_0 + 0.5f), 6.0f);
}


#line 1484
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


#line 1539
float grass_patch_0(uint clump_2)
{
    return grass_unit_pair_0(grass_hash_0(clump_2 ^ 374761393U)).x;
}


#line 1770
float2 grass_blade_lod_0(float distance_0, KernelContext_0 thread* kernelContext_1)
{
    float band_0 = kernelContext_1->field_0->blades_0.y;
    if(!(band_0 > 0.0f))
    {
        return float2(0.0f, 0.0f);
    }
    float half_band_0 = 0.5f * band_0;

    float _S5 = distance_0 - (kernelContext_1->field_0->blades_0.x - band_0);

#line 1779
    return float2(saturate(_S5 / half_band_0), saturate((_S5 - half_band_0) / half_band_0));
}


#line 1673
struct GrassBladeCurve_0
{
    float3 p0_0;
    float3 p1_0;
    float3 p2_0;
    float3 p3_0;
};


#line 1691
GrassBladeCurve_0 grass_blade_curve_0(const GrassInstance_natural_0 thread* blade_0, const GrassBlade_0 thread* row_0)
{

#line 1691
    float4 _S6 = float4(blade_0->root_0) ;

    float height_0 = _S6.w;

#line 1693
    float4 _S7 = float4(blade_0->facing_0) ;
    float _S8 = _S7.x;

#line 1694
    float _S9 = _S7.y;
    float tilt_0 = row_0->shape_0.x;
    float upright_0 = sqrt(max(1.0f - tilt_0 * tilt_0, 0.0f));
    float3 chord_0 = float3(0.0f, height_0 * upright_0, 0.0f) + float3(_S8, 0.0f, _S9) * float3((height_0 * tilt_0)) ;



    float3 bow_0 = float3(_S8 * upright_0, - tilt_0, _S9 * upright_0) * float3((row_0->shape_0.y * height_0)) ;

    thread GrassBladeCurve_0 curve_0;
    float3 _S10 = _S6.xyz;

#line 1704
    (&curve_0)->p0_0 = _S10;

#line 1704
    float3 _S11 = float3(0.3333333432674408f) ;
    (&curve_0)->p1_0 = _S10 + chord_0 * _S11 - bow_0;
    float3 _S12 = (float4(blade_0->lean_0) ).xyz;

#line 1706
    (&curve_0)->p2_0 = _S10 + chord_0 * float3(0.66666668653488159f)  - bow_0 + _S12 * _S11;
    (&curve_0)->p3_0 = _S10 + chord_0 + _S12;
    return curve_0;
}


struct GrassBladePoint_0
{
    float3 position_1;
    float3 normal_0;
};


#line 1728
GrassBladePoint_0 grass_blade_point_0(const GrassBladeCurve_0 thread* curve_1, const GrassInstance_natural_0 thread* blade_1, const GrassBlade_0 thread* row_1, float t_0, float edge_0, float scale_0, KernelContext_0 thread* kernelContext_2)
{

    float u_0 = 1.0f - t_0;
    float _S13 = 3.0f * u_0;

#line 1732
    float _S14 = _S13 * u_0;
    float _S15 = t_0 * t_0;

#line 1733
    float3 centre_0 = curve_1->p0_0 * float3((u_0 * u_0 * u_0))  + curve_1->p1_0 * float3((_S14 * t_0))  + curve_1->p2_0 * float3((_S13 * t_0 * t_0))  + curve_1->p3_0 * float3((_S15 * t_0)) ;

    float3 tangent_1 = (curve_1->p1_0 - curve_1->p0_0) * float3(_S14)  + (curve_1->p2_0 - curve_1->p1_0) * float3((6.0f * u_0 * t_0))  + (curve_1->p3_0 - curve_1->p2_0) * float3((3.0f * t_0 * t_0)) ;

#line 1735
    float4 _S16 = float4(blade_1->facing_0) ;
    float3 side_0 = float3(- _S16.y, 0.0f, _S16.x);

    float3 to_eye_0 = kernelContext_2->frame_0->camera_position_0.xyz - centre_0;
    float3 view_0 = to_eye_0 / float3(max(length(to_eye_0), 9.99999997475242708e-07f)) ;
    float3 perpendicular_0 = cross(tangent_1, view_0);
    float perpendicular_length_0 = length(perpendicular_0);

#line 1741
    float3 screen_side_0;
    if(perpendicular_length_0 > 9.99999997475242708e-07f)
    {

#line 1742
        screen_side_0 = perpendicular_0 / float3(perpendicular_length_0) ;

#line 1742
    }
    else
    {

#line 1742
        screen_side_0 = side_0;

#line 1742
    }
    if((dot(screen_side_0, side_0)) < 0.0f)
    {

#line 1743
        screen_side_0 = - screen_side_0;

#line 1743
    }
    float along_0 = dot(side_0, view_0);
    float side_seen_0 = sqrt(saturate(1.0f - along_0 * along_0));
    float3 direction_1 = side_0 * float3(side_seen_0)  + screen_side_0 * float3((1.0f - side_seen_0)) ;
    float seen_0 = length(direction_1 - view_0 * float3(dot(direction_1, view_0)) );

#line 1752
    float half_width_0 = _S16.z * (1.0f - _S15);
    float least_0 = 0.5f * (max(abs((((float4(centre_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w), 0.00009999999747379f) / max(kernelContext_2->grass_0->screen_0.x, 0.00009999999747379f));

#line 1753
    float widened_0;
    if((half_width_0 * seen_0) < least_0)
    {

#line 1754
        widened_0 = least_0 / max(seen_0, 0.00009999999747379f);

#line 1754
    }
    else
    {

#line 1754
        widened_0 = half_width_0;

#line 1754
    }

    thread GrassBladePoint_0 here_0;
    (&here_0)->position_1 = centre_0 + direction_1 * float3((edge_0 * widened_0 * scale_0)) ;


    float3 normal_1 = cross(tangent_1, side_0);

    (&here_0)->normal_0 = normalize(normal_1 / float3(max(length(normal_1), 9.99999997475242708e-07f))  + side_0 * float3((edge_0 * row_1->shape_0.z)) );
    return here_0;
}


#line 1648
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


#line 1792
GrassVertex_0 grass_blade_vertex_0(const GrassInstance_natural_0 thread* blade_2, uint index_1, uint segments_0, float2 lod_0, KernelContext_0 thread* kernelContext_3)
{

#line 1792
    uint4 _S17 = uint4(blade_2->lanes_0) ;

    uint _S18 = _S17.y;

#line 1794
    GrassBlade_0 _S19 = grass_row_0(_S18, kernelContext_3);

#line 1794
    thread GrassBlade_0 _S20 = _S19;

#line 1794
    GrassBladeCurve_0 _S21 = grass_blade_curve_0(blade_2, &_S20);


    float drop_0 = lod_0.x;
    float morph_0 = lod_0.y;

#line 1798
    float scale_1;


    if((_S17.w) != 0U)
    {

#line 1801
        scale_1 = 1.0f + drop_0;

#line 1801
    }
    else
    {

#line 1801
        scale_1 = 1.0f - drop_0;

#line 1801
    }

    bool tip_0 = index_1 >= (2U * segments_0);
    uint _S22 = min(index_1 / 2U, segments_0);

#line 1804
    float edge_1;
    if(tip_0)
    {

#line 1805
        edge_1 = 0.0f;

#line 1805
    }
    else
    {

#line 1805
        if((index_1 % 2U) == 0U)
        {

#line 1805
            edge_1 = -1.0f;

#line 1805
        }
        else
        {

#line 1805
            edge_1 = 1.0f;

#line 1805
        }

#line 1805
    }

#line 1805
    float t_1;
    if(tip_0)
    {

#line 1806
        t_1 = 1.0f;

#line 1806
    }
    else
    {

#line 1806
        t_1 = float(_S22) / float(segments_0);

#line 1806
    }

#line 1806
    thread GrassBladeCurve_0 _S23 = _S21;

#line 1806
    thread GrassBlade_0 _S24 = _S19;

#line 1806
    GrassBladePoint_0 _S25 = grass_blade_point_0(&_S23, blade_2, &_S24, t_1, edge_1, scale_1, kernelContext_3);
    thread GrassBladePoint_0 here_1 = _S25;

#line 1807
    bool _S26;

    if(morph_0 > 0.0f)
    {

#line 1809
        _S26 = !tip_0;

#line 1809
    }
    else
    {

#line 1809
        _S26 = false;

#line 1809
    }

#line 1809
    if(_S26)
    {

#line 1809
        _S26 = segments_0 != 3U;

#line 1809
    }
    else
    {

#line 1809
        _S26 = false;

#line 1809
    }

#line 1809
    if(_S26)
    {
        uint scaled_0 = _S22 * 3U;
        uint lower_1 = scaled_0 / segments_0;
        float within_0 = float(scaled_0 - lower_1 * segments_0) / float(segments_0);

        float _S27 = float(lower_1) / 3.0f;

#line 1815
        thread GrassBladeCurve_0 _S28 = _S21;

#line 1815
        thread GrassBlade_0 _S29 = _S19;

#line 1815
        GrassBladePoint_0 _S30 = grass_blade_point_0(&_S28, blade_2, &_S29, _S27, edge_1, scale_1, kernelContext_3);

        float _S31 = float(lower_1 + 1U) / 3.0f;

#line 1817
        thread GrassBladeCurve_0 _S32 = _S21;

#line 1817
        thread GrassBlade_0 _S33 = _S19;

#line 1817
        GrassBladePoint_0 _S34 = grass_blade_point_0(&_S32, blade_2, &_S33, _S31, edge_1, scale_1, kernelContext_3);

#line 1817
        float3 _S35 = float3((1.0f - within_0)) ;

#line 1817
        float3 _S36 = float3(within_0) ;

        float3 far_normal_0 = _S30.normal_0 * _S35 + _S34.normal_0 * _S36;

#line 1819
        float3 _S37 = float3((1.0f - morph_0)) ;

#line 1819
        float3 _S38 = float3(morph_0) ;
        (&here_1)->position_1 = (&here_1)->position_1 * _S37 + (_S30.position_1 * _S35 + _S34.position_1 * _S36) * _S38;
        (&here_1)->normal_0 = (&here_1)->normal_0 * _S37 + far_normal_0 * _S38;

#line 1809
    }

#line 1809
    float3 blade_normal_0;

#line 1826
    if((dot((&here_1)->normal_0, kernelContext_3->frame_0->camera_position_0.xyz - (&here_1)->position_1)) < 0.0f)
    {

#line 1826
        blade_normal_0 = - (&here_1)->normal_0;

#line 1826
    }
    else
    {

#line 1826
        blade_normal_0 = (&here_1)->normal_0;

#line 1826
    }

#line 1826
    float3 base_0;



    if((_S19.flags_0.y) == 1U)
    {

#line 1830
        base_0 = float3(0.0f, 1.0f, 0.0f);

#line 1830
    }
    else
    {

#line 1830
        base_0 = (float4(blade_2->ground_0) ).xyz;

#line 1830
    }

#line 1830
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


#line 1557
float3 grass_style_0(float3 color_2, const GrassBlade_0 thread* row_3, float along_1, float patch_2)
{

#line 1557
    float4 _S40 = row_3->occlusion_0;

    float reach_0 = row_3->occlusion_0.w;

#line 1559
    float occluded_0;
    if(reach_0 > 0.0f)
    {

#line 1560
        occluded_0 = saturate(1.0f - along_1 / reach_0);

#line 1560
    }
    else
    {

#line 1560
        occluded_0 = 0.0f;

#line 1560
    }


    float start_0 = row_3->glow_0.w;

    return mix(mix(color_2, _S40.xyz, float3(occluded_0) ), row_3->patch_0.xyz, float3((row_3->patch_0.w * patch_2)) ) + row_3->glow_0.xyz * float3(saturate((along_1 - start_0) / max(1.0f - start_0, 0.00009999999747379f))) ;
}


#line 1411
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_4)
{
    uint _S41 = max(kernelContext_4->frame_0->cluster_grid_0.x, 1U);
    uint _S42 = max(kernelContext_4->frame_0->cluster_grid_0.y, 1U);
    uint _S43 = max(kernelContext_4->frame_0->cluster_grid_0.z, 1U);
    uint _S44 = max(kernelContext_4->frame_0->cluster_grid_0.w, 1U);

#line 1421
    uint _S45 = uint(pixel_0.x) / _S44;

#line 1421
    uint _S46 = min(_S45, _S41 - 1U);
    uint _S47 = uint(pixel_0.y) / _S44;

    float scale_2 = 24.0f / log2(10000.0f);

#line 1432
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_2 + - scale_2 * log2(0.10000000149011612f)), 0.0f, float(_S43 - 1U))) * _S42 + min(_S47, _S42 - 1U)) * _S41 + _S46;
}


#line 1389
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

#line 1408
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 930
float4 atlas_rect_0(uint tile_2, KernelContext_0 thread* kernelContext_5)
{
    return kernelContext_5->frame_0->shadow_atlas_rect_0[tile_2];
}


#line 930
float4 atlas_rect_1(uint tile_3, KernelContext_0 thread* kernelContext_6)
{
    return kernelContext_6->frame_0->shadow_atlas_rect_0[tile_3];
}


#line 945
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 940
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_7)
{
    return rect_1.x / kernelContext_7->frame_0->shadow_params_0.x;
}


#line 907
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 919
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_8)
{

#line 919
    uint _S48;

    if(uint(pixel_1.x) < (kernelContext_8->frame_0->shadow_filter_0.z))
    {

#line 921
        _S48 = kernelContext_8->frame_0->shadow_filter_0.x;

#line 921
    }
    else
    {

#line 921
        _S48 = kernelContext_8->frame_0->shadow_filter_0.y;

#line 921
    }

#line 921
    return _S48;
}


#line 935
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_9)
{
    return kernelContext_9->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 935
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 925
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 950
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_11)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S49 = spoke_0.x;

#line 955
    float _S50 = rotation_0.x;

#line 955
    float _S51 = spoke_0.y;

#line 955
    float _S52 = rotation_0.y;


    float _S53 = ((kernelContext_11->shadow_atlas_0).sample_compare((kernelContext_11->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S49 * _S50 - _S51 * _S52, _S49 * _S52 + _S51 * _S50) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 958
    return _S53;
}


#line 950
float tile_tap_1(float4 rect_6, float2 texel_step_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_12)
{

    float2 tile_min_1 = float2(0.5f, 0.5f) * texel_step_1;

    float _S54 = spoke_1.x;

#line 955
    float _S55 = rotation_1.x;

#line 955
    float _S56 = spoke_1.y;

#line 955
    float _S57 = rotation_1.y;


    float _S58 = ((kernelContext_12->shadow_atlas_0).sample_compare((kernelContext_12->shadow_sampler_0), (atlas_uv_0(rect_6, clamp(tile_uv_2 + float2(_S54 * _S55 - _S56 * _S57, _S54 * _S57 + _S56 * _S55) * texel_step_1, tile_min_1, float2(1.0f)  - tile_min_1))), (reference_1), level((0.0f))));

#line 958
    return _S58;
}


#line 1021
float tile_box_pcf_0(uint tile_4, float2 tile_uv_3, float reference_2, KernelContext_0 thread* kernelContext_13)
{

#line 1021
    float4 _S59 = atlas_rect_1(tile_4, kernelContext_13);


    if(atlas_rect_is_empty_0(_S59))
    {
        return 1.0f;
    }

#line 1026
    float2 _S60 = atlas_step_1(_S59, kernelContext_13);

#line 1026
    int y_0 = int(-1);

#line 1026
    float visibility_0 = 0.0f;

#line 1031
    for(;;)
    {

#line 1031
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 1031
            break;
        }

#line 1031
        int x_0 = int(-1);

        for(;;)
        {

#line 1033
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 1033
                break;
            }

#line 1033
            float _S61 = tile_tap_1(_S59, _S60, tile_uv_3, float2(float(x_0), float(y_0)), float2(1.0f, 0.0f), reference_2, kernelContext_13);

            float visibility_1 = visibility_0 + _S61;

#line 1033
            x_0 = x_0 + int(1);

#line 1033
            visibility_0 = visibility_1;

#line 1033
        }

#line 1031
        y_0 = y_0 + int(1);

#line 1031
    }

#line 1039
    return visibility_0 / 9.0f;
}


#line 913
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 961
float tile_pcf_0(uint tile_5, float2 tile_uv_4, float reference_3, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_14)
{
    float2 rotation_2 = shadow_rotation_0(pixel_3);

#line 963
    float4 _S62 = atlas_rect_0(tile_5, kernelContext_14);

    if(atlas_rect_is_empty_0(_S62))
    {
        return 1.0f;
    }

#line 967
    float2 _S63 = atlas_step_0(_S62, kernelContext_14);



    thread array<float, int(5)> probe_taps_0;

#line 971
    uint spot_0 = 0U;

#line 971
    float probe_0 = 0.0f;

    for(;;)
    {

#line 973
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 973
            break;
        }

#line 973
        float _S64 = tile_tap_0(_S62, _S63, tile_uv_4, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , rotation_2, reference_3, kernelContext_14);

        probe_taps_0[spot_0] = _S64;


        float probe_1 = probe_0 + _S64;

#line 973
        spot_0 = spot_0 + 1U;

#line 973
        probe_0 = probe_1;

#line 973
    }

#line 984
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 990
    uint index_2 = 1U;

#line 990
    float visibility_2 = probe_taps_0[int(0)];

#line 998
    for(;;)
    {

#line 998
        if(index_2 < 23U)
        {
        }
        else
        {

#line 998
            break;
        }

#line 998
        float _S65 = tile_tap_0(_S62, _S63, tile_uv_4, SHADOW_DISC_0[index_2] * float2(radius_2) , rotation_2, reference_3, kernelContext_14);

        float visibility_3 = visibility_2 + _S65;

#line 998
        index_2 = index_2 + 1U;

#line 998
        visibility_2 = visibility_3;

#line 998
    }

#line 1003
    float visibility_4 = visibility_2 + probe_taps_0[int(1)];

#line 1003
    float2 _S66 = float2(radius_2) ;

#line 1003
    float _S67 = tile_tap_0(_S62, _S63, tile_uv_4, float2(0.43503299355506897f, 0.75919097661972046f) * _S66, rotation_2, reference_3, kernelContext_14);


    float visibility_5 = visibility_4 + _S67 + probe_taps_0[int(2)];

#line 1006
    float _S68 = tile_tap_0(_S62, _S63, tile_uv_4, float2(0.82610201835632324f, -0.38168001174926758f) * _S66, rotation_2, reference_3, kernelContext_14);


    float visibility_6 = visibility_5 + _S68 + probe_taps_0[int(3)];

#line 1009
    float _S69 = tile_tap_0(_S62, _S63, tile_uv_4, float2(-0.31940698623657227f, -0.88803398609161377f) * _S66, rotation_2, reference_3, kernelContext_14);


    float visibility_7 = visibility_6 + _S69 + probe_taps_0[int(4)];

#line 1012
    index_2 = 30U;

#line 1012
    visibility_2 = visibility_7;
    for(;;)
    {

#line 1013
        if(index_2 < 32U)
        {
        }
        else
        {

#line 1013
            break;
        }

#line 1013
        float _S70 = tile_tap_0(_S62, _S63, tile_uv_4, SHADOW_DISC_0[index_2] * _S66, rotation_2, reference_3, kernelContext_14);

        float visibility_8 = visibility_2 + _S70;

#line 1013
        index_2 = index_2 + 1U;

#line 1013
        visibility_2 = visibility_8;

#line 1013
    }

#line 1018
    return visibility_2 / 32.0f;
}


#line 1042
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_5, float reference_4, float2 rotation_3, KernelContext_0 thread* kernelContext_15)
{
    float2 texel_0 = kernelContext_15->frame_0->shadow_params_0.xy;

#line 1044
    float4 _S71 = atlas_rect_0(cascade_0, kernelContext_15);

#line 1044
    float2 _S72 = atlas_step_0(_S71, kernelContext_15);


    float2 _S73 = float2(0.5f, 0.5f) * _S72;


    float2 _S74 = float2(1.0f, 1.0f);

#line 1050
    float2 _S75 = _S74 / texel_0;

#line 1060
    float _S76 = rotation_3.x;

#line 1060
    float _S77 = rotation_3.y;

    float2 _S78 = float2(1.0f)  - _S73;

#line 1067
    float2 _S79 = _S75 - _S74;
    int3 _S80 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(1.41421604156494141f * _S76 - 0.0f * _S77, 1.41421604156494141f * _S77 + 0.0f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_1 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S80)).xy), uint(((_S80)).z)));

#line 1068
    float sum_0;

#line 1068
    float found_0;
    if(depth_1 > reference_4)
    {

#line 1069
        sum_0 = depth_1;

#line 1069
        found_0 = 1.0f;

#line 1069
    }
    else
    {

#line 1069
        sum_0 = 0.0f;

#line 1069
        found_0 = 0.0f;

#line 1069
    }

#line 1068
    int3 _S81 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-1.80617594718933105f * _S76 - 1.65460801124572754f * _S77, -1.80617594718933105f * _S77 + 1.65460801124572754f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_2 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S81)).xy), uint(((_S81)).z)));
    if(depth_2 > reference_4)
    {

        float found_1 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_2;

#line 1072
        found_0 = found_1;

#line 1069
    }

#line 1068
    int3 _S82 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(0.27646398544311523f * _S76 - -3.15016794204711914f * _S77, 0.27646398544311523f * _S77 + -3.15016794204711914f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_3 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S82)).xy), uint(((_S82)).z)));
    if(depth_3 > reference_4)
    {

        float found_2 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_3;

#line 1072
        found_0 = found_2;

#line 1069
    }

#line 1068
    int3 _S83 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(2.27656793594360352f * _S76 - 2.96938395500183105f * _S77, 2.27656793594360352f * _S77 + 2.96938395500183105f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_4 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S83)).xy), uint(((_S83)).z)));
    if(depth_4 > reference_4)
    {

        float found_3 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_4;

#line 1072
        found_0 = found_3;

#line 1069
    }

#line 1068
    int3 _S84 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-4.17778396606445312f * _S76 - -0.73899197578430176f * _S77, -4.17778396606445312f * _S77 + -0.73899197578430176f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_5 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S84)).xy), uint(((_S84)).z)));
    if(depth_5 > reference_4)
    {

        float found_4 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_5;

#line 1072
        found_0 = found_4;

#line 1069
    }

#line 1068
    int3 _S85 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(3.95756006240844727f * _S76 - -2.51747989654541016f * _S77, 3.95756006240844727f * _S77 + -2.51747989654541016f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_6 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S85)).xy), uint(((_S85)).z)));
    if(depth_6 > reference_4)
    {

        float found_5 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_6;

#line 1072
        found_0 = found_5;

#line 1069
    }

#line 1068
    int3 _S86 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-1.32372796535491943f * _S76 - 4.92420005798339844f * _S77, -1.32372796535491943f * _S77 + 4.92420005798339844f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_7 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S86)).xy), uint(((_S86)).z)));
    if(depth_7 > reference_4)
    {

        float found_6 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_7;

#line 1072
        found_0 = found_6;

#line 1069
    }

#line 1068
    int3 _S87 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-2.52448797225952148f * _S76 - -4.86075210571289062f * _S77, -2.52448797225952148f * _S77 + -4.86075210571289062f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_8 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S87)).xy), uint(((_S87)).z)));
    if(depth_8 > reference_4)
    {

        float found_7 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_8;

#line 1072
        found_0 = found_7;

#line 1069
    }

#line 1068
    int3 _S88 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(5.47713613510131836f * _S76 - 2.00024008750915527f * _S77, 5.47713613510131836f * _S77 + 2.00024008750915527f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_9 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S88)).xy), uint(((_S88)).z)));
    if(depth_9 > reference_4)
    {

        float found_8 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_9;

#line 1072
        found_0 = found_8;

#line 1069
    }

#line 1068
    int3 _S89 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-5.69804811477661133f * _S76 - 2.35207200050354004f * _S77, -5.69804811477661133f * _S77 + 2.35207200050354004f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_10 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S89)).xy), uint(((_S89)).z)));
    if(depth_10 > reference_4)
    {

        float found_9 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_10;

#line 1072
        found_0 = found_9;

#line 1069
    }

#line 1068
    int3 _S90 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(2.74683189392089844f * _S76 - -5.86983203887939453f * _S77, 2.74683189392089844f * _S77 + -5.86983203887939453f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_11 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S90)).xy), uint(((_S90)).z)));
    if(depth_11 > reference_4)
    {

        float found_10 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_11;

#line 1072
        found_0 = found_10;

#line 1069
    }

#line 1068
    int3 _S91 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(2.02983999252319336f * _S76 - 6.47145605087280273f * _S77, 2.02983999252319336f * _S77 + 6.47145605087280273f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_12 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S91)).xy), uint(((_S91)).z)));
    if(depth_12 > reference_4)
    {

        float found_11 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_12;

#line 1072
        found_0 = found_11;

#line 1069
    }

#line 1068
    int3 _S92 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-6.11796808242797852f * _S76 - -3.54548811912536621f * _S77, -6.11796808242797852f * _S77 + -3.54548811912536621f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_13 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S92)).xy), uint(((_S92)).z)));
    if(depth_13 > reference_4)
    {

        float found_12 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_13;

#line 1072
        found_0 = found_12;

#line 1069
    }

#line 1068
    int3 _S93 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(7.17707204818725586f * _S76 - -1.57785594463348389f * _S77, 7.17707204818725586f * _S77 + -1.57785594463348389f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_14 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S93)).xy), uint(((_S93)).z)));
    if(depth_14 > reference_4)
    {

        float found_13 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_14;

#line 1072
        found_0 = found_13;

#line 1069
    }

#line 1068
    int3 _S94 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-4.38005590438842773f * _S76 - 6.23017597198486328f * _S77, -4.38005590438842773f * _S77 + 6.23017597198486328f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_15 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S94)).xy), uint(((_S94)).z)));
    if(depth_15 > reference_4)
    {

        float found_14 = found_0 + 1.0f;

#line 1072
        sum_0 = sum_0 + depth_15;

#line 1072
        found_0 = found_14;

#line 1069
    }

#line 1068
    int3 _S95 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(-1.01189601421356201f * _S76 - -7.80872011184692383f * _S77, -1.01189601421356201f * _S77 + -7.80872011184692383f * _S76) * _S72, _S73, _S78)) * _S75, _S79)), int(0));

#line 1068
    float depth_16 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S95)).xy), uint(((_S95)).z)));
    if(depth_16 > reference_4)
    {
        float sum_1 = sum_0 + depth_16;

#line 1071
        found_0 = found_0 + 1.0f;

#line 1071
        sum_0 = sum_1;

#line 1069
    }

#line 1076
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 1087
    float _S96 = 2.0f * kernelContext_15->frame_0->cascade_far_0[cascade_0];

#line 1087
    float separation_0 = (sum_0 / found_0 - reference_4) * (_S96 + 40.0f);

#line 1087
    float _S97 = tile_texels_0(_S71, kernelContext_15);

    return clamp(separation_0 * 0.01999999955296516f / (_S96 / _S97), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_1, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_16)
{

#line 1095
    float4 _S98 = atlas_rect_0(cascade_1, kernelContext_16);

#line 1129
    if(atlas_rect_is_empty_0(_S98))
    {


        return 1.0f;
    }
    float _S99 = 2.0f * kernelContext_16->frame_0->cascade_far_0[cascade_1];

#line 1135
    float _S100 = tile_texels_0(_S98, kernelContext_16);

#line 1135
    float texel_world_0 = _S99 / _S100;

#line 1142
    float4 clip_0 = (((float4(world_position_1 + geometric_normal_1 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1146
    bool _S101;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1147
        _S101 = true;

#line 1147
    }
    else
    {

#line 1147
        _S101 = (ndc_0.z) <= 0.0f;

#line 1147
    }

#line 1147
    if(_S101)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 1157
    uint _S102 = shadow_filter_mode_0(pixel_4, kernelContext_16);

#line 1174
    if(_S102 == 2U)
    {

#line 1174
        float _S103 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_0.z, kernelContext_16);

        return _S103;
    }
    if(_S102 == 1U)
    {

#line 1178
        float _S104 = tile_pcf_0(cascade_1, tile_uv_6, ndc_0.z, pixel_4, 2.0f, kernelContext_16);



        return _S104;
    }

    float _S105 = ndc_0.z;

#line 1185
    float _S106 = sun_penumbra_texels_0(cascade_1, tile_uv_6, _S105, shadow_rotation_0(pixel_4), kernelContext_16);

#line 1185
    float _S107 = tile_pcf_0(cascade_1, tile_uv_6, _S105, pixel_4, _S106, kernelContext_16);
    return _S107;
}

float sun_visibility_0(float3 world_position_2, float3 to_light_3, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_17)
{
    uint cascade_2;

#line 1191
    bool covered_0;

#line 1200
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 1212
    float eye_distance_0 = length(world_position_2 - kernelContext_17->frame_0->camera_position_0.xyz);

#line 1212
    uint index_3 = 0U;

#line 1220
    for(;;)
    {

#line 1220
        if(index_3 < 2U)
        {
        }
        else
        {

#line 1220
            covered_0 = false;

#line 1220
            cascade_2 = 1U;

#line 1220
            break;
        }
        if(eye_distance_0 < kernelContext_17->frame_0->cascade_far_0[index_3])
        {

#line 1222
            covered_0 = true;

#line 1222
            cascade_2 = index_3;



            break;
        }

#line 1220
        index_3 = index_3 + 1U;

#line 1220
    }

#line 1229
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 1229
    }

#line 1229
    float _S108 = cascade_visibility_0(cascade_2, world_position_2, to_light_3, geometric_normal_2, pixel_5, kernelContext_17);

#line 1236
    uint _S109 = cascade_2 + 1U;

#line 1236
    if(_S109 >= 2U)
    {



        return _S108;
    }

#line 1249
    float band_1 = kernelContext_17->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_17->frame_0->cascade_far_0[cascade_2] - band_1)) / band_1);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S108;
    }

#line 1257
    float _S110 = cascade_visibility_0(_S109, world_position_2, to_light_3, geometric_normal_2, pixel_5, kernelContext_17);

#line 1268
    return mix(_S108, _S110, blend_0);
}


#line 1357
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S111 = axis_1.x;

#line 1360
    float _S112 = axis_1.y;

#line 1360
    bool _S113;

#line 1360
    if(_S111 >= _S112)
    {

#line 1360
        _S113 = _S111 >= (axis_1.z);

#line 1360
    }
    else
    {

#line 1360
        _S113 = false;

#line 1360
    }

#line 1360
    uint _S114;

#line 1360
    if(_S113)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1362
            _S114 = 0U;

#line 1362
        }
        else
        {

#line 1362
            _S114 = 1U;

#line 1362
        }

#line 1362
        return _S114;
    }
    if(_S112 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1366
            _S114 = 2U;

#line 1366
        }
        else
        {

#line 1366
            _S114 = 3U;

#line 1366
        }

#line 1366
        return _S114;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1368
        _S114 = 4U;

#line 1368
    }
    else
    {

#line 1368
        _S114 = 5U;

#line 1368
    }

#line 1368
    return _S114;
}


#line 902
uint light_tile_0(uint tile_6)
{
    return 2U + tile_6;
}


#line 1271
float punctual_visibility_0(uint tile_7, float3 world_position_3, float3 to_light_4, float n_dot_l_1, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_18)
{

    uint atlas_0 = light_tile_0(tile_7);

#line 1274
    float4 _S115 = atlas_rect_0(atlas_0, kernelContext_18);

    if(atlas_rect_is_empty_0(_S115))
    {


        return 1.0f;
    }

#line 1280
    float _S116 = tile_texels_0(_S115, kernelContext_18);

    float texel_world_1 = map_world_0 / _S116;

#line 1292
    float4 clip_1 = (((float4(world_position_3 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(3)]))));

#line 1299
    float _S117 = clip_1.w;

#line 1299
    if(_S117 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S117) ;

#line 1303
    bool _S118;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1304
        _S118 = true;

#line 1304
    }
    else
    {

#line 1304
        _S118 = (ndc_1.z) <= 0.0f;

#line 1304
    }

#line 1304
    if(_S118)
    {

#line 1304
        _S118 = true;

#line 1304
    }
    else
    {

#line 1304
        _S118 = (ndc_1.z) > 1.0f;

#line 1304
    }

#line 1304
    if(_S118)
    {

#line 1311
        return 1.0f;
    }



    float2 tile_uv_7 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 1316
    uint _S119 = shadow_filter_mode_0(pixel_6, kernelContext_18);

#line 1325
    if(_S119 == 2U)
    {

#line 1325
        float _S120 = tile_box_pcf_0(atlas_0, tile_uv_7, ndc_1.z, kernelContext_18);

        return _S120;
    }

#line 1327
    float _S121 = tile_pcf_0(atlas_0, tile_uv_7, ndc_1.z, pixel_6, 2.0f, kernelContext_18);

    return _S121;
}


#line 1371
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_4, float3 to_light_5, float n_dot_l_2, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_19)
{

    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_4 - (float4(light_0->position_0) ).xyz;

#line 1379
    float _S122 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_4, to_light_5, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_19);

#line 1385
    return _S122;
}


#line 1332
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_8, float3 world_position_5, float3 to_light_6, float n_dot_l_3, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_20)
{

    if(n_dot_l_3 <= 0.0f)
    {


        return 1.0f;
    }

#line 1339
    float4 _S123 = float4(light_1->direction_0) ;

#line 1346
    float cos_outer_1 = _S123.w;

#line 1346
    float _S124 = punctual_visibility_0(tile_8, world_position_5, to_light_6, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (float4(light_1->position_0) ).xyz, normalize(_S123.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_20);

#line 1353
    return _S124;
}


#line 1573
float3 grass_light_0(float3 world_position_6, float3 normal_3, float2 pixel_9, KernelContext_0 thread* kernelContext_21)
{

#line 1573
    uint _S125 = froxel_of_0(pixel_9, (((float4(world_position_6, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w, kernelContext_21);

#line 1584
    uint base_2 = _S125 * 17U;
    uint _S126 = min(kernelContext_21->cluster_lights_0[base_2], 16U);

    float3 _S127 = float3(0.0f, 0.0f, 0.0f);

#line 1587
    uint slot_1 = 0U;

#line 1587
    float3 direct_0 = _S127;
    for(;;)
    {

#line 1588
        if(slot_1 < _S126)
        {
        }
        else
        {

#line 1588
            break;
        }

#line 1588
        thread GpuLight_natural_0 _S128 = kernelContext_21->lights_0[kernelContext_21->cluster_lights_0[base_2 + 1U + slot_1]];

#line 1588
        uint _S129 = (&_S128)->kind_0;


        if(((&_S128)->kind_0) == 3U)
        {



            slot_1 = slot_1 + 1U;

#line 1588
            continue;
        }

#line 1601
        bool _S130 = _S129 == 0U;

#line 1601
        float3 to_light_7;

#line 1601
        float reach_1;

#line 1601
        if(_S130)
        {

#line 1601
            to_light_7 = normalize((float4((&_S128)->direction_0) ).xyz);

#line 1601
            reach_1 = 1.0f;

#line 1601
        }
        else
        {

#line 1601
            float4 _S131 = float4((&_S128)->position_0) ;

#line 1608
            float3 offset_0 = _S131.xyz - world_position_6;
            float distance_3 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;
            float reach_2 = punctual_falloff_0(distance_3, _S131.w);
            if(_S129 == 2U)
            {

#line 1612
                float4 _S132 = float4((&_S128)->direction_0) ;

#line 1612
                reach_1 = reach_2 * spot_cone_0(to_light_8, _S132.xyz, _S132.w, (&_S128)->cos_inner_0);

#line 1612
            }
            else
            {

#line 1612
                reach_1 = reach_2;

#line 1612
            }

#line 1612
            to_light_7 = to_light_8;

#line 1601
        }

#line 1619
        float n_dot_l_4 = dot(normal_3, to_light_7);

#line 1619
        float reach_3;
        if(_S130)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 1623
            float _S133 = sun_visibility_0(world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, &sun_cascade_0, &sun_fade_0, kernelContext_21);

#line 1623
            reach_3 = _S133;

#line 1620
        }
        else
        {

#line 1627
            if(_S129 == 1U)
            {

#line 1627
                uint _S134 = (&_S128)->shadow_tile_0;

                if(((&_S128)->shadow_tile_0) <= 8U)
                {

#line 1629
                    float _S135 = point_visibility_0(&_S128, _S134, world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, kernelContext_21);

#line 1629
                    reach_3 = reach_1 * _S135;

#line 1629
                }
                else
                {

#line 1629
                    reach_3 = reach_1;

#line 1629
                }

#line 1627
            }
            else
            {

#line 1627
                uint _S136 = (&_S128)->shadow_tile_0;

#line 1635
                if(((&_S128)->shadow_tile_0) < 14U)
                {

#line 1635
                    float _S137 = spot_visibility_0(&_S128, _S136, world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, kernelContext_21);

#line 1635
                    reach_3 = reach_1 * _S137;

#line 1635
                }
                else
                {

#line 1635
                    reach_3 = reach_1;

#line 1635
                }

#line 1627
            }

#line 1620
        }

#line 1620
        direct_0 = direct_0 + (float4((&_S128)->color_0) ).xyz * float3((max(n_dot_l_4, 0.0f) * reach_3)) ;

#line 1588
        slot_1 = slot_1 + 1U;

#line 1588
    }

#line 1644
    return direct_0 + kernelContext_21->frame_0->ambient_0.xyz;
}


#line 1644
struct pixelOutput_0
{
    float4 output_1 [[color(0)]];
};


#line 1644
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


#line 1644
float3 grass_style_1(float3 _S138, uint _S139, float _S140, float _S141, KernelContext_0 thread* kernelContext_22)
{

#line 1644
    GrassBlade_natural_0 device* _S142 = kernelContext_22->blades_1+_S139;

#line 1644
    float4 _S143 = float4(_S142->occlusion_0) ;

#line 1559
    float reach_4 = _S143.w;

#line 1559
    float occluded_1;
    if(reach_4 > 0.0f)
    {

#line 1560
        occluded_1 = saturate(1.0f - _S140 / reach_4);

#line 1560
    }
    else
    {

#line 1560
        occluded_1 = 0.0f;

#line 1560
    }

#line 1560
    float4 _S144 = float4(_S142->patch_0) ;

#line 1560
    float4 _S145 = float4(_S142->glow_0) ;


    float start_1 = _S145.w;

    return mix(mix(_S138, _S143.xyz, float3(occluded_1) ), _S144.xyz, float3((_S144.w * _S141)) ) + _S145.xyz * float3(saturate((_S140 - start_1) / max(1.0f - start_1, 0.00009999999747379f))) ;
}


#line 1920
[[fragment]] pixelOutput_0 bladeFragmentMain(pixelInput_0 _S146 [[stage_in]], float4 position_3 [[position]], GrassTile_0 constant* tile_9 [[buffer(2)]], GrassParams_0 constant* grass_1 [[buffer(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GrassField_0 constant* field_1 [[buffer(7)]], uint device* cluster_lights_1 [[buffer(6)]], GpuLight_natural_0 device* lights_1 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_1 [[texture(0)]], sampler grassCardSampler_1 [[sampler(0)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(3)]], sampler windSampler_1 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(4)]], GrassInstance_natural_0 device* grassCells_1 [[buffer(8)]])
{

#line 1920
    thread KernelContext_0 kernelContext_23;

#line 1920
    (&kernelContext_23)->tile_1 = tile_9;

#line 1920
    (&kernelContext_23)->grass_0 = grass_1;

#line 1920
    (&kernelContext_23)->instances_0 = instances_1;

#line 1920
    (&kernelContext_23)->blades_1 = blades_2;

#line 1920
    (&kernelContext_23)->frame_0 = frame_1;

#line 1920
    (&kernelContext_23)->field_0 = field_1;

#line 1920
    (&kernelContext_23)->cluster_lights_0 = cluster_lights_1;

#line 1920
    (&kernelContext_23)->lights_0 = lights_1;

#line 1920
    (&kernelContext_23)->shadow_atlas_0 = shadow_atlas_1;

#line 1920
    (&kernelContext_23)->shadow_sampler_0 = shadow_sampler_1;

#line 1920
    (&kernelContext_23)->grassCard_0 = grassCard_1;

#line 1920
    (&kernelContext_23)->grassCardSampler_0 = grassCardSampler_1;

#line 1920
    (&kernelContext_23)->grassGround_0 = grassGround_1;

#line 1920
    (&kernelContext_23)->wind_0 = wind_1;

#line 1920
    (&kernelContext_23)->windDirectionLayer_0 = windDirectionLayer_1;

#line 1920
    (&kernelContext_23)->windSampler_0 = windSampler_1;

#line 1920
    (&kernelContext_23)->windIntensityLayer_0 = windIntensityLayer_1;

#line 1920
    (&kernelContext_23)->grassCells_0 = grassCells_1;

#line 1920
    float3 _S147 = grass_style_1(_S146.color_3, _S146.row_4, _S146.uv_1.y, _S146.patch_3, &kernelContext_23);

#line 1920
    float3 _S148 = grass_light_0(_S146.world_position_7, normalize(_S146.normal_4), position_3.xy, &kernelContext_23);

#line 1920
    pixelOutput_0 _S149 = { float4(_S147 * _S148, 1.0f) };



    return _S149;
}


#line 1924
struct pixelOutput_1
{
    float4 output_2 [[color(0)]];
};


#line 1924
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


#line 1929
[[fragment]] pixelOutput_1 fragmentMain(pixelInput_1 _S150 [[stage_in]], float4 position_4 [[position]], GrassTile_0 constant* tile_10 [[buffer(2)]], GrassParams_0 constant* grass_2 [[buffer(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(3)]], GrassBlade_natural_0 device* blades_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GrassField_0 constant* field_2 [[buffer(7)]], uint device* cluster_lights_2 [[buffer(6)]], GpuLight_natural_0 device* lights_2 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_2 [[texture(0)]], sampler grassCardSampler_2 [[sampler(0)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(3)]], sampler windSampler_2 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(4)]], GrassInstance_natural_0 device* grassCells_2 [[buffer(8)]])
{

#line 1929
    thread KernelContext_0 kernelContext_24;

#line 1929
    (&kernelContext_24)->tile_1 = tile_10;

#line 1929
    (&kernelContext_24)->grass_0 = grass_2;

#line 1929
    (&kernelContext_24)->instances_0 = instances_2;

#line 1929
    (&kernelContext_24)->blades_1 = blades_3;

#line 1929
    (&kernelContext_24)->frame_0 = frame_2;

#line 1929
    (&kernelContext_24)->field_0 = field_2;

#line 1929
    (&kernelContext_24)->cluster_lights_0 = cluster_lights_2;

#line 1929
    (&kernelContext_24)->lights_0 = lights_2;

#line 1929
    (&kernelContext_24)->shadow_atlas_0 = shadow_atlas_2;

#line 1929
    (&kernelContext_24)->shadow_sampler_0 = shadow_sampler_2;

#line 1929
    (&kernelContext_24)->grassCard_0 = grassCard_2;

#line 1929
    (&kernelContext_24)->grassCardSampler_0 = grassCardSampler_2;

#line 1929
    (&kernelContext_24)->grassGround_0 = grassGround_2;

#line 1929
    (&kernelContext_24)->wind_0 = wind_2;

#line 1929
    (&kernelContext_24)->windDirectionLayer_0 = windDirectionLayer_2;

#line 1929
    (&kernelContext_24)->windSampler_0 = windSampler_2;

#line 1929
    (&kernelContext_24)->windIntensityLayer_0 = windIntensityLayer_2;

#line 1929
    (&kernelContext_24)->grassCells_0 = grassCells_2;

#line 1935
    float _S151 = _S150.uv_2.y;

#line 1935
    float3 _S152 = float3(_S150.uv_2.x, 1.0f - _S151, 0.0f);

    if((((grassCard_2).sample((grassCardSampler_2), ((_S152)).xy, uint(((_S152)).z), level((_S150.level_3)))).x) < 0.5f)
    {
        discard_fragment();

#line 1937
    }

#line 1937
    float3 _S153 = grass_style_1(_S150.color_4, _S150.row_5, _S151, _S150.patch_4, &kernelContext_24);

#line 1937
    float3 _S154 = grass_light_0(_S150.world_position_8, normalize(_S150.normal_5), position_4.xy, &kernelContext_24);

#line 1937
    pixelOutput_1 _S155 = { float4(_S153 * _S154, 1.0f) };

#line 1945
    return _S155;
}


#line 2000
uint grass_shell_count_0(KernelContext_0 thread* kernelContext_25)
{
    return clamp(uint(kernelContext_25->field_0->stack_0.y + 0.5f), 1U, 64U);
}


#line 1507
float grass_ground_texel_0(int2 texel_1, KernelContext_0 thread* kernelContext_26)
{

    int3 _S156 = int3(clamp(texel_1, int2(int(0), int(0)), int2(int(kernelContext_26->field_0->maps_0.x) - int(1), int(kernelContext_26->field_0->maps_0.y) - int(1))), int(0));

#line 1510
    return ((kernelContext_26->grassGround_0).read(vec<uint,2>(((_S156)).xy), uint(((_S156)).z)).x);
}


#line 1498
struct GrassGround_0
{
    float height_1;
    float3 normal_6;
};


#line 1515
GrassGround_0 grass_ground_under_0(float2 world_0, KernelContext_0 thread* kernelContext_27)
{
    float2 at_0 = (world_0 - kernelContext_27->field_0->ground_1.xy) * float2(kernelContext_27->field_0->ground_1.w) ;
    float2 base_3 = floor(at_0);
    float2 blend_1 = at_0 - base_3;
    int2 _S157 = int2(base_3);

#line 1520
    float _S158 = grass_ground_texel_0(_S157, kernelContext_27);

#line 1520
    float _S159 = grass_ground_texel_0(_S157 + int2(int(1), int(0)), kernelContext_27);

#line 1520
    float _S160 = grass_ground_texel_0(_S157 + int2(int(0), int(1)), kernelContext_27);

#line 1520
    float _S161 = grass_ground_texel_0(_S157 + int2(int(1), int(1)), kernelContext_27);

#line 1527
    float _S162 = blend_1.x;

#line 1527
    float _S163 = _S159 - _S158;

#line 1527
    float lower_2 = _S158 + _S162 * _S163;
    float _S164 = _S161 - _S160;

#line 1526
    thread GrassGround_0 under_0;


    (&under_0)->height_1 = lower_2 + blend_1.y * (_S160 + _S162 * _S164 - lower_2);


    (&under_0)->normal_6 = normalize(float3(- (0.5f * (_S163 + _S164)), kernelContext_27->field_0->ground_1.z, - (0.5f * (_S160 - _S158 + (_S161 - _S159)))));
    return under_0;
}


#line 1437
float windSmoothTriangle_0(float u_1)
{
    float s_0 = abs(fract(u_1 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}


float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_28)
{


    float2 _S165 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_28->windDirectionLayer_0).sample((kernelContext_28->windSampler_0), (kernelContext_28->wind_0->directionUv_0 + _S165 * kernelContext_28->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_28->windIntensityLayer_0).sample((kernelContext_28->windSampler_0), (kernelContext_28->wind_0->intensityUv_0 + _S165 * kernelContext_28->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;

    float2 base_4 = kernelContext_28->wind_0->baseDirection_0;
    float _S166 = kernelContext_28->wind_0->baseDirection_0.x;

#line 1458
    float _S167 = deflection_0.x;

#line 1458
    float _S168 = kernelContext_28->wind_0->baseDirection_0.y;

#line 1458
    float _S169 = deflection_0.y;

#line 1458
    float2 turned_0 = float2(_S166 * _S167 - _S168 * _S169, _S166 * _S169 + _S168 * _S167);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 1460
    float2 direction_2;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 1462
        direction_2 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 1462
    }
    else
    {

#line 1462
        direction_2 = base_4;

#line 1462
    }

#line 1467
    float speed_0 = intensity_0 * kernelContext_28->wind_0->baseSpeed_0 * (1.0f + kernelContext_28->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S165, base_4) * kernelContext_28->wind_0->invGustWavelength_0 + kernelContext_28->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_2.x * speed_0, 0.0f, direction_2.y * speed_0);
}



float3 grass_lean_0(float3 velocity_0, float height_2)
{
    float speed_1 = length(velocity_0);
    float bend_0 = height_2 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_2 = velocity_0.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;
    float _S170 = bend_0 * bend_0;
    return float3(along_2.x, - (_S170 / (height_2 + sqrt(max(height_2 * height_2 - _S170, 0.0f)))), along_2.y);
}


#line 1949
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


#line 1973
GrassShellVertex_0 grass_sheet_vertex_0(float2 rest_1, float share_0, float4 fin_1, float occlusion_2, KernelContext_0 thread* kernelContext_29)
{

#line 1973
    GrassGround_0 _S171 = grass_ground_under_0(rest_1, kernelContext_29);


    float lift_0 = share_0 * kernelContext_29->field_0->stack_0.x;
    float _S172 = rest_1.x;

#line 1977
    float _S173 = rest_1.y;

#line 1977
    float3 ground_2 = float3(_S172, _S171.height_1, _S173);

#line 1977
    float3 _S174 = windSample_0(ground_2 - kernelContext_29->frame_0->camera_position_0.xyz, kernelContext_29);

#line 1984
    float3 world_1 = ground_2 + float3(0.0f, lift_0, 0.0f) + grass_lean_0(_S174, kernelContext_29->field_0->stack_0.x) * float3((share_0 * share_0)) ;

    thread GrassShellVertex_0 output_3;
    float4 _S175 = (((float4(world_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(3)]))));

#line 1987
    (&output_3)->position_5 = _S175;
    (&output_3)->world_position_9 = world_1;
    (&output_3)->rest_0 = float3(_S172, lift_0, _S173);
    (&output_3)->normal_7 = _S171.normal_6;


    (&output_3)->footprint_0 = max(abs(_S175.w), 0.00009999999747379f) / max(kernelContext_29->grass_0->screen_0.x, 0.00009999999747379f);
    (&output_3)->occlusion_1 = occlusion_2;
    (&output_3)->fin_0 = fin_1;
    return output_3;
}


#line 2022
float grass_occlusion_at_0(float share_1, KernelContext_0 thread* kernelContext_30)
{

#line 2022
    uint _S176 = grass_shell_count_0(kernelContext_30);

#line 2022
    float occlusion_3 = kernelContext_30->field_0->layers_0[int(0)].y;

#line 2022
    uint shell_0 = 0U;



    for(;;)
    {

#line 2026
        if(shell_0 < _S176)
        {
        }
        else
        {

#line 2026
            break;
        }
        if((kernelContext_30->field_0->layers_0[shell_0].x) <= share_1)
        {

#line 2028
            occlusion_3 = kernelContext_30->field_0->layers_0[shell_0].y;

#line 2028
        }

#line 2026
        shell_0 = shell_0 + 1U;

#line 2026
    }

#line 2033
    return occlusion_3;
}


#line 541
struct GrassInstance_0
{
    float4 root_0;
    float4 facing_0;
    float4 lean_0;
    float4 ground_0;
    float4 clump_1;
    uint4 lanes_0;
};


#line 2100
GrassInstance_0 grass_no_blade_0()
{
    thread GrassInstance_0 none_0;
    float4 _S177 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 2103
    (&none_0)->root_0 = _S177;
    (&none_0)->facing_0 = _S177;
    (&none_0)->lean_0 = _S177;
    (&none_0)->ground_0 = _S177;
    (&none_0)->lanes_0 = uint4(0U, 0U, 0U, 0U);
    return none_0;
}



GrassInstance_0 grass_cell_0(int2 global_0, KernelContext_0 thread* kernelContext_31)
{
    int side_1 = int(max(kernelContext_31->field_0->tiles_0.z, 1U));
    int2 extent_0 = int2(int(kernelContext_31->field_0->tiles_0.x), int(kernelContext_31->field_0->tiles_0.y)) * int2(side_1) ;
    int _S178 = global_0.x;

#line 2117
    bool _S179;

#line 2117
    if(_S178 < int(0))
    {

#line 2117
        _S179 = true;

#line 2117
    }
    else
    {

#line 2117
        _S179 = (global_0.y) < int(0);

#line 2117
    }

#line 2117
    if(_S179)
    {

#line 2117
        _S179 = true;

#line 2117
    }
    else
    {

#line 2117
        _S179 = _S178 >= (extent_0.x);

#line 2117
    }

#line 2117
    if(_S179)
    {

#line 2117
        _S179 = true;

#line 2117
    }
    else
    {

#line 2117
        _S179 = (global_0.y) >= (extent_0.y);

#line 2117
    }

#line 2117
    if(_S179)
    {
        return grass_no_blade_0();
    }
    uint2 _S180 = uint2(global_0);
    uint cells_0 = uint(side_1);
    uint _S181 = _S180.y;

#line 2123
    uint _S182 = _S181 / cells_0;

#line 2123
    uint _S183 = _S182 * kernelContext_31->field_0->tiles_0.x;

#line 2123
    uint _S184 = _S180.x;

#line 2123
    uint _S185 = _S184 / cells_0;

#line 2123
    uint slot_2 = _S183 + _S185;
    uint _S186 = _S181 % cells_0;

#line 2124
    uint _S187 = _S186 * cells_0;

#line 2124
    uint _S188 = _S184 % cells_0;
    GrassInstance_natural_0 _S189 = kernelContext_31->grassCells_0[slot_2 * kernelContext_31->field_0->tiles_0.w + (_S187 + _S188)];

#line 2125
    GrassInstance_0 _S190 = { float4(_S189.root_0) , float4(_S189.facing_0) , float4(_S189.lean_0) , float4(_S189.ground_0) , float4(_S189.clump_1) , uint4(_S189.lanes_0)  };

#line 2125
    return _S190;
}




float grass_strand_reach_0(const GrassInstance_0 thread* blade_3, float lift_1, float floor_reach_0, float fade_1, KernelContext_0 thread* kernelContext_32)
{
    float _S191 = blade_3->root_0.w;

#line 2133
    bool _S192;

#line 2133
    if(!(lift_1 < _S191))
    {

#line 2133
        _S192 = true;

#line 2133
    }
    else
    {

#line 2133
        GrassBlade_0 _S193 = grass_row_0(blade_3->lanes_0.y, kernelContext_32);

#line 2133
        _S192 = (_S193.flags_0.x) != 1U;

#line 2133
    }

#line 2133
    if(_S192)
    {
        return -1.0f;
    }
    float bound_0 = 0.5f * kernelContext_32->field_0->origin_0.w;

    return min(max(min(blade_3->facing_0.z, bound_0) * (1.0f - lift_1 / _S191), floor_reach_0), bound_0) * fade_1;
}


#line 2139
struct pixelOutput_2
{
    float4 output_4 [[color(0)]];
};


#line 2139
struct pixelInput_2
{
    float3 world_position_10 [[user(WORLD_POSITION)]];
    float3 rest_2 [[user(REST_POSITION)]];
    float3 normal_8 [[user(NORMAL)]];
    float footprint_1 [[user(FOOTPRINT)]];
    float occlusion_4 [[user(OCCLUSION)]];
    [[flat]] float4 fin_2 [[user(FIN)]];
};


#line 2144
[[fragment]] pixelOutput_2 shellFragmentMain(pixelInput_2 _S194 [[stage_in]], float4 position_6 [[position]], GrassTile_0 constant* tile_11 [[buffer(2)]], GrassParams_0 constant* grass_3 [[buffer(1)]], GrassInstance_natural_0 device* instances_3 [[buffer(3)]], GrassBlade_natural_0 device* blades_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], GrassField_0 constant* field_3 [[buffer(7)]], uint device* cluster_lights_3 [[buffer(6)]], GpuLight_natural_0 device* lights_3 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_3 [[texture(0)]], sampler grassCardSampler_3 [[sampler(0)]], texture2d<float, access::sample> grassGround_3 [[texture(2)]], WindParams_0 constant* wind_3 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_3 [[texture(3)]], sampler windSampler_3 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_3 [[texture(4)]], GrassInstance_natural_0 device* grassCells_3 [[buffer(8)]])
{

#line 2144
    thread KernelContext_0 kernelContext_33;

#line 2144
    (&kernelContext_33)->tile_1 = tile_11;

#line 2144
    (&kernelContext_33)->grass_0 = grass_3;

#line 2144
    (&kernelContext_33)->instances_0 = instances_3;

#line 2144
    (&kernelContext_33)->blades_1 = blades_4;

#line 2144
    (&kernelContext_33)->frame_0 = frame_3;

#line 2144
    (&kernelContext_33)->field_0 = field_3;

#line 2144
    (&kernelContext_33)->cluster_lights_0 = cluster_lights_3;

#line 2144
    (&kernelContext_33)->lights_0 = lights_3;

#line 2144
    (&kernelContext_33)->shadow_atlas_0 = shadow_atlas_3;

#line 2144
    (&kernelContext_33)->shadow_sampler_0 = shadow_sampler_3;

#line 2144
    (&kernelContext_33)->grassCard_0 = grassCard_3;

#line 2144
    (&kernelContext_33)->grassCardSampler_0 = grassCardSampler_3;

#line 2144
    (&kernelContext_33)->grassGround_0 = grassGround_3;

#line 2144
    (&kernelContext_33)->wind_0 = wind_3;

#line 2144
    (&kernelContext_33)->windDirectionLayer_0 = windDirectionLayer_3;

#line 2144
    (&kernelContext_33)->windSampler_0 = windSampler_3;

#line 2144
    (&kernelContext_33)->windIntensityLayer_0 = windIntensityLayer_3;

#line 2144
    (&kernelContext_33)->grassCells_0 = grassCells_3;

    float cell_1 = field_3->origin_0.w;
    float2 _S195 = _S194.rest_2.xz;

#line 2147
    float2 from_corner_0 = (_S195 - field_3->origin_0.xy) / float2(cell_1) ;
    float2 _S196 = floor(from_corner_0);

#line 2148
    int2 _S197 = int2(_S196);
    float2 inside_0 = from_corner_0 - _S196;

#line 2149
    int _S198;
    if((inside_0.x) < 0.5f)
    {

#line 2150
        _S198 = int(-1);

#line 2150
    }
    else
    {

#line 2150
        _S198 = int(1);

#line 2150
    }

#line 2150
    int band_2;

#line 2150
    if((inside_0.y) < 0.5f)
    {

#line 2150
        band_2 = int(-1);

#line 2150
    }
    else
    {

#line 2150
        band_2 = int(1);

#line 2150
    }
    float lift_2 = _S194.rest_2.y;


    float _S199 = 0.5f * _S194.footprint_1;
    float _S200 = _S194.fin_2.y;

    GrassInstance_0 _S201 = grass_no_blade_0();

    float _S202 = _S194.fin_2.x;

#line 2159
    int dz_0;

#line 2159
    int dx_0;

#line 2159
    float nearest_0;

#line 2159
    float nearest_1;

#line 2159
    GrassInstance_0 found_15;

#line 2159
    GrassInstance_0 found_16;

#line 2159
    bool _S203;

#line 2159
    if(_S202 < 0.5f)
    {

#line 2159
        nearest_1 = 2.0f;

#line 2159
        found_16 = _S201;

#line 2159
        dz_0 = int(0);


        for(;;)
        {

#line 2162
            if(dz_0 < int(2))
            {
            }
            else
            {

#line 2162
                break;
            }

#line 2162
            nearest_0 = nearest_1;

#line 2162
            found_15 = found_16;

#line 2162
            dx_0 = int(0);

            for(;;)
            {

#line 2164
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2164
                    break;
                }

#line 2164
                GrassInstance_0 _S204 = grass_cell_0(_S197 + int2(dx_0 * _S198, dz_0 * band_2), &kernelContext_33);

#line 2164
                thread GrassInstance_0 _S205 = _S204;

#line 2164
                float _S206 = grass_strand_reach_0(&_S205, lift_2, _S199, _S200, &kernelContext_33);



                float2 offset_1 = _S195 - _S204.root_0.xz;
                float apart_0 = sqrt(dot(offset_1, offset_1)) / max(_S206, 9.99999997475242708e-07f);
                if(_S206 > 0.0f)
                {

#line 2170
                    _S203 = apart_0 < nearest_0;

#line 2170
                }
                else
                {

#line 2170
                    _S203 = false;

#line 2170
                }

#line 2170
                if(_S203)
                {

#line 2170
                    nearest_0 = apart_0;

#line 2170
                    found_15 = _S204;

#line 2170
                }

#line 2164
                dx_0 = dx_0 + int(1);

#line 2164
            }

#line 2162
            int dz_1 = dz_0 + int(1);

#line 2162
            nearest_1 = nearest_0;

#line 2162
            found_16 = found_15;

#line 2162
            dz_0 = dz_1;

#line 2162
        }

#line 2162
        nearest_0 = nearest_1;

#line 2162
        found_15 = found_16;

#line 2159
    }
    else
    {

#line 2182
        bool along_z_0 = _S202 < 1.5f;
        float _S207 = _S194.fin_2.z;

#line 2183
        if(along_z_0)
        {

#line 2183
            nearest_0 = field_3->origin_0.x;

#line 2183
        }
        else
        {

#line 2183
            nearest_0 = field_3->origin_0.y;

#line 2183
        }
        int _S208 = int(floor((_S207 - nearest_0) / cell_1 + 0.5f)) - int(2);
        if(along_z_0)
        {

#line 2185
            dz_0 = _S197.y;

#line 2185
        }
        else
        {

#line 2185
            dz_0 = _S197.x;

#line 2185
        }
        if(along_z_0)
        {

#line 2186
            _S198 = band_2;

#line 2186
        }

#line 2186
        nearest_0 = 2.0f;

#line 2186
        found_15 = _S201;

#line 2186
        band_2 = int(0);
        for(;;)
        {

#line 2187
            if(band_2 < int(4))
            {
            }
            else
            {

#line 2187
                break;
            }

#line 2187
            nearest_1 = nearest_0;

#line 2187
            found_16 = found_15;

#line 2187
            dx_0 = int(0);

            for(;;)
            {

#line 2189
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2189
                    break;
                }
                int along_3 = dz_0 + dx_0 * _S198;

#line 2191
                int2 _S209;
                if(along_z_0)
                {

#line 2192
                    _S209 = int2(_S208 + band_2, along_3);

#line 2192
                }
                else
                {

#line 2192
                    _S209 = int2(along_3, _S208 + band_2);

#line 2192
                }

#line 2192
                GrassInstance_0 _S210 = grass_cell_0(_S209, &kernelContext_33);

#line 2192
                thread GrassInstance_0 _S211 = _S210;

#line 2192
                float _S212 = grass_strand_reach_0(&_S211, lift_2, _S199, _S200, &kernelContext_33);

#line 2192
                float offset_2;



                if(along_z_0)
                {

#line 2196
                    offset_2 = _S194.rest_2.z - _S210.root_0.z;

#line 2196
                }
                else
                {

#line 2196
                    offset_2 = _S194.rest_2.x - _S210.root_0.x;

#line 2196
                }
                float apart_1 = abs(offset_2) / max(_S212, 9.99999997475242708e-07f);
                if(_S212 > 0.0f)
                {

#line 2198
                    _S203 = apart_1 < nearest_1;

#line 2198
                }
                else
                {

#line 2198
                    _S203 = false;

#line 2198
                }

#line 2198
                if(_S203)
                {

#line 2198
                    nearest_1 = apart_1;

#line 2198
                    found_16 = _S210;

#line 2198
                }

#line 2189
                dx_0 = dx_0 + int(1);

#line 2189
            }

#line 2187
            int band_3 = band_2 + int(1);

#line 2187
            nearest_0 = nearest_1;

#line 2187
            found_15 = found_16;

#line 2187
            band_2 = band_3;

#line 2187
        }

#line 2159
    }

#line 2206
    if(!(nearest_0 < 1.0f))
    {
        discard_fragment();

#line 2206
    }

#line 2206
    GrassBlade_0 _S213 = grass_row_0(found_15.lanes_0.y, &kernelContext_33);

#line 2212
    float along_blade_0 = saturate(lift_2 / found_15.root_0.w);

    float3 color_5 = mix(_S213.root_color_0.xyz, _S213.tip_color_0.xyz, float3(along_blade_0) ) * float3((1.0f - 0.18000000715255737f * found_15.facing_0.w)) ;
    float _S214 = grass_patch_0(found_15.lanes_0.z);

#line 2215
    thread GrassBlade_0 _S215 = _S213;

#line 2215
    float3 _S216 = grass_style_0(color_5, &_S215, along_blade_0, _S214);

#line 2215
    float3 color_6 = _S216 * float3(_S194.occlusion_4) ;

#line 2215
    float3 normal_9;

    if((_S213.flags_0.y) == 1U)
    {

#line 2217
        normal_9 = float3(0.0f, 1.0f, 0.0f);

#line 2217
    }
    else
    {

#line 2217
        normal_9 = normalize(_S194.normal_8);

#line 2217
    }

#line 2217
    float3 _S217 = grass_light_0(_S194.world_position_10, normal_9, position_6.xy, &kernelContext_33);

#line 2217
    pixelOutput_2 _S218 = { float4(color_6 * _S217, 1.0f) };
    return _S218;
}


#line 2218
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


#line 2218
[[vertex]] vertexMain_Result_0 vertexMain(uint index_4 [[vertex_id]], uint instance_id_0 [[instance_id]], GrassTile_0 constant* tile_12 [[buffer(2)]], GrassParams_0 constant* grass_4 [[buffer(1)]], GrassInstance_natural_0 device* instances_4 [[buffer(3)]], GrassBlade_natural_0 device* blades_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], GrassField_0 constant* field_4 [[buffer(7)]], uint device* cluster_lights_4 [[buffer(6)]], GpuLight_natural_0 device* lights_4 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_4 [[texture(0)]], sampler grassCardSampler_4 [[sampler(0)]], texture2d<float, access::sample> grassGround_4 [[texture(2)]], WindParams_0 constant* wind_4 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_4 [[texture(3)]], sampler windSampler_4 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_4 [[texture(4)]], GrassInstance_natural_0 device* grassCells_4 [[buffer(8)]])
{

#line 2218
    thread KernelContext_0 kernelContext_34;

#line 2218
    (&kernelContext_34)->tile_1 = tile_12;

#line 2218
    (&kernelContext_34)->grass_0 = grass_4;

#line 2218
    (&kernelContext_34)->instances_0 = instances_4;

#line 2218
    (&kernelContext_34)->blades_1 = blades_5;

#line 2218
    (&kernelContext_34)->frame_0 = frame_4;

#line 2218
    (&kernelContext_34)->field_0 = field_4;

#line 2218
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_4;

#line 2218
    (&kernelContext_34)->lights_0 = lights_4;

#line 2218
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_4;

#line 2218
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_4;

#line 2218
    (&kernelContext_34)->grassCard_0 = grassCard_4;

#line 2218
    (&kernelContext_34)->grassCardSampler_0 = grassCardSampler_4;

#line 2218
    (&kernelContext_34)->grassGround_0 = grassGround_4;

#line 2218
    (&kernelContext_34)->wind_0 = wind_4;

#line 2218
    (&kernelContext_34)->windDirectionLayer_0 = windDirectionLayer_4;

#line 2218
    (&kernelContext_34)->windSampler_0 = windSampler_4;

#line 2218
    (&kernelContext_34)->windIntensityLayer_0 = windIntensityLayer_4;

#line 2218
    (&kernelContext_34)->grassCells_0 = grassCells_4;

#line 1854
    GrassInstance_natural_0 blade_4 = instances_4[tile_12->slot_0.x * grass_4->limits_0.x + instance_id_0];

#line 1854
    uint4 _S219 = uint4(blade_4.lanes_0) ;
    uint _S220 = _S219.y;

#line 1855
    GrassBlade_0 _S221 = grass_row_0(_S220, &kernelContext_34);


    uint _S222 = index_4 % 6U;

#line 1858
    float4 _S223 = float4(blade_4.facing_0) ;

#line 1863
    float2 facing_1 = _S223.xy;

#line 1863
    float2 across_0;
    if((index_4 / 6U) == 0U)
    {

#line 1864
        across_0 = facing_1;

#line 1864
    }
    else
    {

#line 1864
        across_0 = float2(- facing_1.y, facing_1.x);

#line 1864
    }

    float _S224 = _S223.z;

#line 1871
    float _S225 = GRASS_CARD_CORNERS_0[_S222].y;

#line 1871
    float4 _S226 = float4(blade_4.root_0) ;
    float3 world_2 = _S226.xyz + float3(across_0.x, 0.0f, across_0.y) * float3((_S224 * (GRASS_CARD_CORNERS_0[_S222].x * 2.0f - 1.0f)))  + float3(0.0f, _S226.w * _S225, 0.0f) + (float4(blade_4.lean_0) ).xyz * float3((_S225 * _S225)) ;

    thread GrassVertex_0 output_5;
    (&output_5)->position_2 = (((float4(world_2, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_5)->world_position_0 = world_2;

#line 1876
    float3 _S227;
    if((_S221.flags_0.y) == 1U)
    {

#line 1877
        _S227 = float3(0.0f, 1.0f, 0.0f);

#line 1877
    }
    else
    {

#line 1877
        _S227 = (float4(blade_4.ground_0) ).xyz;

#line 1877
    }

#line 1877
    (&output_5)->normal_2 = _S227;

#line 1882
    (&output_5)->color_1 = mix(_S221.root_color_0.xyz, _S221.tip_color_0.xyz, float3(_S225) ) * float3((1.0f - 0.18000000715255737f * _S223.w)) ;
    (&output_5)->uv_0 = GRASS_CARD_CORNERS_0[_S222];

#line 1888
    (&output_5)->level_1 = grassCardLevel_0(2.0f * _S224 * (&kernelContext_34)->grass_0->screen_0.x / max(abs((&output_5)->position_2.w), 0.00009999999747379f));

    (&output_5)->row_2 = min(_S220, max(grass_4->limits_0.y, 1U) - 1U);
    (&output_5)->patch_1 = grass_patch_0(_S219.z);
    GrassVertex_0 _S228 = output_5;

#line 1892
    thread vertexMain_Result_0 _S229;

#line 1892
    (&_S229)->position_7 = _S228.position_2;

#line 1892
    (&_S229)->world_position_11 = _S228.world_position_0;

#line 1892
    (&_S229)->normal_10 = _S228.normal_2;

#line 1892
    (&_S229)->color_7 = _S228.color_1;

#line 1892
    (&_S229)->uv_3 = _S228.uv_0;

#line 1892
    (&_S229)->level_4 = _S228.level_1;

#line 1892
    (&_S229)->row_6 = _S228.row_2;

#line 1892
    (&_S229)->patch_5 = _S228.patch_1;

#line 1892
    return _S229;
}


#line 1892
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


#line 1892
[[vertex]] bladeNearVertexMain_Result_0 bladeNearVertexMain(uint index_5 [[vertex_id]], uint instance_id_1 [[instance_id]], GrassTile_0 constant* tile_13 [[buffer(2)]], GrassParams_0 constant* grass_5 [[buffer(1)]], GrassInstance_natural_0 device* instances_5 [[buffer(3)]], GrassBlade_natural_0 device* blades_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], GrassField_0 constant* field_5 [[buffer(7)]], uint device* cluster_lights_5 [[buffer(6)]], GpuLight_natural_0 device* lights_5 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_5 [[texture(0)]], sampler grassCardSampler_5 [[sampler(0)]], texture2d<float, access::sample> grassGround_5 [[texture(2)]], WindParams_0 constant* wind_5 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_5 [[texture(3)]], sampler windSampler_5 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_5 [[texture(4)]], GrassInstance_natural_0 device* grassCells_5 [[buffer(8)]])
{

#line 1892
    thread KernelContext_0 kernelContext_35;

#line 1892
    (&kernelContext_35)->tile_1 = tile_13;

#line 1892
    (&kernelContext_35)->grass_0 = grass_5;

#line 1892
    (&kernelContext_35)->instances_0 = instances_5;

#line 1892
    (&kernelContext_35)->blades_1 = blades_6;

#line 1892
    (&kernelContext_35)->frame_0 = frame_5;

#line 1892
    (&kernelContext_35)->field_0 = field_5;

#line 1892
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_5;

#line 1892
    (&kernelContext_35)->lights_0 = lights_5;

#line 1892
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_5;

#line 1892
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_5;

#line 1892
    (&kernelContext_35)->grassCard_0 = grassCard_5;

#line 1892
    (&kernelContext_35)->grassCardSampler_0 = grassCardSampler_5;

#line 1892
    (&kernelContext_35)->grassGround_0 = grassGround_5;

#line 1892
    (&kernelContext_35)->wind_0 = wind_5;

#line 1892
    (&kernelContext_35)->windDirectionLayer_0 = windDirectionLayer_5;

#line 1892
    (&kernelContext_35)->windSampler_0 = windSampler_5;

#line 1892
    (&kernelContext_35)->windIntensityLayer_0 = windIntensityLayer_5;

#line 1892
    (&kernelContext_35)->grassCells_0 = grassCells_5;

#line 1902
    GrassInstance_natural_0 blade_5 = instances_5[(field_5->tiles_0.x * field_5->tiles_0.y + tile_13->slot_0.x) * grass_5->limits_0.x + instance_id_1];

#line 1902
    thread GrassInstance_natural_0 _S230 = blade_5;

#line 1902
    float2 _S231 = grass_blade_lod_0(length((float4((&_S230)->root_0) ).xyz - frame_5->camera_position_0.xyz), &kernelContext_35);

#line 1902
    _S230 = blade_5;

#line 1902
    GrassVertex_0 _S232 = grass_blade_vertex_0(&_S230, index_5, 7U, _S231, &kernelContext_35);

#line 1902
    thread bladeNearVertexMain_Result_0 _S233;

#line 1902
    (&_S233)->position_8 = _S232.position_2;

#line 1902
    (&_S233)->world_position_12 = _S232.world_position_0;

#line 1902
    (&_S233)->normal_11 = _S232.normal_2;

#line 1902
    (&_S233)->color_8 = _S232.color_1;

#line 1902
    (&_S233)->uv_4 = _S232.uv_0;

#line 1902
    (&_S233)->level_5 = _S232.level_1;

#line 1902
    (&_S233)->row_7 = _S232.row_2;

#line 1902
    (&_S233)->patch_6 = _S232.patch_1;

#line 1902
    return _S233;
}


#line 1902
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


#line 1902
[[vertex]] bladeFarVertexMain_Result_0 bladeFarVertexMain(uint index_6 [[vertex_id]], uint instance_id_2 [[instance_id]], GrassTile_0 constant* tile_14 [[buffer(2)]], GrassParams_0 constant* grass_6 [[buffer(1)]], GrassInstance_natural_0 device* instances_6 [[buffer(3)]], GrassBlade_natural_0 device* blades_7 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_6 [[buffer(0)]], GrassField_0 constant* field_6 [[buffer(7)]], uint device* cluster_lights_6 [[buffer(6)]], GpuLight_natural_0 device* lights_6 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_6 [[texture(0)]], sampler grassCardSampler_6 [[sampler(0)]], texture2d<float, access::sample> grassGround_6 [[texture(2)]], WindParams_0 constant* wind_6 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_6 [[texture(3)]], sampler windSampler_6 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_6 [[texture(4)]], GrassInstance_natural_0 device* grassCells_6 [[buffer(8)]])
{

#line 1902
    thread KernelContext_0 kernelContext_36;

#line 1902
    (&kernelContext_36)->tile_1 = tile_14;

#line 1902
    (&kernelContext_36)->grass_0 = grass_6;

#line 1902
    (&kernelContext_36)->instances_0 = instances_6;

#line 1902
    (&kernelContext_36)->blades_1 = blades_7;

#line 1902
    (&kernelContext_36)->frame_0 = frame_6;

#line 1902
    (&kernelContext_36)->field_0 = field_6;

#line 1902
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_6;

#line 1902
    (&kernelContext_36)->lights_0 = lights_6;

#line 1902
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_6;

#line 1902
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_6;

#line 1902
    (&kernelContext_36)->grassCard_0 = grassCard_6;

#line 1902
    (&kernelContext_36)->grassCardSampler_0 = grassCardSampler_6;

#line 1902
    (&kernelContext_36)->grassGround_0 = grassGround_6;

#line 1902
    (&kernelContext_36)->wind_0 = wind_6;

#line 1902
    (&kernelContext_36)->windDirectionLayer_0 = windDirectionLayer_6;

#line 1902
    (&kernelContext_36)->windSampler_0 = windSampler_6;

#line 1902
    (&kernelContext_36)->windIntensityLayer_0 = windIntensityLayer_6;

#line 1902
    (&kernelContext_36)->grassCells_0 = grassCells_6;

#line 1913
    uint capacity_0 = grass_6->limits_0.x;

    float2 _S234 = float2(1.0f, 1.0f);

#line 1915
    thread GrassInstance_natural_0 _S235 = instances_6[tile_14->slot_0.x * capacity_0 + capacity_0 - 1U - instance_id_2];

#line 1915
    GrassVertex_0 _S236 = grass_blade_vertex_0(&_S235, index_6, 3U, _S234, &kernelContext_36);

#line 1915
    thread bladeFarVertexMain_Result_0 _S237;

#line 1915
    (&_S237)->position_9 = _S236.position_2;

#line 1915
    (&_S237)->world_position_13 = _S236.world_position_0;

#line 1915
    (&_S237)->normal_12 = _S236.normal_2;

#line 1915
    (&_S237)->color_9 = _S236.color_1;

#line 1915
    (&_S237)->uv_5 = _S236.uv_0;

#line 1915
    (&_S237)->level_6 = _S236.level_1;

#line 1915
    (&_S237)->row_8 = _S236.row_2;

#line 1915
    (&_S237)->patch_7 = _S236.patch_1;

#line 1915
    return _S237;
}


#line 1915
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


#line 1915
[[vertex]] shellVertexMain_Result_0 shellVertexMain(uint index_7 [[vertex_id]], uint instance_id_3 [[instance_id]], GrassTile_0 constant* tile_15 [[buffer(2)]], GrassParams_0 constant* grass_7 [[buffer(1)]], GrassInstance_natural_0 device* instances_7 [[buffer(3)]], GrassBlade_natural_0 device* blades_8 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], GrassField_0 constant* field_7 [[buffer(7)]], uint device* cluster_lights_7 [[buffer(6)]], GpuLight_natural_0 device* lights_7 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_7 [[texture(1)]], sampler shadow_sampler_7 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_7 [[texture(0)]], sampler grassCardSampler_7 [[sampler(0)]], texture2d<float, access::sample> grassGround_7 [[texture(2)]], WindParams_0 constant* wind_7 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_7 [[texture(3)]], sampler windSampler_7 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_7 [[texture(4)]], GrassInstance_natural_0 device* grassCells_7 [[buffer(8)]])
{

#line 1915
    thread KernelContext_0 kernelContext_37;

#line 1915
    (&kernelContext_37)->tile_1 = tile_15;

#line 1915
    (&kernelContext_37)->grass_0 = grass_7;

#line 1915
    (&kernelContext_37)->instances_0 = instances_7;

#line 1915
    (&kernelContext_37)->blades_1 = blades_8;

#line 1915
    (&kernelContext_37)->frame_0 = frame_7;

#line 1915
    (&kernelContext_37)->field_0 = field_7;

#line 1915
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_7;

#line 1915
    (&kernelContext_37)->lights_0 = lights_7;

#line 1915
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_7;

#line 1915
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_7;

#line 1915
    (&kernelContext_37)->grassCard_0 = grassCard_7;

#line 1915
    (&kernelContext_37)->grassCardSampler_0 = grassCardSampler_7;

#line 1915
    (&kernelContext_37)->grassGround_0 = grassGround_7;

#line 1915
    (&kernelContext_37)->wind_0 = wind_7;

#line 1915
    (&kernelContext_37)->windDirectionLayer_0 = windDirectionLayer_7;

#line 1915
    (&kernelContext_37)->windSampler_0 = windSampler_7;

#line 1915
    (&kernelContext_37)->windIntensityLayer_0 = windIntensityLayer_7;

#line 1915
    (&kernelContext_37)->grassCells_0 = grassCells_7;

#line 1915
    uint _S238 = grass_shell_count_0(&kernelContext_37);

#line 2011
    uint _S239 = _S238 - 1U;
    uint quad_0 = index_7 / 6U;

#line 2012
    GrassShellVertex_0 _S240 = grass_sheet_vertex_0((&kernelContext_37)->tile_1->tile_0.xy + (float2(float(quad_0 % 16U), float(quad_0 / 16U)) + GRASS_CARD_CORNERS_0[index_7 % 6U]) * float2(((&kernelContext_37)->tile_1->tile_0.z / 16.0f)) , (&kernelContext_37)->field_0->layers_0[_S239 - min(instance_id_3, _S239)].x, float4(0.0f, 1.0f, 0.0f, 0.0f), (&kernelContext_37)->field_0->layers_0[_S239 - min(instance_id_3, _S239)].y, &kernelContext_37);

#line 2012
    thread shellVertexMain_Result_0 _S241;

#line 2012
    (&_S241)->position_10 = _S240.position_5;

#line 2012
    (&_S241)->world_position_14 = _S240.world_position_9;

#line 2012
    (&_S241)->rest_3 = _S240.rest_0;

#line 2012
    (&_S241)->normal_13 = _S240.normal_7;

#line 2012
    (&_S241)->footprint_2 = _S240.footprint_0;

#line 2012
    (&_S241)->occlusion_5 = _S240.occlusion_1;

#line 2012
    (&_S241)->fin_3 = _S240.fin_0;

#line 2012
    return _S241;
}


#line 2012
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


#line 2012
[[vertex]] finVertexMain_Result_0 finVertexMain(uint index_8 [[vertex_id]], GrassTile_0 constant* tile_16 [[buffer(2)]], GrassParams_0 constant* grass_8 [[buffer(1)]], GrassInstance_natural_0 device* instances_8 [[buffer(3)]], GrassBlade_natural_0 device* blades_9 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_8 [[buffer(0)]], GrassField_0 constant* field_8 [[buffer(7)]], uint device* cluster_lights_8 [[buffer(6)]], GpuLight_natural_0 device* lights_8 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_8 [[texture(1)]], sampler shadow_sampler_8 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_8 [[texture(0)]], sampler grassCardSampler_8 [[sampler(0)]], texture2d<float, access::sample> grassGround_8 [[texture(2)]], WindParams_0 constant* wind_8 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_8 [[texture(3)]], sampler windSampler_8 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_8 [[texture(4)]], GrassInstance_natural_0 device* grassCells_8 [[buffer(8)]])
{

#line 2012
    thread KernelContext_0 kernelContext_38;

#line 2012
    (&kernelContext_38)->tile_1 = tile_16;

#line 2012
    (&kernelContext_38)->grass_0 = grass_8;

#line 2012
    (&kernelContext_38)->instances_0 = instances_8;

#line 2012
    (&kernelContext_38)->blades_1 = blades_9;

#line 2012
    (&kernelContext_38)->frame_0 = frame_8;

#line 2012
    (&kernelContext_38)->field_0 = field_8;

#line 2012
    (&kernelContext_38)->cluster_lights_0 = cluster_lights_8;

#line 2012
    (&kernelContext_38)->lights_0 = lights_8;

#line 2012
    (&kernelContext_38)->shadow_atlas_0 = shadow_atlas_8;

#line 2012
    (&kernelContext_38)->shadow_sampler_0 = shadow_sampler_8;

#line 2012
    (&kernelContext_38)->grassCard_0 = grassCard_8;

#line 2012
    (&kernelContext_38)->grassCardSampler_0 = grassCardSampler_8;

#line 2012
    (&kernelContext_38)->grassGround_0 = grassGround_8;

#line 2012
    (&kernelContext_38)->wind_0 = wind_8;

#line 2012
    (&kernelContext_38)->windDirectionLayer_0 = windDirectionLayer_8;

#line 2012
    (&kernelContext_38)->windSampler_0 = windSampler_8;

#line 2012
    (&kernelContext_38)->windIntensityLayer_0 = windIntensityLayer_8;

#line 2012
    (&kernelContext_38)->grassCells_0 = grassCells_8;

#line 2049
    uint quad_1 = index_8 / 6U;
    uint _S242 = index_8 % 6U;
    uint _S243 = max(field_8->tiles_0.z / 4U, 1U);

    uint _S244 = quad_1 / (_S243 * 64U);

#line 2053
    uint _S245 = min(_S244, 1U);
    uint fin_line_0 = quad_1 / 64U % _S243;



    float band_4 = 4.0f * (&kernelContext_38)->field_0->origin_0.w;
    float across_1 = (float(fin_line_0) + 0.5f) * band_4;
    float stride_0 = (&kernelContext_38)->tile_1->tile_0.z / 16.0f;
    float _S246 = float(quad_1 / 4U % 16U);

#line 2061
    float along_4 = (_S246 + GRASS_CARD_CORNERS_0[_S242].x) * stride_0;
    float middle_along_0 = (_S246 + 0.5f) * stride_0;
    float share_2 = (float(quad_1 % 4U) + GRASS_CARD_CORNERS_0[_S242].y) / 4.0f;



    bool along_z_1 = _S245 == 0U;
    float2 _S247 = (&kernelContext_38)->tile_1->tile_0.xy;

#line 2068
    float2 side_2;

#line 2068
    if(along_z_1)
    {

#line 2068
        side_2 = float2(across_1, along_4);

#line 2068
    }
    else
    {

#line 2068
        side_2 = float2(along_4, across_1);

#line 2068
    }

#line 2068
    float2 rest_5 = _S247 + side_2;

    float2 _S248 = (&kernelContext_38)->tile_1->tile_0.xy;

#line 2070
    if(along_z_1)
    {

#line 2070
        side_2 = float2(across_1, middle_along_0);

#line 2070
    }
    else
    {

#line 2070
        side_2 = float2(middle_along_0, across_1);

#line 2070
    }

#line 2070
    float2 middle_0 = _S248 + side_2;
    if(along_z_1)
    {

#line 2071
        side_2 = float2(0.5f * band_4, 0.0f);

#line 2071
    }
    else
    {

#line 2071
        side_2 = float2(0.0f, 0.5f * band_4);

#line 2071
    }

#line 2071
    GrassGround_0 _S249 = grass_ground_under_0(middle_0, &kernelContext_38);

#line 2071
    GrassGround_0 _S250 = grass_ground_under_0(middle_0 - side_2, &kernelContext_38);

#line 2071
    GrassGround_0 _S251 = grass_ground_under_0(middle_0 + side_2, &kernelContext_38);

#line 2077
    float3 eye_0 = (&kernelContext_38)->frame_0->camera_position_0.xyz - float3(middle_0.x, _S249.height_1 + 0.5f * (&kernelContext_38)->field_0->stack_0.x, middle_0.y);
    float3 view_1 = eye_0 / float3(max(length(eye_0), 9.99999997475242708e-07f)) ;
    float facing_near_0 = dot(_S250.normal_6, view_1);
    float facing_far_0 = dot(_S251.normal_6, view_1);

#line 2080
    float graze_0;

    if((facing_near_0 * facing_far_0) <= 0.0f)
    {

#line 2082
        graze_0 = 0.0f;

#line 2082
    }
    else
    {

#line 2082
        graze_0 = min(abs(facing_near_0), abs(facing_far_0));

#line 2082
    }

    float fade_2 = saturate((0.5f - graze_0) / 0.19999998807907104f);


    if(along_z_1)
    {

#line 2087
        graze_0 = (&kernelContext_38)->tile_1->tile_0.x;

#line 2087
    }
    else
    {

#line 2087
        graze_0 = (&kernelContext_38)->tile_1->tile_0.y;

#line 2087
    }

    float4 _S252 = float4(float(_S245) + 1.0f, fade_2, graze_0 + across_1, 0.0f);

#line 2089
    float _S253 = grass_occlusion_at_0(share_2, &kernelContext_38);

#line 2089
    GrassShellVertex_0 _S254 = grass_sheet_vertex_0(rest_5, share_2, _S252, _S253, &kernelContext_38);

#line 2088
    thread GrassShellVertex_0 output_6 = _S254;

#line 2088
    bool _S255;

    if(fade_2 <= 0.0f)
    {

#line 2090
        _S255 = true;

#line 2090
    }
    else
    {

#line 2090
        _S255 = ((&kernelContext_38)->field_0->stack_0.z) <= 0.0f;

#line 2090
    }

#line 2090
    if(_S255)
    {


        (&output_6)->position_5 = float4(0.0f, 0.0f, -1.0f, 1.0f);

#line 2090
    }

#line 2096
    GrassShellVertex_0 _S256 = output_6;

#line 2096
    thread finVertexMain_Result_0 _S257;

#line 2096
    (&_S257)->position_11 = _S256.position_5;

#line 2096
    (&_S257)->world_position_15 = _S256.world_position_9;

#line 2096
    (&_S257)->rest_4 = _S256.rest_0;

#line 2096
    (&_S257)->normal_14 = _S256.normal_7;

#line 2096
    (&_S257)->footprint_3 = _S256.footprint_0;

#line 2096
    (&_S257)->occlusion_6 = _S256.occlusion_1;

#line 2096
    (&_S257)->fin_4 = _S256.fin_0;

#line 2096
    return _S257;
}

