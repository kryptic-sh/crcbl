#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 755 "shaders/grass.slang"
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) } };

#line 709
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 730
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) } };

#line 743
constant array<uint, int(16)> SHADOW_DITHER_0 = { { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U } };

#line 897
constant array<float2, int(6)> GRASS_CARD_CORNERS_0 = { { float2(0.0f, 0.0f), float2(1.0f, 0.0f), float2(0.0f, 1.0f), float2(1.0f, 0.0f), float2(1.0f, 1.0f), float2(0.0f, 1.0f) } };

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


#line 579
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_0;
    packed_float4 clump_1;
    packed_uint4 lanes_0;
};


#line 583
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


#line 567
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 567
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 567
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


#line 615
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


#line 655
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


#line 1542
GrassBlade_0 grass_row_0(uint index_0, KernelContext_0 thread* kernelContext_0)
{
    GrassBlade_natural_0 _S1 = kernelContext_0->blades_1[min(index_0, max(kernelContext_0->grass_0->limits_0.y, 1U) - 1U)];

#line 1544
    GrassBlade_0 _S2 = { float4(_S1.root_color_0) , float4(_S1.tip_color_0) , float4(_S1.size_0) , float4(_S1.occlusion_0) , float4(_S1.glow_0) , float4(_S1.patch_0) , float4(_S1.shape_0) , float4(_S1.clump_0) , uint4(_S1.flags_0)  };

#line 1544
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


#line 1481
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


#line 1536
float grass_patch_0(uint clump_2)
{
    return grass_unit_pair_0(grass_hash_0(clump_2 ^ 374761393U)).x;
}


#line 1767
float2 grass_blade_lod_0(float distance_0, KernelContext_0 thread* kernelContext_1)
{
    float band_0 = kernelContext_1->field_0->blades_0.y;
    if(!(band_0 > 0.0f))
    {
        return float2(0.0f, 0.0f);
    }
    float half_band_0 = 0.5f * band_0;

    float _S5 = distance_0 - (kernelContext_1->field_0->blades_0.x - band_0);

#line 1776
    return float2(saturate(_S5 / half_band_0), saturate((_S5 - half_band_0) / half_band_0));
}


#line 1670
struct GrassBladeCurve_0
{
    float3 p0_0;
    float3 p1_0;
    float3 p2_0;
    float3 p3_0;
};


#line 1688
GrassBladeCurve_0 grass_blade_curve_0(const GrassInstance_natural_0 thread* blade_0, const GrassBlade_0 thread* row_0)
{

#line 1688
    float4 _S6 = float4(blade_0->root_0) ;

    float height_0 = _S6.w;

#line 1690
    float4 _S7 = float4(blade_0->facing_0) ;
    float _S8 = _S7.x;

#line 1691
    float _S9 = _S7.y;
    float tilt_0 = row_0->shape_0.x;
    float upright_0 = sqrt(max(1.0f - tilt_0 * tilt_0, 0.0f));
    float3 chord_0 = float3(0.0f, height_0 * upright_0, 0.0f) + float3(_S8, 0.0f, _S9) * float3((height_0 * tilt_0)) ;



    float3 bow_0 = float3(_S8 * upright_0, - tilt_0, _S9 * upright_0) * float3((row_0->shape_0.y * height_0)) ;

    thread GrassBladeCurve_0 curve_0;
    float3 _S10 = _S6.xyz;

#line 1701
    (&curve_0)->p0_0 = _S10;

#line 1701
    float3 _S11 = float3(0.3333333432674408f) ;
    (&curve_0)->p1_0 = _S10 + chord_0 * _S11 - bow_0;
    float3 _S12 = (float4(blade_0->lean_0) ).xyz;

#line 1703
    (&curve_0)->p2_0 = _S10 + chord_0 * float3(0.66666668653488159f)  - bow_0 + _S12 * _S11;
    (&curve_0)->p3_0 = _S10 + chord_0 + _S12;
    return curve_0;
}


struct GrassBladePoint_0
{
    float3 position_1;
    float3 normal_0;
};


#line 1725
GrassBladePoint_0 grass_blade_point_0(const GrassBladeCurve_0 thread* curve_1, const GrassInstance_natural_0 thread* blade_1, const GrassBlade_0 thread* row_1, float t_0, float edge_0, float scale_0, KernelContext_0 thread* kernelContext_2)
{

    float u_0 = 1.0f - t_0;
    float _S13 = 3.0f * u_0;

#line 1729
    float _S14 = _S13 * u_0;
    float _S15 = t_0 * t_0;

#line 1730
    float3 centre_0 = curve_1->p0_0 * float3((u_0 * u_0 * u_0))  + curve_1->p1_0 * float3((_S14 * t_0))  + curve_1->p2_0 * float3((_S13 * t_0 * t_0))  + curve_1->p3_0 * float3((_S15 * t_0)) ;

    float3 tangent_1 = (curve_1->p1_0 - curve_1->p0_0) * float3(_S14)  + (curve_1->p2_0 - curve_1->p1_0) * float3((6.0f * u_0 * t_0))  + (curve_1->p3_0 - curve_1->p2_0) * float3((3.0f * t_0 * t_0)) ;

#line 1732
    float4 _S16 = float4(blade_1->facing_0) ;
    float3 side_0 = float3(- _S16.y, 0.0f, _S16.x);

    float3 to_eye_0 = kernelContext_2->frame_0->camera_position_0.xyz - centre_0;
    float3 view_0 = to_eye_0 / float3(max(length(to_eye_0), 9.99999997475242708e-07f)) ;
    float3 perpendicular_0 = cross(tangent_1, view_0);
    float perpendicular_length_0 = length(perpendicular_0);

#line 1738
    float3 screen_side_0;
    if(perpendicular_length_0 > 9.99999997475242708e-07f)
    {

#line 1739
        screen_side_0 = perpendicular_0 / float3(perpendicular_length_0) ;

#line 1739
    }
    else
    {

#line 1739
        screen_side_0 = side_0;

#line 1739
    }
    if((dot(screen_side_0, side_0)) < 0.0f)
    {

#line 1740
        screen_side_0 = - screen_side_0;

#line 1740
    }
    float along_0 = dot(side_0, view_0);
    float side_seen_0 = sqrt(saturate(1.0f - along_0 * along_0));
    float3 direction_1 = side_0 * float3(side_seen_0)  + screen_side_0 * float3((1.0f - side_seen_0)) ;
    float seen_0 = length(direction_1 - view_0 * float3(dot(direction_1, view_0)) );

#line 1749
    float half_width_0 = _S16.z * (1.0f - _S15);
    float least_0 = 0.5f * (max(abs((((float4(centre_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w), 0.00009999999747379f) / max(kernelContext_2->grass_0->screen_0.x, 0.00009999999747379f));

#line 1750
    float widened_0;
    if((half_width_0 * seen_0) < least_0)
    {

#line 1751
        widened_0 = least_0 / max(seen_0, 0.00009999999747379f);

#line 1751
    }
    else
    {

#line 1751
        widened_0 = half_width_0;

#line 1751
    }

    thread GrassBladePoint_0 here_0;
    (&here_0)->position_1 = centre_0 + direction_1 * float3((edge_0 * widened_0 * scale_0)) ;


    float3 normal_1 = cross(tangent_1, side_0);

    (&here_0)->normal_0 = normalize(normal_1 / float3(max(length(normal_1), 9.99999997475242708e-07f))  + side_0 * float3((edge_0 * row_1->shape_0.z)) );
    return here_0;
}


#line 1645
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


#line 1789
GrassVertex_0 grass_blade_vertex_0(const GrassInstance_natural_0 thread* blade_2, uint index_1, uint segments_0, float2 lod_0, KernelContext_0 thread* kernelContext_3)
{

#line 1789
    uint4 _S17 = uint4(blade_2->lanes_0) ;

    uint _S18 = _S17.y;

#line 1791
    GrassBlade_0 _S19 = grass_row_0(_S18, kernelContext_3);

#line 1791
    thread GrassBlade_0 _S20 = _S19;

#line 1791
    GrassBladeCurve_0 _S21 = grass_blade_curve_0(blade_2, &_S20);


    float drop_0 = lod_0.x;
    float morph_0 = lod_0.y;

#line 1795
    float scale_1;


    if((_S17.w) != 0U)
    {

#line 1798
        scale_1 = 1.0f + drop_0;

#line 1798
    }
    else
    {

#line 1798
        scale_1 = 1.0f - drop_0;

#line 1798
    }

    bool tip_0 = index_1 >= (2U * segments_0);
    uint _S22 = min(index_1 / 2U, segments_0);

#line 1801
    float edge_1;
    if(tip_0)
    {

#line 1802
        edge_1 = 0.0f;

#line 1802
    }
    else
    {

#line 1802
        if((index_1 % 2U) == 0U)
        {

#line 1802
            edge_1 = -1.0f;

#line 1802
        }
        else
        {

#line 1802
            edge_1 = 1.0f;

#line 1802
        }

#line 1802
    }

#line 1802
    float t_1;
    if(tip_0)
    {

#line 1803
        t_1 = 1.0f;

#line 1803
    }
    else
    {

#line 1803
        t_1 = float(_S22) / float(segments_0);

#line 1803
    }

#line 1803
    thread GrassBladeCurve_0 _S23 = _S21;

#line 1803
    thread GrassBlade_0 _S24 = _S19;

#line 1803
    GrassBladePoint_0 _S25 = grass_blade_point_0(&_S23, blade_2, &_S24, t_1, edge_1, scale_1, kernelContext_3);
    thread GrassBladePoint_0 here_1 = _S25;

#line 1804
    bool _S26;

    if(morph_0 > 0.0f)
    {

#line 1806
        _S26 = !tip_0;

#line 1806
    }
    else
    {

#line 1806
        _S26 = false;

#line 1806
    }

#line 1806
    if(_S26)
    {

#line 1806
        _S26 = segments_0 != 3U;

#line 1806
    }
    else
    {

#line 1806
        _S26 = false;

#line 1806
    }

#line 1806
    if(_S26)
    {
        uint scaled_0 = _S22 * 3U;
        uint lower_1 = scaled_0 / segments_0;
        float within_0 = float(scaled_0 - lower_1 * segments_0) / float(segments_0);

        float _S27 = float(lower_1) / 3.0f;

#line 1812
        thread GrassBladeCurve_0 _S28 = _S21;

#line 1812
        thread GrassBlade_0 _S29 = _S19;

#line 1812
        GrassBladePoint_0 _S30 = grass_blade_point_0(&_S28, blade_2, &_S29, _S27, edge_1, scale_1, kernelContext_3);

        float _S31 = float(lower_1 + 1U) / 3.0f;

#line 1814
        thread GrassBladeCurve_0 _S32 = _S21;

#line 1814
        thread GrassBlade_0 _S33 = _S19;

#line 1814
        GrassBladePoint_0 _S34 = grass_blade_point_0(&_S32, blade_2, &_S33, _S31, edge_1, scale_1, kernelContext_3);

#line 1814
        float3 _S35 = float3((1.0f - within_0)) ;

#line 1814
        float3 _S36 = float3(within_0) ;

        float3 far_normal_0 = _S30.normal_0 * _S35 + _S34.normal_0 * _S36;

#line 1816
        float3 _S37 = float3((1.0f - morph_0)) ;

#line 1816
        float3 _S38 = float3(morph_0) ;
        (&here_1)->position_1 = (&here_1)->position_1 * _S37 + (_S30.position_1 * _S35 + _S34.position_1 * _S36) * _S38;
        (&here_1)->normal_0 = (&here_1)->normal_0 * _S37 + far_normal_0 * _S38;

#line 1806
    }

#line 1806
    float3 blade_normal_0;

#line 1823
    if((dot((&here_1)->normal_0, kernelContext_3->frame_0->camera_position_0.xyz - (&here_1)->position_1)) < 0.0f)
    {

#line 1823
        blade_normal_0 = - (&here_1)->normal_0;

#line 1823
    }
    else
    {

#line 1823
        blade_normal_0 = (&here_1)->normal_0;

#line 1823
    }

#line 1823
    float3 base_0;



    if((_S19.flags_0.y) == 1U)
    {

#line 1827
        base_0 = float3(0.0f, 1.0f, 0.0f);

#line 1827
    }
    else
    {

#line 1827
        base_0 = (float4(blade_2->ground_0) ).xyz;

#line 1827
    }

#line 1827
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


#line 1554
float3 grass_style_0(float3 color_2, const GrassBlade_0 thread* row_3, float along_1, float patch_2)
{

#line 1554
    float4 _S40 = row_3->occlusion_0;

    float reach_0 = row_3->occlusion_0.w;

#line 1556
    float occluded_0;
    if(reach_0 > 0.0f)
    {

#line 1557
        occluded_0 = saturate(1.0f - along_1 / reach_0);

#line 1557
    }
    else
    {

#line 1557
        occluded_0 = 0.0f;

#line 1557
    }


    float start_0 = row_3->glow_0.w;

    return mix(mix(color_2, _S40.xyz, float3(occluded_0) ), row_3->patch_0.xyz, float3((row_3->patch_0.w * patch_2)) ) + row_3->glow_0.xyz * float3(saturate((along_1 - start_0) / max(1.0f - start_0, 0.00009999999747379f))) ;
}


#line 1408
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_4)
{
    uint _S41 = max(kernelContext_4->frame_0->cluster_grid_0.x, 1U);
    uint _S42 = max(kernelContext_4->frame_0->cluster_grid_0.y, 1U);
    uint _S43 = max(kernelContext_4->frame_0->cluster_grid_0.z, 1U);
    uint _S44 = max(kernelContext_4->frame_0->cluster_grid_0.w, 1U);

#line 1418
    uint _S45 = uint(pixel_0.x) / _S44;

#line 1418
    uint _S46 = min(_S45, _S41 - 1U);
    uint _S47 = uint(pixel_0.y) / _S44;

    float scale_2 = 24.0f / log2(10000.0f);

#line 1429
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_2 + - scale_2 * log2(0.10000000149011612f)), 0.0f, float(_S43 - 1U))) * _S42 + min(_S47, _S42 - 1U)) * _S41 + _S46;
}


#line 1386
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

#line 1405
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

#line 1050
    uint index_3 = 0U;

#line 1050
    float sum_0 = 0.0f;

#line 1050
    float found_0 = 0.0f;



    for(;;)
    {

#line 1054
        if(index_3 < 16U)
        {
        }
        else
        {

#line 1054
            break;
        }
        float2 spoke_2 = SHADOW_SEARCH_DISC_0[index_3] * float2(8.0f) ;
        float _S76 = spoke_2.x;

#line 1057
        float _S77 = rotation_3.x;

#line 1057
        float _S78 = spoke_2.y;

#line 1057
        float _S79 = rotation_3.y;

#line 1065
        int3 _S80 = int3(int2(min(atlas_uv_0(_S71, clamp(tile_uv_5 + float2(_S76 * _S77 - _S78 * _S79, _S76 * _S79 + _S78 * _S77) * _S72, _S73, float2(1.0f)  - _S73)) * _S75, _S75 - _S74)), int(0));

#line 1065
        float depth_1 = ((kernelContext_15->shadow_atlas_0).read(vec<uint,2>(((_S80)).xy), uint(((_S80)).z)));
        if(depth_1 > reference_4)
        {

            float found_1 = found_0 + 1.0f;

#line 1069
            sum_0 = sum_0 + depth_1;

#line 1069
            found_0 = found_1;

#line 1066
        }

#line 1054
        index_3 = index_3 + 1U;

#line 1054
    }

#line 1073
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 1084
    float _S81 = 2.0f * kernelContext_15->frame_0->cascade_far_0[cascade_0];

#line 1084
    float separation_0 = (sum_0 / found_0 - reference_4) * (_S81 + 40.0f);

#line 1084
    float _S82 = tile_texels_0(_S71, kernelContext_15);

    return clamp(separation_0 * 0.01999999955296516f / (_S81 / _S82), 2.0f, 8.0f);
}



float cascade_visibility_0(uint cascade_1, float3 world_position_1, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_16)
{

#line 1092
    float4 _S83 = atlas_rect_0(cascade_1, kernelContext_16);

#line 1126
    if(atlas_rect_is_empty_0(_S83))
    {


        return 1.0f;
    }
    float _S84 = 2.0f * kernelContext_16->frame_0->cascade_far_0[cascade_1];

#line 1132
    float _S85 = tile_texels_0(_S83, kernelContext_16);

#line 1132
    float texel_world_0 = _S84 / _S85;

#line 1139
    float4 clip_0 = (((float4(world_position_1 + geometric_normal_1 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1143
    bool _S86;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1144
        _S86 = true;

#line 1144
    }
    else
    {

#line 1144
        _S86 = (ndc_0.z) <= 0.0f;

#line 1144
    }

#line 1144
    if(_S86)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 1154
    uint _S87 = shadow_filter_mode_0(pixel_4, kernelContext_16);

#line 1171
    if(_S87 == 2U)
    {

#line 1171
        float _S88 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_0.z, kernelContext_16);

        return _S88;
    }
    if(_S87 == 1U)
    {

#line 1175
        float _S89 = tile_pcf_0(cascade_1, tile_uv_6, ndc_0.z, pixel_4, 2.0f, kernelContext_16);



        return _S89;
    }

    float _S90 = ndc_0.z;

#line 1182
    float _S91 = sun_penumbra_texels_0(cascade_1, tile_uv_6, _S90, shadow_rotation_0(pixel_4), kernelContext_16);

#line 1182
    float _S92 = tile_pcf_0(cascade_1, tile_uv_6, _S90, pixel_4, _S91, kernelContext_16);
    return _S92;
}

float sun_visibility_0(float3 world_position_2, float3 to_light_3, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_17)
{
    uint cascade_2;

#line 1188
    bool covered_0;

#line 1197
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 1209
    float eye_distance_0 = length(world_position_2 - kernelContext_17->frame_0->camera_position_0.xyz);

#line 1209
    uint index_4 = 0U;

#line 1217
    for(;;)
    {

#line 1217
        if(index_4 < 2U)
        {
        }
        else
        {

#line 1217
            covered_0 = false;

#line 1217
            cascade_2 = 1U;

#line 1217
            break;
        }
        if(eye_distance_0 < kernelContext_17->frame_0->cascade_far_0[index_4])
        {

#line 1219
            covered_0 = true;

#line 1219
            cascade_2 = index_4;



            break;
        }

#line 1217
        index_4 = index_4 + 1U;

#line 1217
    }

#line 1226
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 1226
    }

#line 1226
    float _S93 = cascade_visibility_0(cascade_2, world_position_2, to_light_3, geometric_normal_2, pixel_5, kernelContext_17);

#line 1233
    uint _S94 = cascade_2 + 1U;

#line 1233
    if(_S94 >= 2U)
    {



        return _S93;
    }

#line 1246
    float band_1 = kernelContext_17->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_17->frame_0->cascade_far_0[cascade_2] - band_1)) / band_1);



    *fade_0 = blend_0;
    if(blend_0 <= 0.0f)
    {
        return _S93;
    }

#line 1254
    float _S95 = cascade_visibility_0(_S94, world_position_2, to_light_3, geometric_normal_2, pixel_5, kernelContext_17);

#line 1265
    return mix(_S93, _S95, blend_0);
}


#line 1354
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S96 = axis_1.x;

#line 1357
    float _S97 = axis_1.y;

#line 1357
    bool _S98;

#line 1357
    if(_S96 >= _S97)
    {

#line 1357
        _S98 = _S96 >= (axis_1.z);

#line 1357
    }
    else
    {

#line 1357
        _S98 = false;

#line 1357
    }

#line 1357
    uint _S99;

#line 1357
    if(_S98)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1359
            _S99 = 0U;

#line 1359
        }
        else
        {

#line 1359
            _S99 = 1U;

#line 1359
        }

#line 1359
        return _S99;
    }
    if(_S97 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1363
            _S99 = 2U;

#line 1363
        }
        else
        {

#line 1363
            _S99 = 3U;

#line 1363
        }

#line 1363
        return _S99;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1365
        _S99 = 4U;

#line 1365
    }
    else
    {

#line 1365
        _S99 = 5U;

#line 1365
    }

#line 1365
    return _S99;
}


#line 902
uint light_tile_0(uint tile_6)
{
    return 2U + tile_6;
}


#line 1268
float punctual_visibility_0(uint tile_7, float3 world_position_3, float3 to_light_4, float n_dot_l_1, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_18)
{

    uint atlas_0 = light_tile_0(tile_7);

#line 1271
    float4 _S100 = atlas_rect_0(atlas_0, kernelContext_18);

    if(atlas_rect_is_empty_0(_S100))
    {


        return 1.0f;
    }

#line 1277
    float _S101 = tile_texels_0(_S100, kernelContext_18);

    float texel_world_1 = map_world_0 / _S101;

#line 1289
    float4 clip_1 = (((float4(world_position_3 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(3)]))));

#line 1296
    float _S102 = clip_1.w;

#line 1296
    if(_S102 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S102) ;

#line 1300
    bool _S103;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1301
        _S103 = true;

#line 1301
    }
    else
    {

#line 1301
        _S103 = (ndc_1.z) <= 0.0f;

#line 1301
    }

#line 1301
    if(_S103)
    {

#line 1301
        _S103 = true;

#line 1301
    }
    else
    {

#line 1301
        _S103 = (ndc_1.z) > 1.0f;

#line 1301
    }

#line 1301
    if(_S103)
    {

#line 1308
        return 1.0f;
    }



    float2 tile_uv_7 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 1313
    uint _S104 = shadow_filter_mode_0(pixel_6, kernelContext_18);

#line 1322
    if(_S104 == 2U)
    {

#line 1322
        float _S105 = tile_box_pcf_0(atlas_0, tile_uv_7, ndc_1.z, kernelContext_18);

        return _S105;
    }

#line 1324
    float _S106 = tile_pcf_0(atlas_0, tile_uv_7, ndc_1.z, pixel_6, 2.0f, kernelContext_18);

    return _S106;
}


#line 1368
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_4, float3 to_light_5, float n_dot_l_2, float3 geometric_normal_4, float2 pixel_7, KernelContext_0 thread* kernelContext_19)
{

    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_4 - (float4(light_0->position_0) ).xyz;

#line 1376
    float _S107 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_4, to_light_5, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_7, kernelContext_19);

#line 1382
    return _S107;
}


#line 1329
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_8, float3 world_position_5, float3 to_light_6, float n_dot_l_3, float3 geometric_normal_5, float2 pixel_8, KernelContext_0 thread* kernelContext_20)
{

    if(n_dot_l_3 <= 0.0f)
    {


        return 1.0f;
    }

#line 1336
    float4 _S108 = float4(light_1->direction_0) ;

#line 1343
    float cos_outer_1 = _S108.w;

#line 1343
    float _S109 = punctual_visibility_0(tile_8, world_position_5, to_light_6, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (float4(light_1->position_0) ).xyz, normalize(_S108.xyz)), 0.0f), geometric_normal_5, pixel_8, kernelContext_20);

#line 1350
    return _S109;
}


#line 1570
float3 grass_light_0(float3 world_position_6, float3 normal_3, float2 pixel_9, KernelContext_0 thread* kernelContext_21)
{

#line 1570
    uint _S110 = froxel_of_0(pixel_9, (((float4(world_position_6, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_21->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_21->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_21->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_21->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w, kernelContext_21);

#line 1581
    uint base_2 = _S110 * 17U;
    uint _S111 = min(kernelContext_21->cluster_lights_0[base_2], 16U);

    float3 _S112 = float3(0.0f, 0.0f, 0.0f);

#line 1584
    uint slot_1 = 0U;

#line 1584
    float3 direct_0 = _S112;
    for(;;)
    {

#line 1585
        if(slot_1 < _S111)
        {
        }
        else
        {

#line 1585
            break;
        }

#line 1585
        thread GpuLight_natural_0 _S113 = kernelContext_21->lights_0[kernelContext_21->cluster_lights_0[base_2 + 1U + slot_1]];

#line 1585
        uint _S114 = (&_S113)->kind_0;


        if(((&_S113)->kind_0) == 3U)
        {



            slot_1 = slot_1 + 1U;

#line 1585
            continue;
        }

#line 1598
        bool _S115 = _S114 == 0U;

#line 1598
        float3 to_light_7;

#line 1598
        float reach_1;

#line 1598
        if(_S115)
        {

#line 1598
            to_light_7 = normalize((float4((&_S113)->direction_0) ).xyz);

#line 1598
            reach_1 = 1.0f;

#line 1598
        }
        else
        {

#line 1598
            float4 _S116 = float4((&_S113)->position_0) ;

#line 1605
            float3 offset_0 = _S116.xyz - world_position_6;
            float distance_3 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;
            float reach_2 = punctual_falloff_0(distance_3, _S116.w);
            if(_S114 == 2U)
            {

#line 1609
                float4 _S117 = float4((&_S113)->direction_0) ;

#line 1609
                reach_1 = reach_2 * spot_cone_0(to_light_8, _S117.xyz, _S117.w, (&_S113)->cos_inner_0);

#line 1609
            }
            else
            {

#line 1609
                reach_1 = reach_2;

#line 1609
            }

#line 1609
            to_light_7 = to_light_8;

#line 1598
        }

#line 1616
        float n_dot_l_4 = dot(normal_3, to_light_7);

#line 1616
        float reach_3;
        if(_S115)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 1620
            float _S118 = sun_visibility_0(world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, &sun_cascade_0, &sun_fade_0, kernelContext_21);

#line 1620
            reach_3 = _S118;

#line 1617
        }
        else
        {

#line 1624
            if(_S114 == 1U)
            {

#line 1624
                uint _S119 = (&_S113)->shadow_tile_0;

                if(((&_S113)->shadow_tile_0) <= 8U)
                {

#line 1626
                    float _S120 = point_visibility_0(&_S113, _S119, world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, kernelContext_21);

#line 1626
                    reach_3 = reach_1 * _S120;

#line 1626
                }
                else
                {

#line 1626
                    reach_3 = reach_1;

#line 1626
                }

#line 1624
            }
            else
            {

#line 1624
                uint _S121 = (&_S113)->shadow_tile_0;

#line 1632
                if(((&_S113)->shadow_tile_0) < 14U)
                {

#line 1632
                    float _S122 = spot_visibility_0(&_S113, _S121, world_position_6, to_light_7, n_dot_l_4, normal_3, pixel_9, kernelContext_21);

#line 1632
                    reach_3 = reach_1 * _S122;

#line 1632
                }
                else
                {

#line 1632
                    reach_3 = reach_1;

#line 1632
                }

#line 1624
            }

#line 1617
        }

#line 1617
        direct_0 = direct_0 + (float4((&_S113)->color_0) ).xyz * float3((max(n_dot_l_4, 0.0f) * reach_3)) ;

#line 1585
        slot_1 = slot_1 + 1U;

#line 1585
    }

#line 1641
    return direct_0 + kernelContext_21->frame_0->ambient_0.xyz;
}


#line 1641
struct pixelOutput_0
{
    float4 output_1 [[color(0)]];
};


#line 1641
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


#line 1641
float3 grass_style_1(float3 _S123, uint _S124, float _S125, float _S126, KernelContext_0 thread* kernelContext_22)
{

#line 1641
    GrassBlade_natural_0 device* _S127 = kernelContext_22->blades_1+_S124;

#line 1641
    float4 _S128 = float4(_S127->occlusion_0) ;

#line 1556
    float reach_4 = _S128.w;

#line 1556
    float occluded_1;
    if(reach_4 > 0.0f)
    {

#line 1557
        occluded_1 = saturate(1.0f - _S125 / reach_4);

#line 1557
    }
    else
    {

#line 1557
        occluded_1 = 0.0f;

#line 1557
    }

#line 1557
    float4 _S129 = float4(_S127->patch_0) ;

#line 1557
    float4 _S130 = float4(_S127->glow_0) ;


    float start_1 = _S130.w;

    return mix(mix(_S123, _S128.xyz, float3(occluded_1) ), _S129.xyz, float3((_S129.w * _S126)) ) + _S130.xyz * float3(saturate((_S125 - start_1) / max(1.0f - start_1, 0.00009999999747379f))) ;
}


#line 1917
[[fragment]] pixelOutput_0 bladeFragmentMain(pixelInput_0 _S131 [[stage_in]], float4 position_3 [[position]], GrassTile_0 constant* tile_9 [[buffer(2)]], GrassParams_0 constant* grass_1 [[buffer(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GrassField_0 constant* field_1 [[buffer(7)]], uint device* cluster_lights_1 [[buffer(6)]], GpuLight_natural_0 device* lights_1 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_1 [[texture(0)]], sampler grassCardSampler_1 [[sampler(0)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(3)]], sampler windSampler_1 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(4)]], GrassInstance_natural_0 device* grassCells_1 [[buffer(8)]])
{

#line 1917
    thread KernelContext_0 kernelContext_23;

#line 1917
    (&kernelContext_23)->tile_1 = tile_9;

#line 1917
    (&kernelContext_23)->grass_0 = grass_1;

#line 1917
    (&kernelContext_23)->instances_0 = instances_1;

#line 1917
    (&kernelContext_23)->blades_1 = blades_2;

#line 1917
    (&kernelContext_23)->frame_0 = frame_1;

#line 1917
    (&kernelContext_23)->field_0 = field_1;

#line 1917
    (&kernelContext_23)->cluster_lights_0 = cluster_lights_1;

#line 1917
    (&kernelContext_23)->lights_0 = lights_1;

#line 1917
    (&kernelContext_23)->shadow_atlas_0 = shadow_atlas_1;

#line 1917
    (&kernelContext_23)->shadow_sampler_0 = shadow_sampler_1;

#line 1917
    (&kernelContext_23)->grassCard_0 = grassCard_1;

#line 1917
    (&kernelContext_23)->grassCardSampler_0 = grassCardSampler_1;

#line 1917
    (&kernelContext_23)->grassGround_0 = grassGround_1;

#line 1917
    (&kernelContext_23)->wind_0 = wind_1;

#line 1917
    (&kernelContext_23)->windDirectionLayer_0 = windDirectionLayer_1;

#line 1917
    (&kernelContext_23)->windSampler_0 = windSampler_1;

#line 1917
    (&kernelContext_23)->windIntensityLayer_0 = windIntensityLayer_1;

#line 1917
    (&kernelContext_23)->grassCells_0 = grassCells_1;

#line 1917
    float3 _S132 = grass_style_1(_S131.color_3, _S131.row_4, _S131.uv_1.y, _S131.patch_3, &kernelContext_23);

#line 1917
    float3 _S133 = grass_light_0(_S131.world_position_7, normalize(_S131.normal_4), position_3.xy, &kernelContext_23);

#line 1917
    pixelOutput_0 _S134 = { float4(_S132 * _S133, 1.0f) };



    return _S134;
}


#line 1921
struct pixelOutput_1
{
    float4 output_2 [[color(0)]];
};


#line 1921
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


#line 1926
[[fragment]] pixelOutput_1 fragmentMain(pixelInput_1 _S135 [[stage_in]], float4 position_4 [[position]], GrassTile_0 constant* tile_10 [[buffer(2)]], GrassParams_0 constant* grass_2 [[buffer(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(3)]], GrassBlade_natural_0 device* blades_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GrassField_0 constant* field_2 [[buffer(7)]], uint device* cluster_lights_2 [[buffer(6)]], GpuLight_natural_0 device* lights_2 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_2 [[texture(0)]], sampler grassCardSampler_2 [[sampler(0)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(3)]], sampler windSampler_2 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(4)]], GrassInstance_natural_0 device* grassCells_2 [[buffer(8)]])
{

#line 1926
    thread KernelContext_0 kernelContext_24;

#line 1926
    (&kernelContext_24)->tile_1 = tile_10;

#line 1926
    (&kernelContext_24)->grass_0 = grass_2;

#line 1926
    (&kernelContext_24)->instances_0 = instances_2;

#line 1926
    (&kernelContext_24)->blades_1 = blades_3;

#line 1926
    (&kernelContext_24)->frame_0 = frame_2;

#line 1926
    (&kernelContext_24)->field_0 = field_2;

#line 1926
    (&kernelContext_24)->cluster_lights_0 = cluster_lights_2;

#line 1926
    (&kernelContext_24)->lights_0 = lights_2;

#line 1926
    (&kernelContext_24)->shadow_atlas_0 = shadow_atlas_2;

#line 1926
    (&kernelContext_24)->shadow_sampler_0 = shadow_sampler_2;

#line 1926
    (&kernelContext_24)->grassCard_0 = grassCard_2;

#line 1926
    (&kernelContext_24)->grassCardSampler_0 = grassCardSampler_2;

#line 1926
    (&kernelContext_24)->grassGround_0 = grassGround_2;

#line 1926
    (&kernelContext_24)->wind_0 = wind_2;

#line 1926
    (&kernelContext_24)->windDirectionLayer_0 = windDirectionLayer_2;

#line 1926
    (&kernelContext_24)->windSampler_0 = windSampler_2;

#line 1926
    (&kernelContext_24)->windIntensityLayer_0 = windIntensityLayer_2;

#line 1926
    (&kernelContext_24)->grassCells_0 = grassCells_2;

#line 1932
    float _S136 = _S135.uv_2.y;

#line 1932
    float3 _S137 = float3(_S135.uv_2.x, 1.0f - _S136, 0.0f);

    if((((grassCard_2).sample((grassCardSampler_2), ((_S137)).xy, uint(((_S137)).z), level((_S135.level_3)))).x) < 0.5f)
    {
        discard_fragment();

#line 1934
    }

#line 1934
    float3 _S138 = grass_style_1(_S135.color_4, _S135.row_5, _S136, _S135.patch_4, &kernelContext_24);

#line 1934
    float3 _S139 = grass_light_0(_S135.world_position_8, normalize(_S135.normal_5), position_4.xy, &kernelContext_24);

#line 1934
    pixelOutput_1 _S140 = { float4(_S138 * _S139, 1.0f) };

#line 1942
    return _S140;
}


#line 1997
uint grass_shell_count_0(KernelContext_0 thread* kernelContext_25)
{
    return clamp(uint(kernelContext_25->field_0->stack_0.y + 0.5f), 1U, 64U);
}


#line 1504
float grass_ground_texel_0(int2 texel_1, KernelContext_0 thread* kernelContext_26)
{

    int3 _S141 = int3(clamp(texel_1, int2(int(0), int(0)), int2(int(kernelContext_26->field_0->maps_0.x) - int(1), int(kernelContext_26->field_0->maps_0.y) - int(1))), int(0));

#line 1507
    return ((kernelContext_26->grassGround_0).read(vec<uint,2>(((_S141)).xy), uint(((_S141)).z)).x);
}


#line 1495
struct GrassGround_0
{
    float height_1;
    float3 normal_6;
};


#line 1512
GrassGround_0 grass_ground_under_0(float2 world_0, KernelContext_0 thread* kernelContext_27)
{
    float2 at_0 = (world_0 - kernelContext_27->field_0->ground_1.xy) * float2(kernelContext_27->field_0->ground_1.w) ;
    float2 base_3 = floor(at_0);
    float2 blend_1 = at_0 - base_3;
    int2 _S142 = int2(base_3);

#line 1517
    float _S143 = grass_ground_texel_0(_S142, kernelContext_27);

#line 1517
    float _S144 = grass_ground_texel_0(_S142 + int2(int(1), int(0)), kernelContext_27);

#line 1517
    float _S145 = grass_ground_texel_0(_S142 + int2(int(0), int(1)), kernelContext_27);

#line 1517
    float _S146 = grass_ground_texel_0(_S142 + int2(int(1), int(1)), kernelContext_27);

#line 1524
    float _S147 = blend_1.x;

#line 1524
    float _S148 = _S144 - _S143;

#line 1524
    float lower_2 = _S143 + _S147 * _S148;
    float _S149 = _S146 - _S145;

#line 1523
    thread GrassGround_0 under_0;


    (&under_0)->height_1 = lower_2 + blend_1.y * (_S145 + _S147 * _S149 - lower_2);


    (&under_0)->normal_6 = normalize(float3(- (0.5f * (_S148 + _S149)), kernelContext_27->field_0->ground_1.z, - (0.5f * (_S145 - _S143 + (_S146 - _S144)))));
    return under_0;
}


#line 1434
float windSmoothTriangle_0(float u_1)
{
    float s_0 = abs(fract(u_1 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}


float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_28)
{


    float2 _S150 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_28->windDirectionLayer_0).sample((kernelContext_28->windSampler_0), (kernelContext_28->wind_0->directionUv_0 + _S150 * kernelContext_28->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_28->windIntensityLayer_0).sample((kernelContext_28->windSampler_0), (kernelContext_28->wind_0->intensityUv_0 + _S150 * kernelContext_28->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;

    float2 base_4 = kernelContext_28->wind_0->baseDirection_0;
    float _S151 = kernelContext_28->wind_0->baseDirection_0.x;

#line 1455
    float _S152 = deflection_0.x;

#line 1455
    float _S153 = kernelContext_28->wind_0->baseDirection_0.y;

#line 1455
    float _S154 = deflection_0.y;

#line 1455
    float2 turned_0 = float2(_S151 * _S152 - _S153 * _S154, _S151 * _S154 + _S153 * _S152);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 1457
    float2 direction_2;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 1459
        direction_2 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 1459
    }
    else
    {

#line 1459
        direction_2 = base_4;

#line 1459
    }

#line 1464
    float speed_0 = intensity_0 * kernelContext_28->wind_0->baseSpeed_0 * (1.0f + kernelContext_28->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S150, base_4) * kernelContext_28->wind_0->invGustWavelength_0 + kernelContext_28->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_2.x * speed_0, 0.0f, direction_2.y * speed_0);
}



float3 grass_lean_0(float3 velocity_0, float height_2)
{
    float speed_1 = length(velocity_0);
    float bend_0 = height_2 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_2 = velocity_0.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;
    float _S155 = bend_0 * bend_0;
    return float3(along_2.x, - (_S155 / (height_2 + sqrt(max(height_2 * height_2 - _S155, 0.0f)))), along_2.y);
}


#line 1946
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


#line 1970
GrassShellVertex_0 grass_sheet_vertex_0(float2 rest_1, float share_0, float4 fin_1, float occlusion_2, KernelContext_0 thread* kernelContext_29)
{

#line 1970
    GrassGround_0 _S156 = grass_ground_under_0(rest_1, kernelContext_29);


    float lift_0 = share_0 * kernelContext_29->field_0->stack_0.x;
    float _S157 = rest_1.x;

#line 1974
    float _S158 = rest_1.y;

#line 1974
    float3 ground_2 = float3(_S157, _S156.height_1, _S158);

#line 1974
    float3 _S159 = windSample_0(ground_2 - kernelContext_29->frame_0->camera_position_0.xyz, kernelContext_29);

#line 1981
    float3 world_1 = ground_2 + float3(0.0f, lift_0, 0.0f) + grass_lean_0(_S159, kernelContext_29->field_0->stack_0.x) * float3((share_0 * share_0)) ;

    thread GrassShellVertex_0 output_3;
    float4 _S160 = (((float4(world_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_29->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_29->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_29->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_29->frame_0->view_proj_0.data_0[int(3)][int(3)]))));

#line 1984
    (&output_3)->position_5 = _S160;
    (&output_3)->world_position_9 = world_1;
    (&output_3)->rest_0 = float3(_S157, lift_0, _S158);
    (&output_3)->normal_7 = _S156.normal_6;


    (&output_3)->footprint_0 = max(abs(_S160.w), 0.00009999999747379f) / max(kernelContext_29->grass_0->screen_0.x, 0.00009999999747379f);
    (&output_3)->occlusion_1 = occlusion_2;
    (&output_3)->fin_0 = fin_1;
    return output_3;
}


#line 2019
float grass_occlusion_at_0(float share_1, KernelContext_0 thread* kernelContext_30)
{

#line 2019
    uint _S161 = grass_shell_count_0(kernelContext_30);

#line 2019
    float occlusion_3 = kernelContext_30->field_0->layers_0[int(0)].y;

#line 2019
    uint shell_0 = 0U;



    for(;;)
    {

#line 2023
        if(shell_0 < _S161)
        {
        }
        else
        {

#line 2023
            break;
        }
        if((kernelContext_30->field_0->layers_0[shell_0].x) <= share_1)
        {

#line 2025
            occlusion_3 = kernelContext_30->field_0->layers_0[shell_0].y;

#line 2025
        }

#line 2023
        shell_0 = shell_0 + 1U;

#line 2023
    }

#line 2030
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


#line 2097
GrassInstance_0 grass_no_blade_0()
{
    thread GrassInstance_0 none_0;
    float4 _S162 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 2100
    (&none_0)->root_0 = _S162;
    (&none_0)->facing_0 = _S162;
    (&none_0)->lean_0 = _S162;
    (&none_0)->ground_0 = _S162;
    (&none_0)->lanes_0 = uint4(0U, 0U, 0U, 0U);
    return none_0;
}



GrassInstance_0 grass_cell_0(int2 global_0, KernelContext_0 thread* kernelContext_31)
{
    int side_1 = int(max(kernelContext_31->field_0->tiles_0.z, 1U));
    int2 extent_0 = int2(int(kernelContext_31->field_0->tiles_0.x), int(kernelContext_31->field_0->tiles_0.y)) * int2(side_1) ;
    int _S163 = global_0.x;

#line 2114
    bool _S164;

#line 2114
    if(_S163 < int(0))
    {

#line 2114
        _S164 = true;

#line 2114
    }
    else
    {

#line 2114
        _S164 = (global_0.y) < int(0);

#line 2114
    }

#line 2114
    if(_S164)
    {

#line 2114
        _S164 = true;

#line 2114
    }
    else
    {

#line 2114
        _S164 = _S163 >= (extent_0.x);

#line 2114
    }

#line 2114
    if(_S164)
    {

#line 2114
        _S164 = true;

#line 2114
    }
    else
    {

#line 2114
        _S164 = (global_0.y) >= (extent_0.y);

#line 2114
    }

#line 2114
    if(_S164)
    {
        return grass_no_blade_0();
    }
    uint2 _S165 = uint2(global_0);
    uint cells_0 = uint(side_1);
    uint _S166 = _S165.y;

#line 2120
    uint _S167 = _S166 / cells_0;

#line 2120
    uint _S168 = _S167 * kernelContext_31->field_0->tiles_0.x;

#line 2120
    uint _S169 = _S165.x;

#line 2120
    uint _S170 = _S169 / cells_0;

#line 2120
    uint slot_2 = _S168 + _S170;
    uint _S171 = _S166 % cells_0;

#line 2121
    uint _S172 = _S171 * cells_0;

#line 2121
    uint _S173 = _S169 % cells_0;
    GrassInstance_natural_0 _S174 = kernelContext_31->grassCells_0[slot_2 * kernelContext_31->field_0->tiles_0.w + (_S172 + _S173)];

#line 2122
    GrassInstance_0 _S175 = { float4(_S174.root_0) , float4(_S174.facing_0) , float4(_S174.lean_0) , float4(_S174.ground_0) , float4(_S174.clump_1) , uint4(_S174.lanes_0)  };

#line 2122
    return _S175;
}




float grass_strand_reach_0(const GrassInstance_0 thread* blade_3, float lift_1, float floor_reach_0, float fade_1, KernelContext_0 thread* kernelContext_32)
{
    float _S176 = blade_3->root_0.w;

#line 2130
    bool _S177;

#line 2130
    if(!(lift_1 < _S176))
    {

#line 2130
        _S177 = true;

#line 2130
    }
    else
    {

#line 2130
        GrassBlade_0 _S178 = grass_row_0(blade_3->lanes_0.y, kernelContext_32);

#line 2130
        _S177 = (_S178.flags_0.x) != 1U;

#line 2130
    }

#line 2130
    if(_S177)
    {
        return -1.0f;
    }
    float bound_0 = 0.5f * kernelContext_32->field_0->origin_0.w;

    return min(max(min(blade_3->facing_0.z, bound_0) * (1.0f - lift_1 / _S176), floor_reach_0), bound_0) * fade_1;
}


#line 2136
struct pixelOutput_2
{
    float4 output_4 [[color(0)]];
};


#line 2136
struct pixelInput_2
{
    float3 world_position_10 [[user(WORLD_POSITION)]];
    float3 rest_2 [[user(REST_POSITION)]];
    float3 normal_8 [[user(NORMAL)]];
    float footprint_1 [[user(FOOTPRINT)]];
    float occlusion_4 [[user(OCCLUSION)]];
    [[flat]] float4 fin_2 [[user(FIN)]];
};


#line 2141
[[fragment]] pixelOutput_2 shellFragmentMain(pixelInput_2 _S179 [[stage_in]], float4 position_6 [[position]], GrassTile_0 constant* tile_11 [[buffer(2)]], GrassParams_0 constant* grass_3 [[buffer(1)]], GrassInstance_natural_0 device* instances_3 [[buffer(3)]], GrassBlade_natural_0 device* blades_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], GrassField_0 constant* field_3 [[buffer(7)]], uint device* cluster_lights_3 [[buffer(6)]], GpuLight_natural_0 device* lights_3 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_3 [[texture(0)]], sampler grassCardSampler_3 [[sampler(0)]], texture2d<float, access::sample> grassGround_3 [[texture(2)]], WindParams_0 constant* wind_3 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_3 [[texture(3)]], sampler windSampler_3 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_3 [[texture(4)]], GrassInstance_natural_0 device* grassCells_3 [[buffer(8)]])
{

#line 2141
    thread KernelContext_0 kernelContext_33;

#line 2141
    (&kernelContext_33)->tile_1 = tile_11;

#line 2141
    (&kernelContext_33)->grass_0 = grass_3;

#line 2141
    (&kernelContext_33)->instances_0 = instances_3;

#line 2141
    (&kernelContext_33)->blades_1 = blades_4;

#line 2141
    (&kernelContext_33)->frame_0 = frame_3;

#line 2141
    (&kernelContext_33)->field_0 = field_3;

#line 2141
    (&kernelContext_33)->cluster_lights_0 = cluster_lights_3;

#line 2141
    (&kernelContext_33)->lights_0 = lights_3;

#line 2141
    (&kernelContext_33)->shadow_atlas_0 = shadow_atlas_3;

#line 2141
    (&kernelContext_33)->shadow_sampler_0 = shadow_sampler_3;

#line 2141
    (&kernelContext_33)->grassCard_0 = grassCard_3;

#line 2141
    (&kernelContext_33)->grassCardSampler_0 = grassCardSampler_3;

#line 2141
    (&kernelContext_33)->grassGround_0 = grassGround_3;

#line 2141
    (&kernelContext_33)->wind_0 = wind_3;

#line 2141
    (&kernelContext_33)->windDirectionLayer_0 = windDirectionLayer_3;

#line 2141
    (&kernelContext_33)->windSampler_0 = windSampler_3;

#line 2141
    (&kernelContext_33)->windIntensityLayer_0 = windIntensityLayer_3;

#line 2141
    (&kernelContext_33)->grassCells_0 = grassCells_3;

    float cell_1 = field_3->origin_0.w;
    float2 _S180 = _S179.rest_2.xz;

#line 2144
    float2 from_corner_0 = (_S180 - field_3->origin_0.xy) / float2(cell_1) ;
    float2 _S181 = floor(from_corner_0);

#line 2145
    int2 _S182 = int2(_S181);
    float2 inside_0 = from_corner_0 - _S181;

#line 2146
    int _S183;
    if((inside_0.x) < 0.5f)
    {

#line 2147
        _S183 = int(-1);

#line 2147
    }
    else
    {

#line 2147
        _S183 = int(1);

#line 2147
    }

#line 2147
    int band_2;

#line 2147
    if((inside_0.y) < 0.5f)
    {

#line 2147
        band_2 = int(-1);

#line 2147
    }
    else
    {

#line 2147
        band_2 = int(1);

#line 2147
    }
    float lift_2 = _S179.rest_2.y;


    float _S184 = 0.5f * _S179.footprint_1;
    float _S185 = _S179.fin_2.y;

    GrassInstance_0 _S186 = grass_no_blade_0();

    float _S187 = _S179.fin_2.x;

#line 2156
    int dz_0;

#line 2156
    int dx_0;

#line 2156
    float nearest_0;

#line 2156
    float nearest_1;

#line 2156
    GrassInstance_0 found_2;

#line 2156
    GrassInstance_0 found_3;

#line 2156
    bool _S188;

#line 2156
    if(_S187 < 0.5f)
    {

#line 2156
        nearest_1 = 2.0f;

#line 2156
        found_3 = _S186;

#line 2156
        dz_0 = int(0);


        for(;;)
        {

#line 2159
            if(dz_0 < int(2))
            {
            }
            else
            {

#line 2159
                break;
            }

#line 2159
            nearest_0 = nearest_1;

#line 2159
            found_2 = found_3;

#line 2159
            dx_0 = int(0);

            for(;;)
            {

#line 2161
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2161
                    break;
                }

#line 2161
                GrassInstance_0 _S189 = grass_cell_0(_S182 + int2(dx_0 * _S183, dz_0 * band_2), &kernelContext_33);

#line 2161
                thread GrassInstance_0 _S190 = _S189;

#line 2161
                float _S191 = grass_strand_reach_0(&_S190, lift_2, _S184, _S185, &kernelContext_33);



                float2 offset_1 = _S180 - _S189.root_0.xz;
                float apart_0 = sqrt(dot(offset_1, offset_1)) / max(_S191, 9.99999997475242708e-07f);
                if(_S191 > 0.0f)
                {

#line 2167
                    _S188 = apart_0 < nearest_0;

#line 2167
                }
                else
                {

#line 2167
                    _S188 = false;

#line 2167
                }

#line 2167
                if(_S188)
                {

#line 2167
                    nearest_0 = apart_0;

#line 2167
                    found_2 = _S189;

#line 2167
                }

#line 2161
                dx_0 = dx_0 + int(1);

#line 2161
            }

#line 2159
            int dz_1 = dz_0 + int(1);

#line 2159
            nearest_1 = nearest_0;

#line 2159
            found_3 = found_2;

#line 2159
            dz_0 = dz_1;

#line 2159
        }

#line 2159
        nearest_0 = nearest_1;

#line 2159
        found_2 = found_3;

#line 2156
    }
    else
    {

#line 2179
        bool along_z_0 = _S187 < 1.5f;
        float _S192 = _S179.fin_2.z;

#line 2180
        if(along_z_0)
        {

#line 2180
            nearest_0 = field_3->origin_0.x;

#line 2180
        }
        else
        {

#line 2180
            nearest_0 = field_3->origin_0.y;

#line 2180
        }
        int _S193 = int(floor((_S192 - nearest_0) / cell_1 + 0.5f)) - int(2);
        if(along_z_0)
        {

#line 2182
            dz_0 = _S182.y;

#line 2182
        }
        else
        {

#line 2182
            dz_0 = _S182.x;

#line 2182
        }
        if(along_z_0)
        {

#line 2183
            _S183 = band_2;

#line 2183
        }

#line 2183
        nearest_0 = 2.0f;

#line 2183
        found_2 = _S186;

#line 2183
        band_2 = int(0);
        for(;;)
        {

#line 2184
            if(band_2 < int(4))
            {
            }
            else
            {

#line 2184
                break;
            }

#line 2184
            nearest_1 = nearest_0;

#line 2184
            found_3 = found_2;

#line 2184
            dx_0 = int(0);

            for(;;)
            {

#line 2186
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2186
                    break;
                }
                int along_3 = dz_0 + dx_0 * _S183;

#line 2188
                int2 _S194;
                if(along_z_0)
                {

#line 2189
                    _S194 = int2(_S193 + band_2, along_3);

#line 2189
                }
                else
                {

#line 2189
                    _S194 = int2(along_3, _S193 + band_2);

#line 2189
                }

#line 2189
                GrassInstance_0 _S195 = grass_cell_0(_S194, &kernelContext_33);

#line 2189
                thread GrassInstance_0 _S196 = _S195;

#line 2189
                float _S197 = grass_strand_reach_0(&_S196, lift_2, _S184, _S185, &kernelContext_33);

#line 2189
                float offset_2;



                if(along_z_0)
                {

#line 2193
                    offset_2 = _S179.rest_2.z - _S195.root_0.z;

#line 2193
                }
                else
                {

#line 2193
                    offset_2 = _S179.rest_2.x - _S195.root_0.x;

#line 2193
                }
                float apart_1 = abs(offset_2) / max(_S197, 9.99999997475242708e-07f);
                if(_S197 > 0.0f)
                {

#line 2195
                    _S188 = apart_1 < nearest_1;

#line 2195
                }
                else
                {

#line 2195
                    _S188 = false;

#line 2195
                }

#line 2195
                if(_S188)
                {

#line 2195
                    nearest_1 = apart_1;

#line 2195
                    found_3 = _S195;

#line 2195
                }

#line 2186
                dx_0 = dx_0 + int(1);

#line 2186
            }

#line 2184
            int band_3 = band_2 + int(1);

#line 2184
            nearest_0 = nearest_1;

#line 2184
            found_2 = found_3;

#line 2184
            band_2 = band_3;

#line 2184
        }

#line 2156
    }

#line 2203
    if(!(nearest_0 < 1.0f))
    {
        discard_fragment();

#line 2203
    }

#line 2203
    GrassBlade_0 _S198 = grass_row_0(found_2.lanes_0.y, &kernelContext_33);

#line 2209
    float along_blade_0 = saturate(lift_2 / found_2.root_0.w);

    float3 color_5 = mix(_S198.root_color_0.xyz, _S198.tip_color_0.xyz, float3(along_blade_0) ) * float3((1.0f - 0.18000000715255737f * found_2.facing_0.w)) ;
    float _S199 = grass_patch_0(found_2.lanes_0.z);

#line 2212
    thread GrassBlade_0 _S200 = _S198;

#line 2212
    float3 _S201 = grass_style_0(color_5, &_S200, along_blade_0, _S199);

#line 2212
    float3 color_6 = _S201 * float3(_S179.occlusion_4) ;

#line 2212
    float3 normal_9;

    if((_S198.flags_0.y) == 1U)
    {

#line 2214
        normal_9 = float3(0.0f, 1.0f, 0.0f);

#line 2214
    }
    else
    {

#line 2214
        normal_9 = normalize(_S179.normal_8);

#line 2214
    }

#line 2214
    float3 _S202 = grass_light_0(_S179.world_position_10, normal_9, position_6.xy, &kernelContext_33);

#line 2214
    pixelOutput_2 _S203 = { float4(color_6 * _S202, 1.0f) };
    return _S203;
}


#line 2215
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


#line 2215
[[vertex]] vertexMain_Result_0 vertexMain(uint index_5 [[vertex_id]], uint instance_id_0 [[instance_id]], uint base_instance_0 [[base_instance]], GrassTile_0 constant* tile_12 [[buffer(2)]], GrassParams_0 constant* grass_4 [[buffer(1)]], GrassInstance_natural_0 device* instances_4 [[buffer(3)]], GrassBlade_natural_0 device* blades_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], GrassField_0 constant* field_4 [[buffer(7)]], uint device* cluster_lights_4 [[buffer(6)]], GpuLight_natural_0 device* lights_4 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_4 [[texture(0)]], sampler grassCardSampler_4 [[sampler(0)]], texture2d<float, access::sample> grassGround_4 [[texture(2)]], WindParams_0 constant* wind_4 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_4 [[texture(3)]], sampler windSampler_4 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_4 [[texture(4)]], GrassInstance_natural_0 device* grassCells_4 [[buffer(8)]])
{

#line 2215
    thread KernelContext_0 kernelContext_34;

#line 2215
    (&kernelContext_34)->tile_1 = tile_12;

#line 2215
    (&kernelContext_34)->grass_0 = grass_4;

#line 2215
    (&kernelContext_34)->instances_0 = instances_4;

#line 2215
    (&kernelContext_34)->blades_1 = blades_5;

#line 2215
    (&kernelContext_34)->frame_0 = frame_4;

#line 2215
    (&kernelContext_34)->field_0 = field_4;

#line 2215
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_4;

#line 2215
    (&kernelContext_34)->lights_0 = lights_4;

#line 2215
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_4;

#line 2215
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_4;

#line 2215
    (&kernelContext_34)->grassCard_0 = grassCard_4;

#line 2215
    (&kernelContext_34)->grassCardSampler_0 = grassCardSampler_4;

#line 2215
    (&kernelContext_34)->grassGround_0 = grassGround_4;

#line 2215
    (&kernelContext_34)->wind_0 = wind_4;

#line 2215
    (&kernelContext_34)->windDirectionLayer_0 = windDirectionLayer_4;

#line 2215
    (&kernelContext_34)->windSampler_0 = windSampler_4;

#line 2215
    (&kernelContext_34)->windIntensityLayer_0 = windIntensityLayer_4;

#line 2215
    (&kernelContext_34)->grassCells_0 = grassCells_4;

#line 1851
    GrassInstance_natural_0 blade_4 = instances_4[tile_12->slot_0.x * grass_4->limits_0.x + (instance_id_0 - base_instance_0)];

#line 1851
    uint4 _S204 = uint4(blade_4.lanes_0) ;
    uint _S205 = _S204.y;

#line 1852
    GrassBlade_0 _S206 = grass_row_0(_S205, &kernelContext_34);


    uint _S207 = index_5 % 6U;

#line 1855
    float4 _S208 = float4(blade_4.facing_0) ;

#line 1860
    float2 facing_1 = _S208.xy;

#line 1860
    float2 across_0;
    if((index_5 / 6U) == 0U)
    {

#line 1861
        across_0 = facing_1;

#line 1861
    }
    else
    {

#line 1861
        across_0 = float2(- facing_1.y, facing_1.x);

#line 1861
    }

    float _S209 = _S208.z;

#line 1868
    float _S210 = GRASS_CARD_CORNERS_0[_S207].y;

#line 1868
    float4 _S211 = float4(blade_4.root_0) ;
    float3 world_2 = _S211.xyz + float3(across_0.x, 0.0f, across_0.y) * float3((_S209 * (GRASS_CARD_CORNERS_0[_S207].x * 2.0f - 1.0f)))  + float3(0.0f, _S211.w * _S210, 0.0f) + (float4(blade_4.lean_0) ).xyz * float3((_S210 * _S210)) ;

    thread GrassVertex_0 output_5;
    (&output_5)->position_2 = (((float4(world_2, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_34)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_5)->world_position_0 = world_2;

#line 1873
    float3 _S212;
    if((_S206.flags_0.y) == 1U)
    {

#line 1874
        _S212 = float3(0.0f, 1.0f, 0.0f);

#line 1874
    }
    else
    {

#line 1874
        _S212 = (float4(blade_4.ground_0) ).xyz;

#line 1874
    }

#line 1874
    (&output_5)->normal_2 = _S212;

#line 1879
    (&output_5)->color_1 = mix(_S206.root_color_0.xyz, _S206.tip_color_0.xyz, float3(_S210) ) * float3((1.0f - 0.18000000715255737f * _S208.w)) ;
    (&output_5)->uv_0 = GRASS_CARD_CORNERS_0[_S207];

#line 1885
    (&output_5)->level_1 = grassCardLevel_0(2.0f * _S209 * (&kernelContext_34)->grass_0->screen_0.x / max(abs((&output_5)->position_2.w), 0.00009999999747379f));

    (&output_5)->row_2 = min(_S205, max(grass_4->limits_0.y, 1U) - 1U);
    (&output_5)->patch_1 = grass_patch_0(_S204.z);
    GrassVertex_0 _S213 = output_5;

#line 1889
    thread vertexMain_Result_0 _S214;

#line 1889
    (&_S214)->position_7 = _S213.position_2;

#line 1889
    (&_S214)->world_position_11 = _S213.world_position_0;

#line 1889
    (&_S214)->normal_10 = _S213.normal_2;

#line 1889
    (&_S214)->color_7 = _S213.color_1;

#line 1889
    (&_S214)->uv_3 = _S213.uv_0;

#line 1889
    (&_S214)->level_4 = _S213.level_1;

#line 1889
    (&_S214)->row_6 = _S213.row_2;

#line 1889
    (&_S214)->patch_5 = _S213.patch_1;

#line 1889
    return _S214;
}


#line 1889
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


#line 1889
[[vertex]] bladeNearVertexMain_Result_0 bladeNearVertexMain(uint index_6 [[vertex_id]], uint instance_id_1 [[instance_id]], uint base_instance_1 [[base_instance]], GrassTile_0 constant* tile_13 [[buffer(2)]], GrassParams_0 constant* grass_5 [[buffer(1)]], GrassInstance_natural_0 device* instances_5 [[buffer(3)]], GrassBlade_natural_0 device* blades_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], GrassField_0 constant* field_5 [[buffer(7)]], uint device* cluster_lights_5 [[buffer(6)]], GpuLight_natural_0 device* lights_5 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_5 [[texture(0)]], sampler grassCardSampler_5 [[sampler(0)]], texture2d<float, access::sample> grassGround_5 [[texture(2)]], WindParams_0 constant* wind_5 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_5 [[texture(3)]], sampler windSampler_5 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_5 [[texture(4)]], GrassInstance_natural_0 device* grassCells_5 [[buffer(8)]])
{

#line 1889
    thread KernelContext_0 kernelContext_35;

#line 1889
    (&kernelContext_35)->tile_1 = tile_13;

#line 1889
    (&kernelContext_35)->grass_0 = grass_5;

#line 1889
    (&kernelContext_35)->instances_0 = instances_5;

#line 1889
    (&kernelContext_35)->blades_1 = blades_6;

#line 1889
    (&kernelContext_35)->frame_0 = frame_5;

#line 1889
    (&kernelContext_35)->field_0 = field_5;

#line 1889
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_5;

#line 1889
    (&kernelContext_35)->lights_0 = lights_5;

#line 1889
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_5;

#line 1889
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_5;

#line 1889
    (&kernelContext_35)->grassCard_0 = grassCard_5;

#line 1889
    (&kernelContext_35)->grassCardSampler_0 = grassCardSampler_5;

#line 1889
    (&kernelContext_35)->grassGround_0 = grassGround_5;

#line 1889
    (&kernelContext_35)->wind_0 = wind_5;

#line 1889
    (&kernelContext_35)->windDirectionLayer_0 = windDirectionLayer_5;

#line 1889
    (&kernelContext_35)->windSampler_0 = windSampler_5;

#line 1889
    (&kernelContext_35)->windIntensityLayer_0 = windIntensityLayer_5;

#line 1889
    (&kernelContext_35)->grassCells_0 = grassCells_5;

#line 1899
    GrassInstance_natural_0 blade_5 = instances_5[(field_5->tiles_0.x * field_5->tiles_0.y + tile_13->slot_0.x) * grass_5->limits_0.x + (instance_id_1 - base_instance_1)];

#line 1899
    thread GrassInstance_natural_0 _S215 = blade_5;

#line 1899
    float2 _S216 = grass_blade_lod_0(length((float4((&_S215)->root_0) ).xyz - frame_5->camera_position_0.xyz), &kernelContext_35);

#line 1899
    _S215 = blade_5;

#line 1899
    GrassVertex_0 _S217 = grass_blade_vertex_0(&_S215, index_6, 7U, _S216, &kernelContext_35);

#line 1899
    thread bladeNearVertexMain_Result_0 _S218;

#line 1899
    (&_S218)->position_8 = _S217.position_2;

#line 1899
    (&_S218)->world_position_12 = _S217.world_position_0;

#line 1899
    (&_S218)->normal_11 = _S217.normal_2;

#line 1899
    (&_S218)->color_8 = _S217.color_1;

#line 1899
    (&_S218)->uv_4 = _S217.uv_0;

#line 1899
    (&_S218)->level_5 = _S217.level_1;

#line 1899
    (&_S218)->row_7 = _S217.row_2;

#line 1899
    (&_S218)->patch_6 = _S217.patch_1;

#line 1899
    return _S218;
}


#line 1899
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


#line 1899
[[vertex]] bladeFarVertexMain_Result_0 bladeFarVertexMain(uint index_7 [[vertex_id]], uint instance_id_2 [[instance_id]], uint base_instance_2 [[base_instance]], GrassTile_0 constant* tile_14 [[buffer(2)]], GrassParams_0 constant* grass_6 [[buffer(1)]], GrassInstance_natural_0 device* instances_6 [[buffer(3)]], GrassBlade_natural_0 device* blades_7 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_6 [[buffer(0)]], GrassField_0 constant* field_6 [[buffer(7)]], uint device* cluster_lights_6 [[buffer(6)]], GpuLight_natural_0 device* lights_6 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_6 [[texture(0)]], sampler grassCardSampler_6 [[sampler(0)]], texture2d<float, access::sample> grassGround_6 [[texture(2)]], WindParams_0 constant* wind_6 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_6 [[texture(3)]], sampler windSampler_6 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_6 [[texture(4)]], GrassInstance_natural_0 device* grassCells_6 [[buffer(8)]])
{

#line 1899
    thread KernelContext_0 kernelContext_36;

#line 1899
    (&kernelContext_36)->tile_1 = tile_14;

#line 1899
    (&kernelContext_36)->grass_0 = grass_6;

#line 1899
    (&kernelContext_36)->instances_0 = instances_6;

#line 1899
    (&kernelContext_36)->blades_1 = blades_7;

#line 1899
    (&kernelContext_36)->frame_0 = frame_6;

#line 1899
    (&kernelContext_36)->field_0 = field_6;

#line 1899
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_6;

#line 1899
    (&kernelContext_36)->lights_0 = lights_6;

#line 1899
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_6;

#line 1899
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_6;

#line 1899
    (&kernelContext_36)->grassCard_0 = grassCard_6;

#line 1899
    (&kernelContext_36)->grassCardSampler_0 = grassCardSampler_6;

#line 1899
    (&kernelContext_36)->grassGround_0 = grassGround_6;

#line 1899
    (&kernelContext_36)->wind_0 = wind_6;

#line 1899
    (&kernelContext_36)->windDirectionLayer_0 = windDirectionLayer_6;

#line 1899
    (&kernelContext_36)->windSampler_0 = windSampler_6;

#line 1899
    (&kernelContext_36)->windIntensityLayer_0 = windIntensityLayer_6;

#line 1899
    (&kernelContext_36)->grassCells_0 = grassCells_6;

#line 1910
    uint capacity_0 = grass_6->limits_0.x;

    float2 _S219 = float2(1.0f, 1.0f);

#line 1912
    thread GrassInstance_natural_0 _S220 = instances_6[tile_14->slot_0.x * capacity_0 + capacity_0 - 1U - (instance_id_2 - base_instance_2)];

#line 1912
    GrassVertex_0 _S221 = grass_blade_vertex_0(&_S220, index_7, 3U, _S219, &kernelContext_36);

#line 1912
    thread bladeFarVertexMain_Result_0 _S222;

#line 1912
    (&_S222)->position_9 = _S221.position_2;

#line 1912
    (&_S222)->world_position_13 = _S221.world_position_0;

#line 1912
    (&_S222)->normal_12 = _S221.normal_2;

#line 1912
    (&_S222)->color_9 = _S221.color_1;

#line 1912
    (&_S222)->uv_5 = _S221.uv_0;

#line 1912
    (&_S222)->level_6 = _S221.level_1;

#line 1912
    (&_S222)->row_8 = _S221.row_2;

#line 1912
    (&_S222)->patch_7 = _S221.patch_1;

#line 1912
    return _S222;
}


#line 1912
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


#line 1912
[[vertex]] shellVertexMain_Result_0 shellVertexMain(uint index_8 [[vertex_id]], uint instance_id_3 [[instance_id]], uint base_instance_3 [[base_instance]], GrassTile_0 constant* tile_15 [[buffer(2)]], GrassParams_0 constant* grass_7 [[buffer(1)]], GrassInstance_natural_0 device* instances_7 [[buffer(3)]], GrassBlade_natural_0 device* blades_8 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], GrassField_0 constant* field_7 [[buffer(7)]], uint device* cluster_lights_7 [[buffer(6)]], GpuLight_natural_0 device* lights_7 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_7 [[texture(1)]], sampler shadow_sampler_7 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_7 [[texture(0)]], sampler grassCardSampler_7 [[sampler(0)]], texture2d<float, access::sample> grassGround_7 [[texture(2)]], WindParams_0 constant* wind_7 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_7 [[texture(3)]], sampler windSampler_7 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_7 [[texture(4)]], GrassInstance_natural_0 device* grassCells_7 [[buffer(8)]])
{

#line 1912
    thread KernelContext_0 kernelContext_37;

#line 1912
    (&kernelContext_37)->tile_1 = tile_15;

#line 1912
    (&kernelContext_37)->grass_0 = grass_7;

#line 1912
    (&kernelContext_37)->instances_0 = instances_7;

#line 1912
    (&kernelContext_37)->blades_1 = blades_8;

#line 1912
    (&kernelContext_37)->frame_0 = frame_7;

#line 1912
    (&kernelContext_37)->field_0 = field_7;

#line 1912
    (&kernelContext_37)->cluster_lights_0 = cluster_lights_7;

#line 1912
    (&kernelContext_37)->lights_0 = lights_7;

#line 1912
    (&kernelContext_37)->shadow_atlas_0 = shadow_atlas_7;

#line 1912
    (&kernelContext_37)->shadow_sampler_0 = shadow_sampler_7;

#line 1912
    (&kernelContext_37)->grassCard_0 = grassCard_7;

#line 1912
    (&kernelContext_37)->grassCardSampler_0 = grassCardSampler_7;

#line 1912
    (&kernelContext_37)->grassGround_0 = grassGround_7;

#line 1912
    (&kernelContext_37)->wind_0 = wind_7;

#line 1912
    (&kernelContext_37)->windDirectionLayer_0 = windDirectionLayer_7;

#line 1912
    (&kernelContext_37)->windSampler_0 = windSampler_7;

#line 1912
    (&kernelContext_37)->windIntensityLayer_0 = windIntensityLayer_7;

#line 1912
    (&kernelContext_37)->grassCells_0 = grassCells_7;

#line 1912
    uint _S223 = instance_id_3 - base_instance_3;

#line 1912
    uint _S224 = grass_shell_count_0(&kernelContext_37);

#line 2008
    uint _S225 = _S224 - 1U;
    uint quad_0 = index_8 / 6U;

#line 2009
    GrassShellVertex_0 _S226 = grass_sheet_vertex_0((&kernelContext_37)->tile_1->tile_0.xy + (float2(float(quad_0 % 16U), float(quad_0 / 16U)) + GRASS_CARD_CORNERS_0[index_8 % 6U]) * float2(((&kernelContext_37)->tile_1->tile_0.z / 16.0f)) , (&kernelContext_37)->field_0->layers_0[_S225 - min(_S223, _S225)].x, float4(0.0f, 1.0f, 0.0f, 0.0f), (&kernelContext_37)->field_0->layers_0[_S225 - min(_S223, _S225)].y, &kernelContext_37);

#line 2009
    thread shellVertexMain_Result_0 _S227;

#line 2009
    (&_S227)->position_10 = _S226.position_5;

#line 2009
    (&_S227)->world_position_14 = _S226.world_position_9;

#line 2009
    (&_S227)->rest_3 = _S226.rest_0;

#line 2009
    (&_S227)->normal_13 = _S226.normal_7;

#line 2009
    (&_S227)->footprint_2 = _S226.footprint_0;

#line 2009
    (&_S227)->occlusion_5 = _S226.occlusion_1;

#line 2009
    (&_S227)->fin_3 = _S226.fin_0;

#line 2009
    return _S227;
}


#line 2009
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


#line 2009
[[vertex]] finVertexMain_Result_0 finVertexMain(uint index_9 [[vertex_id]], GrassTile_0 constant* tile_16 [[buffer(2)]], GrassParams_0 constant* grass_8 [[buffer(1)]], GrassInstance_natural_0 device* instances_8 [[buffer(3)]], GrassBlade_natural_0 device* blades_9 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_8 [[buffer(0)]], GrassField_0 constant* field_8 [[buffer(7)]], uint device* cluster_lights_8 [[buffer(6)]], GpuLight_natural_0 device* lights_8 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_8 [[texture(1)]], sampler shadow_sampler_8 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_8 [[texture(0)]], sampler grassCardSampler_8 [[sampler(0)]], texture2d<float, access::sample> grassGround_8 [[texture(2)]], WindParams_0 constant* wind_8 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_8 [[texture(3)]], sampler windSampler_8 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_8 [[texture(4)]], GrassInstance_natural_0 device* grassCells_8 [[buffer(8)]])
{

#line 2009
    thread KernelContext_0 kernelContext_38;

#line 2009
    (&kernelContext_38)->tile_1 = tile_16;

#line 2009
    (&kernelContext_38)->grass_0 = grass_8;

#line 2009
    (&kernelContext_38)->instances_0 = instances_8;

#line 2009
    (&kernelContext_38)->blades_1 = blades_9;

#line 2009
    (&kernelContext_38)->frame_0 = frame_8;

#line 2009
    (&kernelContext_38)->field_0 = field_8;

#line 2009
    (&kernelContext_38)->cluster_lights_0 = cluster_lights_8;

#line 2009
    (&kernelContext_38)->lights_0 = lights_8;

#line 2009
    (&kernelContext_38)->shadow_atlas_0 = shadow_atlas_8;

#line 2009
    (&kernelContext_38)->shadow_sampler_0 = shadow_sampler_8;

#line 2009
    (&kernelContext_38)->grassCard_0 = grassCard_8;

#line 2009
    (&kernelContext_38)->grassCardSampler_0 = grassCardSampler_8;

#line 2009
    (&kernelContext_38)->grassGround_0 = grassGround_8;

#line 2009
    (&kernelContext_38)->wind_0 = wind_8;

#line 2009
    (&kernelContext_38)->windDirectionLayer_0 = windDirectionLayer_8;

#line 2009
    (&kernelContext_38)->windSampler_0 = windSampler_8;

#line 2009
    (&kernelContext_38)->windIntensityLayer_0 = windIntensityLayer_8;

#line 2009
    (&kernelContext_38)->grassCells_0 = grassCells_8;

#line 2046
    uint quad_1 = index_9 / 6U;
    uint _S228 = index_9 % 6U;
    uint _S229 = max(field_8->tiles_0.z / 4U, 1U);

    uint _S230 = quad_1 / (_S229 * 64U);

#line 2050
    uint _S231 = min(_S230, 1U);
    uint fin_line_0 = quad_1 / 64U % _S229;



    float band_4 = 4.0f * (&kernelContext_38)->field_0->origin_0.w;
    float across_1 = (float(fin_line_0) + 0.5f) * band_4;
    float stride_0 = (&kernelContext_38)->tile_1->tile_0.z / 16.0f;
    float _S232 = float(quad_1 / 4U % 16U);

#line 2058
    float along_4 = (_S232 + GRASS_CARD_CORNERS_0[_S228].x) * stride_0;
    float middle_along_0 = (_S232 + 0.5f) * stride_0;
    float share_2 = (float(quad_1 % 4U) + GRASS_CARD_CORNERS_0[_S228].y) / 4.0f;



    bool along_z_1 = _S231 == 0U;
    float2 _S233 = (&kernelContext_38)->tile_1->tile_0.xy;

#line 2065
    float2 side_2;

#line 2065
    if(along_z_1)
    {

#line 2065
        side_2 = float2(across_1, along_4);

#line 2065
    }
    else
    {

#line 2065
        side_2 = float2(along_4, across_1);

#line 2065
    }

#line 2065
    float2 rest_5 = _S233 + side_2;

    float2 _S234 = (&kernelContext_38)->tile_1->tile_0.xy;

#line 2067
    if(along_z_1)
    {

#line 2067
        side_2 = float2(across_1, middle_along_0);

#line 2067
    }
    else
    {

#line 2067
        side_2 = float2(middle_along_0, across_1);

#line 2067
    }

#line 2067
    float2 middle_0 = _S234 + side_2;
    if(along_z_1)
    {

#line 2068
        side_2 = float2(0.5f * band_4, 0.0f);

#line 2068
    }
    else
    {

#line 2068
        side_2 = float2(0.0f, 0.5f * band_4);

#line 2068
    }

#line 2068
    GrassGround_0 _S235 = grass_ground_under_0(middle_0, &kernelContext_38);

#line 2068
    GrassGround_0 _S236 = grass_ground_under_0(middle_0 - side_2, &kernelContext_38);

#line 2068
    GrassGround_0 _S237 = grass_ground_under_0(middle_0 + side_2, &kernelContext_38);

#line 2074
    float3 eye_0 = (&kernelContext_38)->frame_0->camera_position_0.xyz - float3(middle_0.x, _S235.height_1 + 0.5f * (&kernelContext_38)->field_0->stack_0.x, middle_0.y);
    float3 view_1 = eye_0 / float3(max(length(eye_0), 9.99999997475242708e-07f)) ;
    float facing_near_0 = dot(_S236.normal_6, view_1);
    float facing_far_0 = dot(_S237.normal_6, view_1);

#line 2077
    float graze_0;

    if((facing_near_0 * facing_far_0) <= 0.0f)
    {

#line 2079
        graze_0 = 0.0f;

#line 2079
    }
    else
    {

#line 2079
        graze_0 = min(abs(facing_near_0), abs(facing_far_0));

#line 2079
    }

    float fade_2 = saturate((0.5f - graze_0) / 0.19999998807907104f);


    if(along_z_1)
    {

#line 2084
        graze_0 = (&kernelContext_38)->tile_1->tile_0.x;

#line 2084
    }
    else
    {

#line 2084
        graze_0 = (&kernelContext_38)->tile_1->tile_0.y;

#line 2084
    }

    float4 _S238 = float4(float(_S231) + 1.0f, fade_2, graze_0 + across_1, 0.0f);

#line 2086
    float _S239 = grass_occlusion_at_0(share_2, &kernelContext_38);

#line 2086
    GrassShellVertex_0 _S240 = grass_sheet_vertex_0(rest_5, share_2, _S238, _S239, &kernelContext_38);

#line 2085
    thread GrassShellVertex_0 output_6 = _S240;

#line 2085
    bool _S241;

    if(fade_2 <= 0.0f)
    {

#line 2087
        _S241 = true;

#line 2087
    }
    else
    {

#line 2087
        _S241 = ((&kernelContext_38)->field_0->stack_0.z) <= 0.0f;

#line 2087
    }

#line 2087
    if(_S241)
    {


        (&output_6)->position_5 = float4(0.0f, 0.0f, -1.0f, 1.0f);

#line 2087
    }

#line 2093
    GrassShellVertex_0 _S242 = output_6;

#line 2093
    thread finVertexMain_Result_0 _S243;

#line 2093
    (&_S243)->position_11 = _S242.position_5;

#line 2093
    (&_S243)->world_position_15 = _S242.world_position_9;

#line 2093
    (&_S243)->rest_4 = _S242.rest_0;

#line 2093
    (&_S243)->normal_14 = _S242.normal_7;

#line 2093
    (&_S243)->footprint_3 = _S242.footprint_0;

#line 2093
    (&_S243)->occlusion_6 = _S242.occlusion_1;

#line 2093
    (&_S243)->fin_4 = _S242.fin_0;

#line 2093
    return _S243;
}

