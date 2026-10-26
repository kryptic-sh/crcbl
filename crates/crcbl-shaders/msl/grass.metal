#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 711 "shaders/grass.slang"
constant array<float2, int(32)> SHADOW_DISC_0 = { { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) } };

#line 732
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { { 0U, 23U, 25U, 27U, 29U } };

#line 757
constant array<float2, int(16)> SHADOW_SEARCH_OFFSET_0 = { { float2(1.41421604156494141f, 0.0f), float2(-1.80617594718933105f, 1.65460801124572754f), float2(0.27646398544311523f, -3.15016794204711914f), float2(2.27656793594360352f, 2.96938395500183105f), float2(-4.17778396606445312f, -0.73899197578430176f), float2(3.95756006240844727f, -2.51747989654541016f), float2(-1.32372796535491943f, 4.92420005798339844f), float2(-2.52448797225952148f, -4.86075210571289062f), float2(5.47713613510131836f, 2.00024008750915527f), float2(-5.69804811477661133f, 2.35207200050354004f), float2(2.74683189392089844f, -5.86983203887939453f), float2(2.02983999252319336f, 6.47145605087280273f), float2(-6.11796808242797852f, -3.54548811912536621f), float2(7.17707204818725586f, -1.57785594463348389f), float2(-4.38005590438842773f, 6.23017597198486328f), float2(-1.01189601421356201f, -7.80872011184692383f) } };

#line 734
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) } };

#line 745
constant array<uint, int(16)> SHADOW_DITHER_0 = { { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U } };

#line 899
constant array<float2, int(6)> GRASS_CARD_CORNERS_0 = { { float2(0.0f, 0.0f), float2(1.0f, 0.0f), float2(0.0f, 1.0f), float2(1.0f, 0.0f), float2(1.0f, 1.0f), float2(0.0f, 1.0f) } };

#line 507
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


#line 492
struct GrassTile_0
{
    float4 tile_0;
    uint4 slot_0;
};


#line 477
struct GrassParams_0
{
    uint4 limits_0;
    float4 screen_0;
};


#line 581
struct GrassInstance_natural_0
{
    packed_float4 root_0;
    packed_float4 facing_0;
    packed_float4 lean_0;
    packed_float4 ground_0;
    packed_float4 clump_1;
    packed_uint4 lanes_0;
};


#line 585
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


#line 569
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<float4, int(4)> data_0;
};


#line 569
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(2)> data_1;
};


#line 569
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_0, int(14)> data_2;
};


#line 172
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


#line 626
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


#line 617
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


#line 666
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


#line 657
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


#line 1548
GrassBlade_0 grass_row_0(uint index_0, KernelContext_0 thread* kernelContext_0)
{
    GrassBlade_natural_0 _S1 = kernelContext_0->blades_1[min(index_0, max(kernelContext_0->grass_0->limits_0.y, 1U) - 1U)];

#line 1550
    GrassBlade_0 _S2 = { float4(_S1.root_color_0) , float4(_S1.tip_color_0) , float4(_S1.size_0) , float4(_S1.occlusion_0) , float4(_S1.glow_0) , float4(_S1.patch_0) , float4(_S1.shape_0) , float4(_S1.clump_0) , uint4(_S1.flags_0)  };

#line 1550
    return _S2;
}


#line 829
float grassCardLevel_0(float pixels_0)
{
    float _S3 = 64.0f / max(pixels_0, 0.00009999999747379f);

#line 831
    float level_0 = 0.0f;

#line 831
    uint step_0 = 1U;

    for(;;)
    {

#line 833
        if(step_0 < 7U)
        {
        }
        else
        {

#line 833
            break;
        }
        uint _S4 = step_0 - 1U;

#line 835
        float lower_0 = float(1U << _S4);
        if(_S3 >= lower_0)
        {

#line 836
            level_0 = float(_S4) + min((_S3 - lower_0) / lower_0, 1.0f);

#line 836
        }

#line 833
        step_0 = step_0 + 1U;

#line 833
    }

#line 849
    return min(floor(level_0 + 0.5f), 6.0f);
}


#line 1487
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


#line 1542
float grass_patch_0(uint clump_2)
{
    return grass_unit_pair_0(grass_hash_0(clump_2 ^ 374761393U)).x;
}


#line 1787
float2 grass_blade_lod_0(float distance_0, KernelContext_0 thread* kernelContext_1)
{
    float band_0 = kernelContext_1->field_0->blades_0.y;
    if(!(band_0 > 0.0f))
    {
        return float2(0.0f, 0.0f);
    }
    float half_band_0 = 0.5f * band_0;

    float _S5 = distance_0 - (kernelContext_1->field_0->blades_0.x - band_0);

#line 1796
    return float2(saturate(_S5 / half_band_0), saturate((_S5 - half_band_0) / half_band_0));
}


#line 1690
struct GrassBladeCurve_0
{
    float3 p0_0;
    float3 p1_0;
    float3 p2_0;
    float3 p3_0;
};


#line 1708
GrassBladeCurve_0 grass_blade_curve_0(const GrassInstance_natural_0 thread* blade_0, const GrassBlade_0 thread* row_0)
{

#line 1708
    float4 _S6 = float4(blade_0->root_0) ;

    float height_0 = _S6.w;

#line 1710
    float4 _S7 = float4(blade_0->facing_0) ;
    float _S8 = _S7.x;

#line 1711
    float _S9 = _S7.y;
    float tilt_0 = row_0->shape_0.x;
    float upright_0 = sqrt(max(1.0f - tilt_0 * tilt_0, 0.0f));
    float3 chord_0 = float3(0.0f, height_0 * upright_0, 0.0f) + float3(_S8, 0.0f, _S9) * float3((height_0 * tilt_0)) ;



    float3 bow_0 = float3(_S8 * upright_0, - tilt_0, _S9 * upright_0) * float3((row_0->shape_0.y * height_0)) ;

    thread GrassBladeCurve_0 curve_0;
    float3 _S10 = _S6.xyz;

#line 1721
    (&curve_0)->p0_0 = _S10;

#line 1721
    float3 _S11 = float3(0.3333333432674408f) ;
    (&curve_0)->p1_0 = _S10 + chord_0 * _S11 - bow_0;
    float3 _S12 = (float4(blade_0->lean_0) ).xyz;

#line 1723
    (&curve_0)->p2_0 = _S10 + chord_0 * float3(0.66666668653488159f)  - bow_0 + _S12 * _S11;
    (&curve_0)->p3_0 = _S10 + chord_0 + _S12;
    return curve_0;
}


struct GrassBladePoint_0
{
    float3 position_1;
    float3 normal_0;
};


#line 1745
GrassBladePoint_0 grass_blade_point_0(const GrassBladeCurve_0 thread* curve_1, const GrassInstance_natural_0 thread* blade_1, const GrassBlade_0 thread* row_1, float t_0, float edge_0, float scale_0, KernelContext_0 thread* kernelContext_2)
{

    float u_0 = 1.0f - t_0;
    float _S13 = 3.0f * u_0;

#line 1749
    float _S14 = _S13 * u_0;
    float _S15 = t_0 * t_0;

#line 1750
    float3 centre_0 = curve_1->p0_0 * float3((u_0 * u_0 * u_0))  + curve_1->p1_0 * float3((_S14 * t_0))  + curve_1->p2_0 * float3((_S13 * t_0 * t_0))  + curve_1->p3_0 * float3((_S15 * t_0)) ;

    float3 tangent_1 = (curve_1->p1_0 - curve_1->p0_0) * float3(_S14)  + (curve_1->p2_0 - curve_1->p1_0) * float3((6.0f * u_0 * t_0))  + (curve_1->p3_0 - curve_1->p2_0) * float3((3.0f * t_0 * t_0)) ;

#line 1752
    float4 _S16 = float4(blade_1->facing_0) ;
    float3 side_0 = float3(- _S16.y, 0.0f, _S16.x);

    float3 to_eye_0 = kernelContext_2->frame_0->camera_position_0.xyz - centre_0;
    float3 view_0 = to_eye_0 / float3(max(length(to_eye_0), 9.99999997475242708e-07f)) ;
    float3 perpendicular_0 = cross(tangent_1, view_0);
    float perpendicular_length_0 = length(perpendicular_0);

#line 1758
    float3 screen_side_0;
    if(perpendicular_length_0 > 9.99999997475242708e-07f)
    {

#line 1759
        screen_side_0 = perpendicular_0 / float3(perpendicular_length_0) ;

#line 1759
    }
    else
    {

#line 1759
        screen_side_0 = side_0;

#line 1759
    }
    if((dot(screen_side_0, side_0)) < 0.0f)
    {

#line 1760
        screen_side_0 = - screen_side_0;

#line 1760
    }
    float along_0 = dot(side_0, view_0);
    float side_seen_0 = sqrt(saturate(1.0f - along_0 * along_0));
    float3 direction_1 = side_0 * float3(side_seen_0)  + screen_side_0 * float3((1.0f - side_seen_0)) ;
    float seen_0 = length(direction_1 - view_0 * float3(dot(direction_1, view_0)) );

#line 1769
    float half_width_0 = _S16.z * (1.0f - _S15);
    float least_0 = 0.5f * (max(abs((((float4(centre_0, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_2->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_2->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w), 0.00009999999747379f) / max(kernelContext_2->grass_0->screen_0.x, 0.00009999999747379f));

#line 1770
    float widened_0;
    if((half_width_0 * seen_0) < least_0)
    {

#line 1771
        widened_0 = least_0 / max(seen_0, 0.00009999999747379f);

#line 1771
    }
    else
    {

#line 1771
        widened_0 = half_width_0;

#line 1771
    }

    thread GrassBladePoint_0 here_0;
    (&here_0)->position_1 = centre_0 + direction_1 * float3((edge_0 * widened_0 * scale_0)) ;


    float3 normal_1 = cross(tangent_1, side_0);

    (&here_0)->normal_0 = normalize(normal_1 / float3(max(length(normal_1), 9.99999997475242708e-07f))  + side_0 * float3((edge_0 * row_1->shape_0.z)) );
    return here_0;
}


#line 1665
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


#line 1809
GrassVertex_0 grass_blade_vertex_0(const GrassInstance_natural_0 thread* blade_2, uint index_1, uint segments_0, float2 lod_0, KernelContext_0 thread* kernelContext_3)
{

#line 1809
    uint4 _S17 = uint4(blade_2->lanes_0) ;

    uint _S18 = _S17.y;

#line 1811
    GrassBlade_0 _S19 = grass_row_0(_S18, kernelContext_3);

#line 1811
    thread GrassBlade_0 _S20 = _S19;

#line 1811
    GrassBladeCurve_0 _S21 = grass_blade_curve_0(blade_2, &_S20);


    float drop_0 = lod_0.x;
    float morph_0 = lod_0.y;

#line 1815
    float scale_1;


    if((_S17.w) != 0U)
    {

#line 1818
        scale_1 = 1.0f + drop_0;

#line 1818
    }
    else
    {

#line 1818
        scale_1 = 1.0f - drop_0;

#line 1818
    }

    bool tip_0 = index_1 >= (2U * segments_0);
    uint _S22 = min(index_1 / 2U, segments_0);

#line 1821
    float edge_1;
    if(tip_0)
    {

#line 1822
        edge_1 = 0.0f;

#line 1822
    }
    else
    {

#line 1822
        if((index_1 % 2U) == 0U)
        {

#line 1822
            edge_1 = -1.0f;

#line 1822
        }
        else
        {

#line 1822
            edge_1 = 1.0f;

#line 1822
        }

#line 1822
    }

#line 1822
    float t_1;
    if(tip_0)
    {

#line 1823
        t_1 = 1.0f;

#line 1823
    }
    else
    {

#line 1823
        t_1 = float(_S22) / float(segments_0);

#line 1823
    }

#line 1823
    thread GrassBladeCurve_0 _S23 = _S21;

#line 1823
    thread GrassBlade_0 _S24 = _S19;

#line 1823
    GrassBladePoint_0 _S25 = grass_blade_point_0(&_S23, blade_2, &_S24, t_1, edge_1, scale_1, kernelContext_3);
    thread GrassBladePoint_0 here_1 = _S25;

#line 1824
    bool _S26;

    if(morph_0 > 0.0f)
    {

#line 1826
        _S26 = !tip_0;

#line 1826
    }
    else
    {

#line 1826
        _S26 = false;

#line 1826
    }

#line 1826
    if(_S26)
    {

#line 1826
        _S26 = segments_0 != 3U;

#line 1826
    }
    else
    {

#line 1826
        _S26 = false;

#line 1826
    }

#line 1826
    if(_S26)
    {
        uint scaled_0 = _S22 * 3U;
        uint lower_1 = scaled_0 / segments_0;
        float within_0 = float(scaled_0 - lower_1 * segments_0) / float(segments_0);

        float _S27 = float(lower_1) / 3.0f;

#line 1832
        thread GrassBladeCurve_0 _S28 = _S21;

#line 1832
        thread GrassBlade_0 _S29 = _S19;

#line 1832
        GrassBladePoint_0 _S30 = grass_blade_point_0(&_S28, blade_2, &_S29, _S27, edge_1, scale_1, kernelContext_3);

        float _S31 = float(lower_1 + 1U) / 3.0f;

#line 1834
        thread GrassBladeCurve_0 _S32 = _S21;

#line 1834
        thread GrassBlade_0 _S33 = _S19;

#line 1834
        GrassBladePoint_0 _S34 = grass_blade_point_0(&_S32, blade_2, &_S33, _S31, edge_1, scale_1, kernelContext_3);

#line 1834
        float3 _S35 = float3((1.0f - within_0)) ;

#line 1834
        float3 _S36 = float3(within_0) ;

        float3 far_normal_0 = _S30.normal_0 * _S35 + _S34.normal_0 * _S36;

#line 1836
        float3 _S37 = float3((1.0f - morph_0)) ;

#line 1836
        float3 _S38 = float3(morph_0) ;
        (&here_1)->position_1 = (&here_1)->position_1 * _S37 + (_S30.position_1 * _S35 + _S34.position_1 * _S36) * _S38;
        (&here_1)->normal_0 = (&here_1)->normal_0 * _S37 + far_normal_0 * _S38;

#line 1826
    }

#line 1826
    float3 blade_normal_0;

#line 1843
    if((dot((&here_1)->normal_0, kernelContext_3->frame_0->camera_position_0.xyz - (&here_1)->position_1)) < 0.0f)
    {

#line 1843
        blade_normal_0 = - (&here_1)->normal_0;

#line 1843
    }
    else
    {

#line 1843
        blade_normal_0 = (&here_1)->normal_0;

#line 1843
    }

#line 1843
    float3 base_0;



    if((_S19.flags_0.y) == 1U)
    {

#line 1847
        base_0 = float3(0.0f, 1.0f, 0.0f);

#line 1847
    }
    else
    {

#line 1847
        base_0 = (float4(blade_2->ground_0) ).xyz;

#line 1847
    }

#line 1847
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


#line 1560
float3 grass_style_0(float3 color_2, const GrassBlade_0 thread* row_3, float along_1, float patch_2)
{

#line 1560
    float4 _S40 = row_3->occlusion_0;

    float reach_0 = row_3->occlusion_0.w;

#line 1562
    float occluded_0;
    if(reach_0 > 0.0f)
    {

#line 1563
        occluded_0 = saturate(1.0f - along_1 / reach_0);

#line 1563
    }
    else
    {

#line 1563
        occluded_0 = 0.0f;

#line 1563
    }


    float start_0 = row_3->glow_0.w;

    return mix(mix(color_2, _S40.xyz, float3(occluded_0) ), row_3->patch_0.xyz, float3((row_3->patch_0.w * patch_2)) ) + row_3->glow_0.xyz * float3(saturate((along_1 - start_0) / max(1.0f - start_0, 0.00009999999747379f))) ;
}


#line 1414
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_4)
{
    uint _S41 = max(kernelContext_4->frame_0->cluster_grid_0.x, 1U);
    uint _S42 = max(kernelContext_4->frame_0->cluster_grid_0.y, 1U);
    uint _S43 = max(kernelContext_4->frame_0->cluster_grid_0.z, 1U);
    uint _S44 = max(kernelContext_4->frame_0->cluster_grid_0.w, 1U);

#line 1424
    uint _S45 = uint(pixel_0.x) / _S44;

#line 1424
    uint _S46 = min(_S45, _S41 - 1U);
    uint _S47 = uint(pixel_0.y) / _S44;

    float scale_2 = 24.0f / log2(10000.0f);

#line 1435
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_2 + - scale_2 * log2(0.10000000149011612f)), 0.0f, float(_S43 - 1U))) * _S42 + min(_S47, _S42 - 1U)) * _S41 + _S46;
}


#line 1392
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

#line 1411
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 932
float4 atlas_rect_0(uint tile_2, KernelContext_0 thread* kernelContext_5)
{
    return kernelContext_5->frame_0->shadow_atlas_rect_0[tile_2];
}


#line 932
float4 atlas_rect_1(uint tile_3, KernelContext_0 thread* kernelContext_6)
{
    return kernelContext_6->frame_0->shadow_atlas_rect_0[tile_3];
}


#line 947
bool atlas_rect_is_empty_0(float4 rect_0)
{
    return !((rect_0.x) > 0.0f);
}


#line 942
float tile_texels_0(float4 rect_1, KernelContext_0 thread* kernelContext_7)
{
    return rect_1.x / kernelContext_7->frame_0->shadow_params_0.x;
}


#line 909
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 921
uint shadow_filter_mode_0(float2 pixel_1, KernelContext_0 thread* kernelContext_8)
{

#line 921
    uint _S48;

    if(uint(pixel_1.x) < (kernelContext_8->frame_0->shadow_filter_0.z))
    {

#line 923
        _S48 = kernelContext_8->frame_0->shadow_filter_0.x;

#line 923
    }
    else
    {

#line 923
        _S48 = kernelContext_8->frame_0->shadow_filter_0.y;

#line 923
    }

#line 923
    return _S48;
}


#line 937
float2 atlas_step_0(float4 rect_2, KernelContext_0 thread* kernelContext_9)
{
    return kernelContext_9->frame_0->shadow_params_0.xy / rect_2.xy;
}


#line 937
float2 atlas_step_1(float4 rect_3, KernelContext_0 thread* kernelContext_10)
{
    return kernelContext_10->frame_0->shadow_params_0.xy / rect_3.xy;
}


#line 927
float2 atlas_uv_0(float4 rect_4, float2 tile_uv_0)
{
    return rect_4.zw + tile_uv_0 * rect_4.xy;
}


#line 952
float tile_tap_0(float4 rect_5, float2 texel_step_0, float2 tile_uv_1, float2 spoke_0, float2 rotation_0, float reference_0, KernelContext_0 thread* kernelContext_11)
{

    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_step_0;

    float _S49 = spoke_0.x;

#line 957
    float _S50 = rotation_0.x;

#line 957
    float _S51 = spoke_0.y;

#line 957
    float _S52 = rotation_0.y;


    float _S53 = ((kernelContext_11->shadow_atlas_0).sample_compare((kernelContext_11->shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_1 + float2(_S49 * _S50 - _S51 * _S52, _S49 * _S52 + _S51 * _S50) * texel_step_0, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_0), level((0.0f))));

#line 960
    return _S53;
}


#line 952
float tile_tap_1(float4 rect_6, float2 texel_step_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_12)
{

    float2 tile_min_1 = float2(0.5f, 0.5f) * texel_step_1;

    float _S54 = spoke_1.x;

#line 957
    float _S55 = rotation_1.x;

#line 957
    float _S56 = spoke_1.y;

#line 957
    float _S57 = rotation_1.y;


    float _S58 = ((kernelContext_12->shadow_atlas_0).sample_compare((kernelContext_12->shadow_sampler_0), (atlas_uv_0(rect_6, clamp(tile_uv_2 + float2(_S54 * _S55 - _S56 * _S57, _S54 * _S57 + _S56 * _S55) * texel_step_1, tile_min_1, float2(1.0f)  - tile_min_1))), (reference_1), level((0.0f))));

#line 960
    return _S58;
}


#line 1023
float tile_box_pcf_0(uint tile_4, float2 tile_uv_3, float reference_2, KernelContext_0 thread* kernelContext_13)
{

#line 1023
    float4 _S59 = atlas_rect_1(tile_4, kernelContext_13);


    if(atlas_rect_is_empty_0(_S59))
    {
        return 1.0f;
    }

#line 1028
    float2 _S60 = atlas_step_1(_S59, kernelContext_13);

#line 1028
    int y_0 = int(-1);

#line 1028
    float visibility_0 = 0.0f;

#line 1033
    for(;;)
    {

#line 1033
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 1033
            break;
        }

#line 1033
        int x_0 = int(-1);

        for(;;)
        {

#line 1035
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 1035
                break;
            }

#line 1035
            float _S61 = tile_tap_1(_S59, _S60, tile_uv_3, float2(float(x_0), float(y_0)), float2(1.0f, 0.0f), reference_2, kernelContext_13);

            float visibility_1 = visibility_0 + _S61;

#line 1035
            x_0 = x_0 + int(1);

#line 1035
            visibility_0 = visibility_1;

#line 1035
        }

#line 1033
        y_0 = y_0 + int(1);

#line 1033
    }

#line 1041
    return visibility_0 / 9.0f;
}


#line 915
float2 shadow_rotation_0(float2 pixel_2)
{
    uint2 cell_0 = uint2(pixel_2) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 1044
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_4, float reference_3, float2 rotation_2, KernelContext_0 thread* kernelContext_14)
{
    float2 texel_0 = kernelContext_14->frame_0->shadow_params_0.xy;

#line 1046
    float4 _S62 = atlas_rect_0(cascade_0, kernelContext_14);

#line 1046
    float2 _S63 = atlas_step_0(_S62, kernelContext_14);


    float2 _S64 = float2(0.5f, 0.5f) * _S63;


    float2 _S65 = float2(1.0f, 1.0f);

#line 1052
    float2 _S66 = _S65 / texel_0;

#line 1052
    uint index_2 = 0U;

#line 1052
    float sum_0 = 0.0f;

#line 1052
    float found_0 = 0.0f;



    for(;;)
    {

#line 1056
        if(index_2 < 16U)
        {
        }
        else
        {

#line 1056
            break;
        }

        float _S67 = SHADOW_SEARCH_OFFSET_0[index_2].x;

#line 1059
        float _S68 = rotation_2.x;

#line 1059
        float _S69 = SHADOW_SEARCH_OFFSET_0[index_2].y;

#line 1059
        float _S70 = rotation_2.y;

#line 1067
        int3 _S71 = int3(int2(min(atlas_uv_0(_S62, clamp(tile_uv_4 + float2(_S67 * _S68 - _S69 * _S70, _S67 * _S70 + _S69 * _S68) * _S63, _S64, float2(1.0f)  - _S64)) * _S66, _S66 - _S65)), int(0));

#line 1067
        float depth_1 = ((kernelContext_14->shadow_atlas_0).read(vec<uint,2>(((_S71)).xy), uint(((_S71)).z)));
        if(depth_1 > reference_3)
        {

            float found_1 = found_0 + 1.0f;

#line 1071
            sum_0 = sum_0 + depth_1;

#line 1071
            found_0 = found_1;

#line 1068
        }

#line 1056
        index_2 = index_2 + 1U;

#line 1056
    }

#line 1075
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 1086
    float _S72 = 2.0f * kernelContext_14->frame_0->cascade_far_0[cascade_0];

#line 1086
    float separation_0 = (sum_0 / found_0 - reference_3) * (_S72 + 40.0f);

#line 1086
    float _S73 = tile_texels_0(_S62, kernelContext_14);

    return clamp(separation_0 * 0.01999999955296516f / (_S72 / _S73), 2.0f, 8.0f);
}


#line 963
float tile_pcf_0(uint tile_5, float2 tile_uv_5, float reference_4, float2 pixel_3, float radius_2, KernelContext_0 thread* kernelContext_15)
{
    float2 rotation_3 = shadow_rotation_0(pixel_3);

#line 965
    float4 _S74 = atlas_rect_0(tile_5, kernelContext_15);

    if(atlas_rect_is_empty_0(_S74))
    {
        return 1.0f;
    }

#line 969
    float2 _S75 = atlas_step_0(_S74, kernelContext_15);



    thread array<float, int(5)> probe_taps_0;

#line 973
    uint spot_0 = 0U;

#line 973
    float probe_0 = 0.0f;

    for(;;)
    {

#line 975
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 975
            break;
        }

#line 975
        float _S76 = tile_tap_0(_S74, _S75, tile_uv_5, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_2) , rotation_3, reference_4, kernelContext_15);

        probe_taps_0[spot_0] = _S76;


        float probe_1 = probe_0 + _S76;

#line 975
        spot_0 = spot_0 + 1U;

#line 975
        probe_0 = probe_1;

#line 975
    }

#line 986
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 992
    uint index_3 = 1U;

#line 992
    float visibility_2 = probe_taps_0[int(0)];

#line 1000
    for(;;)
    {

#line 1000
        if(index_3 < 23U)
        {
        }
        else
        {

#line 1000
            break;
        }

#line 1000
        float _S77 = tile_tap_0(_S74, _S75, tile_uv_5, SHADOW_DISC_0[index_3] * float2(radius_2) , rotation_3, reference_4, kernelContext_15);

        float visibility_3 = visibility_2 + _S77;

#line 1000
        index_3 = index_3 + 1U;

#line 1000
        visibility_2 = visibility_3;

#line 1000
    }

#line 1005
    float visibility_4 = visibility_2 + probe_taps_0[int(1)];

#line 1005
    float2 _S78 = float2(radius_2) ;

#line 1005
    float _S79 = tile_tap_0(_S74, _S75, tile_uv_5, float2(0.43503299355506897f, 0.75919097661972046f) * _S78, rotation_3, reference_4, kernelContext_15);


    float visibility_5 = visibility_4 + _S79 + probe_taps_0[int(2)];

#line 1008
    float _S80 = tile_tap_0(_S74, _S75, tile_uv_5, float2(0.82610201835632324f, -0.38168001174926758f) * _S78, rotation_3, reference_4, kernelContext_15);


    float visibility_6 = visibility_5 + _S80 + probe_taps_0[int(3)];

#line 1011
    float _S81 = tile_tap_0(_S74, _S75, tile_uv_5, float2(-0.31940698623657227f, -0.88803398609161377f) * _S78, rotation_3, reference_4, kernelContext_15);


    float visibility_7 = visibility_6 + _S81 + probe_taps_0[int(4)];

#line 1014
    index_3 = 30U;

#line 1014
    visibility_2 = visibility_7;
    for(;;)
    {

#line 1015
        if(index_3 < 32U)
        {
        }
        else
        {

#line 1015
            break;
        }

#line 1015
        float _S82 = tile_tap_0(_S74, _S75, tile_uv_5, SHADOW_DISC_0[index_3] * _S78, rotation_3, reference_4, kernelContext_15);

        float visibility_8 = visibility_2 + _S82;

#line 1015
        index_3 = index_3 + 1U;

#line 1015
        visibility_2 = visibility_8;

#line 1015
    }

#line 1020
    return visibility_2 / 32.0f;
}


#line 1093
float cascade_visibility_0(uint cascade_1, float3 world_position_1, float3 to_light_2, float3 geometric_normal_1, float2 pixel_4, KernelContext_0 thread* kernelContext_16)
{

#line 1094
    float4 _S83 = atlas_rect_0(cascade_1, kernelContext_16);

#line 1128
    if(atlas_rect_is_empty_0(_S83))
    {


        return 1.0f;
    }
    float _S84 = 2.0f * kernelContext_16->frame_0->cascade_far_0[cascade_1];

#line 1134
    float _S85 = tile_texels_0(_S83, kernelContext_16);

#line 1134
    float texel_world_0 = _S84 / _S85;

#line 1141
    float4 clip_0 = (((float4(world_position_1 + geometric_normal_1 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_16->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(0)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(1)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(2)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(0)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(1)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(2)][int(3)], (&kernelContext_16->frame_0->shadow_view_proj_0)->data_1[cascade_1].data_0[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1145
    bool _S86;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1146
        _S86 = true;

#line 1146
    }
    else
    {

#line 1146
        _S86 = (ndc_0.z) <= 0.0f;

#line 1146
    }

#line 1146
    if(_S86)
    {



        return 1.0f;
    }



    float2 tile_uv_6 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 1156
    uint _S87 = shadow_filter_mode_0(pixel_4, kernelContext_16);

#line 1173
    if(_S87 == 2U)
    {

#line 1173
        float _S88 = tile_box_pcf_0(cascade_1, tile_uv_6, ndc_0.z, kernelContext_16);

        return _S88;
    }

#line 1175
    float radius_3;

#line 1183
    if(_S87 != 1U)
    {

#line 1183
        float _S89 = sun_penumbra_texels_0(cascade_1, tile_uv_6, ndc_0.z, shadow_rotation_0(pixel_4), kernelContext_16);

#line 1183
        radius_3 = _S89;

#line 1183
    }
    else
    {

#line 1183
        radius_3 = 2.0f;

#line 1183
    }

#line 1183
    float _S90 = tile_pcf_0(cascade_1, tile_uv_6, ndc_0.z, pixel_4, radius_3, kernelContext_16);

#line 1188
    return _S90;
}

float sun_visibility_0(float3 world_position_2, float3 to_light_3, float n_dot_l_0, float3 geometric_normal_2, float2 pixel_5, uint thread* selected_0, float thread* fade_0, KernelContext_0 thread* kernelContext_17)
{
    uint cascade_2;

#line 1193
    bool covered_0;

#line 1202
    *selected_0 = 2U;
    *fade_0 = 0.0f;
    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 1214
    float eye_distance_0 = length(world_position_2 - kernelContext_17->frame_0->camera_position_0.xyz);

#line 1214
    uint index_4 = 0U;

#line 1222
    for(;;)
    {

#line 1222
        if(index_4 < 2U)
        {
        }
        else
        {

#line 1222
            covered_0 = false;

#line 1222
            cascade_2 = 1U;

#line 1222
            break;
        }
        if(eye_distance_0 < kernelContext_17->frame_0->cascade_far_0[index_4])
        {

#line 1224
            covered_0 = true;

#line 1224
            cascade_2 = index_4;



            break;
        }

#line 1222
        index_4 = index_4 + 1U;

#line 1222
    }

#line 1231
    if(covered_0)
    {
        *selected_0 = cascade_2;

#line 1231
    }

#line 1231
    float blend_0;

#line 1243
    if((cascade_2 + 1U) < 2U)
    {

        float band_1 = kernelContext_17->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;

#line 1246
        blend_0 = saturate((eye_distance_0 - (kernelContext_17->frame_0->cascade_far_0[cascade_2] - band_1)) / band_1);

#line 1243
    }
    else
    {

#line 1243
        blend_0 = 0.0f;

#line 1243
    }

#line 1251
    *fade_0 = blend_0;

#line 1251
    uint lookups_0;

#line 1265
    if(blend_0 > 0.0f)
    {

#line 1265
        lookups_0 = 2U;

#line 1265
    }
    else
    {

#line 1265
        lookups_0 = 1U;

#line 1265
    }

#line 1265
    float visibility_9 = 1.0f;

#line 1265
    float next_0 = 1.0f;

#line 1265
    uint step_1 = 0U;



    for(;;)
    {

#line 1269
        if(step_1 < lookups_0)
        {
        }
        else
        {

#line 1269
            break;
        }

#line 1269
        float _S91 = cascade_visibility_0(cascade_2 + step_1, world_position_2, to_light_3, geometric_normal_2, pixel_5, kernelContext_17);


        if(step_1 == 0U)
        {

#line 1272
            visibility_9 = _S91;

#line 1272
        }
        else
        {

#line 1272
            next_0 = _S91;

#line 1272
        }

#line 1269
        step_1 = step_1 + 1U;

#line 1269
    }

#line 1281
    if(lookups_0 == 1U)
    {

#line 1281
        blend_0 = visibility_9;

#line 1281
    }
    else
    {

#line 1281
        blend_0 = mix(visibility_9, next_0, blend_0);

#line 1281
    }

#line 1281
    return blend_0;
}


#line 1363
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S92 = axis_1.x;

#line 1366
    float _S93 = axis_1.y;

#line 1366
    bool _S94;

#line 1366
    if(_S92 >= _S93)
    {

#line 1366
        _S94 = _S92 >= (axis_1.z);

#line 1366
    }
    else
    {

#line 1366
        _S94 = false;

#line 1366
    }

#line 1366
    uint _S95;

#line 1366
    if(_S94)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1368
            _S95 = 0U;

#line 1368
        }
        else
        {

#line 1368
            _S95 = 1U;

#line 1368
        }

#line 1368
        return _S95;
    }
    if(_S93 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1372
            _S95 = 2U;

#line 1372
        }
        else
        {

#line 1372
            _S95 = 3U;

#line 1372
        }

#line 1372
        return _S95;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1374
        _S95 = 4U;

#line 1374
    }
    else
    {

#line 1374
        _S95 = 5U;

#line 1374
    }

#line 1374
    return _S95;
}




float point_map_world_0(const GpuLight_natural_0 thread* light_0, float3 world_position_3, uint thread* face_0)
{
    float3 from_light_1 = world_position_3 - (float4(light_0->position_0) ).xyz;

#line 1387
    float _S96 = max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z));
    *face_0 = point_face_0(from_light_1);
    return 2.0f * _S96;
}


#line 1347
float spot_map_world_0(const GpuLight_natural_0 thread* light_1, float3 world_position_4)
{

#line 1347
    float4 _S97 = float4(light_1->direction_0) ;

#line 1353
    float cos_outer_1 = _S97.w;

#line 1360
    return 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_4 - (float4(light_1->position_0) ).xyz, normalize(_S97.xyz)), 0.0f);
}


#line 904
uint light_tile_0(uint tile_6)
{
    return 2U + tile_6;
}


#line 1284
float punctual_visibility_0(uint tile_7, float3 world_position_5, float3 to_light_4, float n_dot_l_1, float map_world_0, float3 geometric_normal_3, float2 pixel_6, KernelContext_0 thread* kernelContext_18)
{

    uint atlas_0 = light_tile_0(tile_7);

#line 1287
    float4 _S98 = atlas_rect_0(atlas_0, kernelContext_18);

    if(atlas_rect_is_empty_0(_S98))
    {


        return 1.0f;
    }

#line 1293
    float _S99 = tile_texels_0(_S98, kernelContext_18);

    float texel_world_1 = map_world_0 / _S99;

#line 1305
    float4 clip_1 = (((float4(world_position_5 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(0)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(1)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(2)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(0)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(1)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(2)][int(3)], (&kernelContext_18->frame_0->light_view_proj_0)->data_2[tile_7].data_0[int(3)][int(3)]))));

#line 1312
    float _S100 = clip_1.w;

#line 1312
    if(_S100 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S100) ;

#line 1316
    bool _S101;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1317
        _S101 = true;

#line 1317
    }
    else
    {

#line 1317
        _S101 = (ndc_1.z) <= 0.0f;

#line 1317
    }

#line 1317
    if(_S101)
    {

#line 1317
        _S101 = true;

#line 1317
    }
    else
    {

#line 1317
        _S101 = (ndc_1.z) > 1.0f;

#line 1317
    }

#line 1317
    if(_S101)
    {

#line 1324
        return 1.0f;
    }



    float2 tile_uv_7 = float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f);

#line 1329
    uint _S102 = shadow_filter_mode_0(pixel_6, kernelContext_18);

#line 1338
    if(_S102 == 2U)
    {

#line 1338
        float _S103 = tile_box_pcf_0(atlas_0, tile_uv_7, ndc_1.z, kernelContext_18);

        return _S103;
    }

#line 1340
    float _S104 = tile_pcf_0(atlas_0, tile_uv_7, ndc_1.z, pixel_6, 2.0f, kernelContext_18);

    return _S104;
}


#line 1576
float3 grass_light_0(float3 world_position_6, float3 normal_3, float2 pixel_7, KernelContext_0 thread* kernelContext_19)
{

#line 1576
    uint _S105 = froxel_of_0(pixel_7, (((float4(world_position_6, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_19->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_19->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_19->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_19->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_19->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_19->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_19->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_19->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_19->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_19->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_19->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_19->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_19->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_19->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_19->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_19->frame_0->view_proj_0.data_0[int(3)][int(3)])))).w, kernelContext_19);

#line 1587
    uint base_1 = _S105 * 17U;
    uint _S106 = min(kernelContext_19->cluster_lights_0[base_1], 16U);

    float3 _S107 = float3(0.0f, 0.0f, 0.0f);

#line 1590
    uint slot_1 = 0U;

#line 1590
    float3 direct_0 = _S107;
    for(;;)
    {

#line 1591
        if(slot_1 < _S106)
        {
        }
        else
        {

#line 1591
            break;
        }

#line 1591
        thread GpuLight_natural_0 _S108 = kernelContext_19->lights_0[kernelContext_19->cluster_lights_0[base_1 + 1U + slot_1]];

#line 1591
        uint _S109 = (&_S108)->kind_0;


        if(((&_S108)->kind_0) == 3U)
        {



            slot_1 = slot_1 + 1U;

#line 1591
            continue;
        }

#line 1604
        bool _S110 = _S109 == 0U;

#line 1604
        float3 to_light_5;

#line 1604
        float reach_1;

#line 1604
        if(_S110)
        {

#line 1604
            to_light_5 = normalize((float4((&_S108)->direction_0) ).xyz);

#line 1604
            reach_1 = 1.0f;

#line 1604
        }
        else
        {

#line 1604
            float4 _S111 = float4((&_S108)->position_0) ;

#line 1611
            float3 offset_0 = _S111.xyz - world_position_6;
            float distance_3 = length(offset_0);
            float3 to_light_6 = offset_0 / float3(max(distance_3, 9.99999997475242708e-07f)) ;
            float reach_2 = punctual_falloff_0(distance_3, _S111.w);
            if(_S109 == 2U)
            {

#line 1615
                float4 _S112 = float4((&_S108)->direction_0) ;

#line 1615
                reach_1 = reach_2 * spot_cone_0(to_light_6, _S112.xyz, _S112.w, (&_S108)->cos_inner_0);

#line 1615
            }
            else
            {

#line 1615
                reach_1 = reach_2;

#line 1615
            }

#line 1615
            to_light_5 = to_light_6;

#line 1604
        }

#line 1622
        float n_dot_l_2 = dot(normal_3, to_light_5);

#line 1622
        float reach_3;
        if(_S110)
        {
            thread uint sun_cascade_0;
            thread float sun_fade_0;

#line 1626
            float _S113 = sun_visibility_0(world_position_6, to_light_5, n_dot_l_2, normal_3, pixel_7, &sun_cascade_0, &sun_fade_0, kernelContext_19);

#line 1626
            reach_3 = _S113;

#line 1623
        }
        else
        {

#line 1623
            uint tile_8;

#line 1623
            float map_world_1;

#line 1637
            if(_S109 == 1U)
            {

#line 1637
                uint _S114 = (&_S108)->shadow_tile_0;

                if(((&_S108)->shadow_tile_0) <= 8U)
                {
                    thread uint face_1;

#line 1641
                    float _S115 = point_map_world_0(&_S108, world_position_6, &face_1);

#line 1641
                    tile_8 = _S114 + face_1;

#line 1641
                    map_world_1 = _S115;

#line 1639
                }
                else
                {

#line 1639
                    tile_8 = 4294967295U;

#line 1639
                    map_world_1 = 0.0f;

#line 1639
                }

#line 1637
            }
            else
            {

#line 1637
                uint _S116 = (&_S108)->shadow_tile_0;

#line 1646
                if(((&_S108)->shadow_tile_0) < 14U)
                {

#line 1646
                    float _S117 = spot_map_world_0(&_S108, world_position_6);

#line 1646
                    tile_8 = _S116;

#line 1646
                    map_world_1 = _S117;

#line 1646
                }
                else
                {

#line 1646
                    tile_8 = 4294967295U;

#line 1646
                    map_world_1 = 0.0f;

#line 1646
                }

#line 1637
            }

#line 1637
            bool _S118;

#line 1651
            if(tile_8 != 4294967295U)
            {

#line 1651
                _S118 = n_dot_l_2 > 0.0f;

#line 1651
            }
            else
            {

#line 1651
                _S118 = false;

#line 1651
            }

#line 1651
            if(_S118)
            {

#line 1651
                float _S119 = punctual_visibility_0(tile_8, world_position_6, to_light_5, n_dot_l_2, map_world_1, normal_3, pixel_7, kernelContext_19);

#line 1651
                reach_3 = reach_1 * _S119;

#line 1651
            }
            else
            {

#line 1651
                reach_3 = reach_1;

#line 1651
            }

#line 1623
        }

#line 1623
        direct_0 = direct_0 + (float4((&_S108)->color_0) ).xyz * float3((max(n_dot_l_2, 0.0f) * reach_3)) ;

#line 1591
        slot_1 = slot_1 + 1U;

#line 1591
    }

#line 1661
    return direct_0 + kernelContext_19->frame_0->ambient_0.xyz;
}


#line 1661
struct pixelOutput_0
{
    float4 output_1 [[color(0)]];
};


#line 1661
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


#line 1661
float3 grass_style_1(float3 _S120, uint _S121, float _S122, float _S123, KernelContext_0 thread* kernelContext_20)
{

#line 1661
    GrassBlade_natural_0 device* _S124 = kernelContext_20->blades_1+_S121;

#line 1661
    float4 _S125 = float4(_S124->occlusion_0) ;

#line 1562
    float reach_4 = _S125.w;

#line 1562
    float occluded_1;
    if(reach_4 > 0.0f)
    {

#line 1563
        occluded_1 = saturate(1.0f - _S122 / reach_4);

#line 1563
    }
    else
    {

#line 1563
        occluded_1 = 0.0f;

#line 1563
    }

#line 1563
    float4 _S126 = float4(_S124->patch_0) ;

#line 1563
    float4 _S127 = float4(_S124->glow_0) ;


    float start_1 = _S127.w;

    return mix(mix(_S120, _S125.xyz, float3(occluded_1) ), _S126.xyz, float3((_S126.w * _S123)) ) + _S127.xyz * float3(saturate((_S122 - start_1) / max(1.0f - start_1, 0.00009999999747379f))) ;
}


#line 1937
[[fragment]] pixelOutput_0 bladeFragmentMain(pixelInput_0 _S128 [[stage_in]], float4 position_3 [[position]], GrassTile_0 constant* tile_9 [[buffer(2)]], GrassParams_0 constant* grass_1 [[buffer(1)]], GrassInstance_natural_0 device* instances_1 [[buffer(3)]], GrassBlade_natural_0 device* blades_2 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GrassField_0 constant* field_1 [[buffer(7)]], uint device* cluster_lights_1 [[buffer(6)]], GpuLight_natural_0 device* lights_1 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_1 [[texture(0)]], sampler grassCardSampler_1 [[sampler(0)]], texture2d<float, access::sample> grassGround_1 [[texture(2)]], WindParams_0 constant* wind_1 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_1 [[texture(3)]], sampler windSampler_1 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_1 [[texture(4)]], GrassInstance_natural_0 device* grassCells_1 [[buffer(8)]])
{

#line 1937
    thread KernelContext_0 kernelContext_21;

#line 1937
    (&kernelContext_21)->tile_1 = tile_9;

#line 1937
    (&kernelContext_21)->grass_0 = grass_1;

#line 1937
    (&kernelContext_21)->instances_0 = instances_1;

#line 1937
    (&kernelContext_21)->blades_1 = blades_2;

#line 1937
    (&kernelContext_21)->frame_0 = frame_1;

#line 1937
    (&kernelContext_21)->field_0 = field_1;

#line 1937
    (&kernelContext_21)->cluster_lights_0 = cluster_lights_1;

#line 1937
    (&kernelContext_21)->lights_0 = lights_1;

#line 1937
    (&kernelContext_21)->shadow_atlas_0 = shadow_atlas_1;

#line 1937
    (&kernelContext_21)->shadow_sampler_0 = shadow_sampler_1;

#line 1937
    (&kernelContext_21)->grassCard_0 = grassCard_1;

#line 1937
    (&kernelContext_21)->grassCardSampler_0 = grassCardSampler_1;

#line 1937
    (&kernelContext_21)->grassGround_0 = grassGround_1;

#line 1937
    (&kernelContext_21)->wind_0 = wind_1;

#line 1937
    (&kernelContext_21)->windDirectionLayer_0 = windDirectionLayer_1;

#line 1937
    (&kernelContext_21)->windSampler_0 = windSampler_1;

#line 1937
    (&kernelContext_21)->windIntensityLayer_0 = windIntensityLayer_1;

#line 1937
    (&kernelContext_21)->grassCells_0 = grassCells_1;

#line 1937
    float3 _S129 = grass_style_1(_S128.color_3, _S128.row_4, _S128.uv_1.y, _S128.patch_3, &kernelContext_21);

#line 1937
    float3 _S130 = grass_light_0(_S128.world_position_7, normalize(_S128.normal_4), position_3.xy, &kernelContext_21);

#line 1937
    pixelOutput_0 _S131 = { float4(_S129 * _S130, 1.0f) };



    return _S131;
}


#line 1941
struct pixelOutput_1
{
    float4 output_2 [[color(0)]];
};


#line 1941
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


#line 1946
[[fragment]] pixelOutput_1 fragmentMain(pixelInput_1 _S132 [[stage_in]], float4 position_4 [[position]], GrassTile_0 constant* tile_10 [[buffer(2)]], GrassParams_0 constant* grass_2 [[buffer(1)]], GrassInstance_natural_0 device* instances_2 [[buffer(3)]], GrassBlade_natural_0 device* blades_3 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GrassField_0 constant* field_2 [[buffer(7)]], uint device* cluster_lights_2 [[buffer(6)]], GpuLight_natural_0 device* lights_2 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_2 [[texture(0)]], sampler grassCardSampler_2 [[sampler(0)]], texture2d<float, access::sample> grassGround_2 [[texture(2)]], WindParams_0 constant* wind_2 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_2 [[texture(3)]], sampler windSampler_2 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_2 [[texture(4)]], GrassInstance_natural_0 device* grassCells_2 [[buffer(8)]])
{

#line 1946
    thread KernelContext_0 kernelContext_22;

#line 1946
    (&kernelContext_22)->tile_1 = tile_10;

#line 1946
    (&kernelContext_22)->grass_0 = grass_2;

#line 1946
    (&kernelContext_22)->instances_0 = instances_2;

#line 1946
    (&kernelContext_22)->blades_1 = blades_3;

#line 1946
    (&kernelContext_22)->frame_0 = frame_2;

#line 1946
    (&kernelContext_22)->field_0 = field_2;

#line 1946
    (&kernelContext_22)->cluster_lights_0 = cluster_lights_2;

#line 1946
    (&kernelContext_22)->lights_0 = lights_2;

#line 1946
    (&kernelContext_22)->shadow_atlas_0 = shadow_atlas_2;

#line 1946
    (&kernelContext_22)->shadow_sampler_0 = shadow_sampler_2;

#line 1946
    (&kernelContext_22)->grassCard_0 = grassCard_2;

#line 1946
    (&kernelContext_22)->grassCardSampler_0 = grassCardSampler_2;

#line 1946
    (&kernelContext_22)->grassGround_0 = grassGround_2;

#line 1946
    (&kernelContext_22)->wind_0 = wind_2;

#line 1946
    (&kernelContext_22)->windDirectionLayer_0 = windDirectionLayer_2;

#line 1946
    (&kernelContext_22)->windSampler_0 = windSampler_2;

#line 1946
    (&kernelContext_22)->windIntensityLayer_0 = windIntensityLayer_2;

#line 1946
    (&kernelContext_22)->grassCells_0 = grassCells_2;

#line 1952
    float _S133 = _S132.uv_2.y;

#line 1952
    float3 _S134 = float3(_S132.uv_2.x, 1.0f - _S133, 0.0f);

    if((((grassCard_2).sample((grassCardSampler_2), ((_S134)).xy, uint(((_S134)).z), level((_S132.level_3)))).x) < 0.5f)
    {
        discard_fragment();

#line 1954
    }

#line 1954
    float3 _S135 = grass_style_1(_S132.color_4, _S132.row_5, _S133, _S132.patch_4, &kernelContext_22);

#line 1954
    float3 _S136 = grass_light_0(_S132.world_position_8, normalize(_S132.normal_5), position_4.xy, &kernelContext_22);

#line 1954
    pixelOutput_1 _S137 = { float4(_S135 * _S136, 1.0f) };

#line 1962
    return _S137;
}


#line 2017
uint grass_shell_count_0(KernelContext_0 thread* kernelContext_23)
{
    return clamp(uint(kernelContext_23->field_0->stack_0.y + 0.5f), 1U, 64U);
}


#line 1510
float grass_ground_texel_0(int2 texel_1, KernelContext_0 thread* kernelContext_24)
{

    int3 _S138 = int3(clamp(texel_1, int2(int(0), int(0)), int2(int(kernelContext_24->field_0->maps_0.x) - int(1), int(kernelContext_24->field_0->maps_0.y) - int(1))), int(0));

#line 1513
    return ((kernelContext_24->grassGround_0).read(vec<uint,2>(((_S138)).xy), uint(((_S138)).z)).x);
}


#line 1501
struct GrassGround_0
{
    float height_1;
    float3 normal_6;
};


#line 1518
GrassGround_0 grass_ground_under_0(float2 world_0, KernelContext_0 thread* kernelContext_25)
{
    float2 at_0 = (world_0 - kernelContext_25->field_0->ground_1.xy) * float2(kernelContext_25->field_0->ground_1.w) ;
    float2 base_2 = floor(at_0);
    float2 blend_1 = at_0 - base_2;
    int2 _S139 = int2(base_2);

#line 1523
    float _S140 = grass_ground_texel_0(_S139, kernelContext_25);

#line 1523
    float _S141 = grass_ground_texel_0(_S139 + int2(int(1), int(0)), kernelContext_25);

#line 1523
    float _S142 = grass_ground_texel_0(_S139 + int2(int(0), int(1)), kernelContext_25);

#line 1523
    float _S143 = grass_ground_texel_0(_S139 + int2(int(1), int(1)), kernelContext_25);

#line 1530
    float _S144 = blend_1.x;

#line 1530
    float _S145 = _S141 - _S140;

#line 1530
    float lower_2 = _S140 + _S144 * _S145;
    float _S146 = _S143 - _S142;

#line 1529
    thread GrassGround_0 under_0;


    (&under_0)->height_1 = lower_2 + blend_1.y * (_S142 + _S144 * _S146 - lower_2);


    (&under_0)->normal_6 = normalize(float3(- (0.5f * (_S145 + _S146)), kernelContext_25->field_0->ground_1.z, - (0.5f * (_S142 - _S140 + (_S143 - _S141)))));
    return under_0;
}


#line 1440
float windSmoothTriangle_0(float u_1)
{
    float s_0 = abs(fract(u_1 + 0.5f) * 2.0f - 1.0f);
    return s_0 * s_0 * (3.0f - 2.0f * s_0);
}


float3 windSample_0(float3 posRel_0, KernelContext_0 thread* kernelContext_26)
{


    float2 _S147 = posRel_0.xz;


    float2 deflection_0 = ((kernelContext_26->windDirectionLayer_0).sample((kernelContext_26->windSampler_0), (kernelContext_26->wind_0->directionUv_0 + _S147 * kernelContext_26->wind_0->directionUvPerMetre_0), level((0.0f)))).xy * float2(2.0f)  - float2(1.0f) ;



    float intensity_0 = ((kernelContext_26->windIntensityLayer_0).sample((kernelContext_26->windSampler_0), (kernelContext_26->wind_0->intensityUv_0 + _S147 * kernelContext_26->wind_0->intensityUvPerMetre_0), level((0.0f)))).x;

    float2 base_3 = kernelContext_26->wind_0->baseDirection_0;
    float _S148 = kernelContext_26->wind_0->baseDirection_0.x;

#line 1461
    float _S149 = deflection_0.x;

#line 1461
    float _S150 = kernelContext_26->wind_0->baseDirection_0.y;

#line 1461
    float _S151 = deflection_0.y;

#line 1461
    float2 turned_0 = float2(_S148 * _S149 - _S150 * _S151, _S148 * _S151 + _S150 * _S149);

    float lengthSquared_0 = dot(turned_0, turned_0);

#line 1463
    float2 direction_2;

    if(lengthSquared_0 > 9.999999960041972e-13f)
    {

#line 1465
        direction_2 = turned_0 / float2(sqrt(lengthSquared_0)) ;

#line 1465
    }
    else
    {

#line 1465
        direction_2 = base_3;

#line 1465
    }

#line 1470
    float speed_0 = intensity_0 * kernelContext_26->wind_0->baseSpeed_0 * (1.0f + kernelContext_26->wind_0->gustAmplitude_0 * (2.0f * windSmoothTriangle_0(dot(_S147, base_3) * kernelContext_26->wind_0->invGustWavelength_0 + kernelContext_26->wind_0->gustPhase_0) - 1.0f));
    return float3(direction_2.x * speed_0, 0.0f, direction_2.y * speed_0);
}



float3 grass_lean_0(float3 velocity_0, float height_2)
{
    float speed_1 = length(velocity_0);
    float bend_0 = height_2 * 0.60000002384185791f * (speed_1 / (speed_1 + 6.0f));
    float2 along_2 = velocity_0.xz * float2((bend_0 / max(speed_1, 9.99999997475242708e-07f))) ;
    float _S152 = bend_0 * bend_0;
    return float3(along_2.x, - (_S152 / (height_2 + sqrt(max(height_2 * height_2 - _S152, 0.0f)))), along_2.y);
}


#line 1966
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


#line 1990
GrassShellVertex_0 grass_sheet_vertex_0(float2 rest_1, float share_0, float4 fin_1, float occlusion_2, KernelContext_0 thread* kernelContext_27)
{

#line 1990
    GrassGround_0 _S153 = grass_ground_under_0(rest_1, kernelContext_27);


    float lift_0 = share_0 * kernelContext_27->field_0->stack_0.x;
    float _S154 = rest_1.x;

#line 1994
    float _S155 = rest_1.y;

#line 1994
    float3 ground_2 = float3(_S154, _S153.height_1, _S155);

#line 1994
    float3 _S156 = windSample_0(ground_2 - kernelContext_27->frame_0->camera_position_0.xyz, kernelContext_27);

#line 2001
    float3 world_1 = ground_2 + float3(0.0f, lift_0, 0.0f) + grass_lean_0(_S156, kernelContext_27->field_0->stack_0.x) * float3((share_0 * share_0)) ;

    thread GrassShellVertex_0 output_3;
    float4 _S157 = (((float4(world_1, 1.0f)) * (matrix<float,int(4),int(4)> (kernelContext_27->frame_0->view_proj_0.data_0[int(0)][int(0)], kernelContext_27->frame_0->view_proj_0.data_0[int(1)][int(0)], kernelContext_27->frame_0->view_proj_0.data_0[int(2)][int(0)], kernelContext_27->frame_0->view_proj_0.data_0[int(3)][int(0)], kernelContext_27->frame_0->view_proj_0.data_0[int(0)][int(1)], kernelContext_27->frame_0->view_proj_0.data_0[int(1)][int(1)], kernelContext_27->frame_0->view_proj_0.data_0[int(2)][int(1)], kernelContext_27->frame_0->view_proj_0.data_0[int(3)][int(1)], kernelContext_27->frame_0->view_proj_0.data_0[int(0)][int(2)], kernelContext_27->frame_0->view_proj_0.data_0[int(1)][int(2)], kernelContext_27->frame_0->view_proj_0.data_0[int(2)][int(2)], kernelContext_27->frame_0->view_proj_0.data_0[int(3)][int(2)], kernelContext_27->frame_0->view_proj_0.data_0[int(0)][int(3)], kernelContext_27->frame_0->view_proj_0.data_0[int(1)][int(3)], kernelContext_27->frame_0->view_proj_0.data_0[int(2)][int(3)], kernelContext_27->frame_0->view_proj_0.data_0[int(3)][int(3)]))));

#line 2004
    (&output_3)->position_5 = _S157;
    (&output_3)->world_position_9 = world_1;
    (&output_3)->rest_0 = float3(_S154, lift_0, _S155);
    (&output_3)->normal_7 = _S153.normal_6;


    (&output_3)->footprint_0 = max(abs(_S157.w), 0.00009999999747379f) / max(kernelContext_27->grass_0->screen_0.x, 0.00009999999747379f);
    (&output_3)->occlusion_1 = occlusion_2;
    (&output_3)->fin_0 = fin_1;
    return output_3;
}


#line 2039
float grass_occlusion_at_0(float share_1, KernelContext_0 thread* kernelContext_28)
{

#line 2039
    uint _S158 = grass_shell_count_0(kernelContext_28);

#line 2039
    float occlusion_3 = kernelContext_28->field_0->layers_0[int(0)].y;

#line 2039
    uint shell_0 = 0U;



    for(;;)
    {

#line 2043
        if(shell_0 < _S158)
        {
        }
        else
        {

#line 2043
            break;
        }
        if((kernelContext_28->field_0->layers_0[shell_0].x) <= share_1)
        {

#line 2045
            occlusion_3 = kernelContext_28->field_0->layers_0[shell_0].y;

#line 2045
        }

#line 2043
        shell_0 = shell_0 + 1U;

#line 2043
    }

#line 2050
    return occlusion_3;
}


#line 543
struct GrassInstance_0
{
    float4 root_0;
    float4 facing_0;
    float4 lean_0;
    float4 ground_0;
    float4 clump_1;
    uint4 lanes_0;
};


#line 2117
GrassInstance_0 grass_no_blade_0()
{
    thread GrassInstance_0 none_0;
    float4 _S159 = float4(0.0f, 0.0f, 0.0f, 0.0f);

#line 2120
    (&none_0)->root_0 = _S159;
    (&none_0)->facing_0 = _S159;
    (&none_0)->lean_0 = _S159;
    (&none_0)->ground_0 = _S159;
    (&none_0)->lanes_0 = uint4(0U, 0U, 0U, 0U);
    return none_0;
}



GrassInstance_0 grass_cell_0(int2 global_0, KernelContext_0 thread* kernelContext_29)
{
    int side_1 = int(max(kernelContext_29->field_0->tiles_0.z, 1U));
    int2 extent_0 = int2(int(kernelContext_29->field_0->tiles_0.x), int(kernelContext_29->field_0->tiles_0.y)) * int2(side_1) ;
    int _S160 = global_0.x;

#line 2134
    bool _S161;

#line 2134
    if(_S160 < int(0))
    {

#line 2134
        _S161 = true;

#line 2134
    }
    else
    {

#line 2134
        _S161 = (global_0.y) < int(0);

#line 2134
    }

#line 2134
    if(_S161)
    {

#line 2134
        _S161 = true;

#line 2134
    }
    else
    {

#line 2134
        _S161 = _S160 >= (extent_0.x);

#line 2134
    }

#line 2134
    if(_S161)
    {

#line 2134
        _S161 = true;

#line 2134
    }
    else
    {

#line 2134
        _S161 = (global_0.y) >= (extent_0.y);

#line 2134
    }

#line 2134
    if(_S161)
    {
        return grass_no_blade_0();
    }
    uint2 _S162 = uint2(global_0);
    uint cells_0 = uint(side_1);
    uint _S163 = _S162.y;

#line 2140
    uint _S164 = _S163 / cells_0;

#line 2140
    uint _S165 = _S164 * kernelContext_29->field_0->tiles_0.x;

#line 2140
    uint _S166 = _S162.x;

#line 2140
    uint _S167 = _S166 / cells_0;

#line 2140
    uint slot_2 = _S165 + _S167;
    uint _S168 = _S163 % cells_0;

#line 2141
    uint _S169 = _S168 * cells_0;

#line 2141
    uint _S170 = _S166 % cells_0;
    GrassInstance_natural_0 _S171 = kernelContext_29->grassCells_0[slot_2 * kernelContext_29->field_0->tiles_0.w + (_S169 + _S170)];

#line 2142
    GrassInstance_0 _S172 = { float4(_S171.root_0) , float4(_S171.facing_0) , float4(_S171.lean_0) , float4(_S171.ground_0) , float4(_S171.clump_1) , uint4(_S171.lanes_0)  };

#line 2142
    return _S172;
}




float grass_strand_reach_0(const GrassInstance_0 thread* blade_3, float lift_1, float floor_reach_0, float fade_1, KernelContext_0 thread* kernelContext_30)
{
    float _S173 = blade_3->root_0.w;

#line 2150
    bool _S174;

#line 2150
    if(!(lift_1 < _S173))
    {

#line 2150
        _S174 = true;

#line 2150
    }
    else
    {

#line 2150
        GrassBlade_0 _S175 = grass_row_0(blade_3->lanes_0.y, kernelContext_30);

#line 2150
        _S174 = (_S175.flags_0.x) != 1U;

#line 2150
    }

#line 2150
    if(_S174)
    {
        return -1.0f;
    }
    float bound_0 = 0.5f * kernelContext_30->field_0->origin_0.w;

    return min(max(min(blade_3->facing_0.z, bound_0) * (1.0f - lift_1 / _S173), floor_reach_0), bound_0) * fade_1;
}


#line 2156
struct pixelOutput_2
{
    float4 output_4 [[color(0)]];
};


#line 2156
struct pixelInput_2
{
    float3 world_position_10 [[user(WORLD_POSITION)]];
    float3 rest_2 [[user(REST_POSITION)]];
    float3 normal_8 [[user(NORMAL)]];
    float footprint_1 [[user(FOOTPRINT)]];
    float occlusion_4 [[user(OCCLUSION)]];
    [[flat]] float4 fin_2 [[user(FIN)]];
};


#line 2161
[[fragment]] pixelOutput_2 shellFragmentMain(pixelInput_2 _S176 [[stage_in]], float4 position_6 [[position]], GrassTile_0 constant* tile_11 [[buffer(2)]], GrassParams_0 constant* grass_3 [[buffer(1)]], GrassInstance_natural_0 device* instances_3 [[buffer(3)]], GrassBlade_natural_0 device* blades_4 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_3 [[buffer(0)]], GrassField_0 constant* field_3 [[buffer(7)]], uint device* cluster_lights_3 [[buffer(6)]], GpuLight_natural_0 device* lights_3 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_3 [[texture(1)]], sampler shadow_sampler_3 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_3 [[texture(0)]], sampler grassCardSampler_3 [[sampler(0)]], texture2d<float, access::sample> grassGround_3 [[texture(2)]], WindParams_0 constant* wind_3 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_3 [[texture(3)]], sampler windSampler_3 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_3 [[texture(4)]], GrassInstance_natural_0 device* grassCells_3 [[buffer(8)]])
{

#line 2161
    thread KernelContext_0 kernelContext_31;

#line 2161
    (&kernelContext_31)->tile_1 = tile_11;

#line 2161
    (&kernelContext_31)->grass_0 = grass_3;

#line 2161
    (&kernelContext_31)->instances_0 = instances_3;

#line 2161
    (&kernelContext_31)->blades_1 = blades_4;

#line 2161
    (&kernelContext_31)->frame_0 = frame_3;

#line 2161
    (&kernelContext_31)->field_0 = field_3;

#line 2161
    (&kernelContext_31)->cluster_lights_0 = cluster_lights_3;

#line 2161
    (&kernelContext_31)->lights_0 = lights_3;

#line 2161
    (&kernelContext_31)->shadow_atlas_0 = shadow_atlas_3;

#line 2161
    (&kernelContext_31)->shadow_sampler_0 = shadow_sampler_3;

#line 2161
    (&kernelContext_31)->grassCard_0 = grassCard_3;

#line 2161
    (&kernelContext_31)->grassCardSampler_0 = grassCardSampler_3;

#line 2161
    (&kernelContext_31)->grassGround_0 = grassGround_3;

#line 2161
    (&kernelContext_31)->wind_0 = wind_3;

#line 2161
    (&kernelContext_31)->windDirectionLayer_0 = windDirectionLayer_3;

#line 2161
    (&kernelContext_31)->windSampler_0 = windSampler_3;

#line 2161
    (&kernelContext_31)->windIntensityLayer_0 = windIntensityLayer_3;

#line 2161
    (&kernelContext_31)->grassCells_0 = grassCells_3;

    float cell_1 = field_3->origin_0.w;
    float2 _S177 = _S176.rest_2.xz;

#line 2164
    float2 from_corner_0 = (_S177 - field_3->origin_0.xy) / float2(cell_1) ;
    float2 _S178 = floor(from_corner_0);

#line 2165
    int2 _S179 = int2(_S178);
    float2 inside_0 = from_corner_0 - _S178;

#line 2166
    int _S180;
    if((inside_0.x) < 0.5f)
    {

#line 2167
        _S180 = int(-1);

#line 2167
    }
    else
    {

#line 2167
        _S180 = int(1);

#line 2167
    }

#line 2167
    int band_2;

#line 2167
    if((inside_0.y) < 0.5f)
    {

#line 2167
        band_2 = int(-1);

#line 2167
    }
    else
    {

#line 2167
        band_2 = int(1);

#line 2167
    }
    float lift_2 = _S176.rest_2.y;


    float _S181 = 0.5f * _S176.footprint_1;
    float _S182 = _S176.fin_2.y;

    GrassInstance_0 _S183 = grass_no_blade_0();

    float _S184 = _S176.fin_2.x;

#line 2176
    int dz_0;

#line 2176
    int dx_0;

#line 2176
    float nearest_0;

#line 2176
    float nearest_1;

#line 2176
    GrassInstance_0 found_2;

#line 2176
    GrassInstance_0 found_3;

#line 2176
    bool _S185;

#line 2176
    if(_S184 < 0.5f)
    {

#line 2176
        nearest_1 = 2.0f;

#line 2176
        found_3 = _S183;

#line 2176
        dz_0 = int(0);


        for(;;)
        {

#line 2179
            if(dz_0 < int(2))
            {
            }
            else
            {

#line 2179
                break;
            }

#line 2179
            nearest_0 = nearest_1;

#line 2179
            found_2 = found_3;

#line 2179
            dx_0 = int(0);

            for(;;)
            {

#line 2181
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2181
                    break;
                }

#line 2181
                GrassInstance_0 _S186 = grass_cell_0(_S179 + int2(dx_0 * _S180, dz_0 * band_2), &kernelContext_31);

#line 2181
                thread GrassInstance_0 _S187 = _S186;

#line 2181
                float _S188 = grass_strand_reach_0(&_S187, lift_2, _S181, _S182, &kernelContext_31);



                float2 offset_1 = _S177 - _S186.root_0.xz;
                float apart_0 = sqrt(dot(offset_1, offset_1)) / max(_S188, 9.99999997475242708e-07f);
                if(_S188 > 0.0f)
                {

#line 2187
                    _S185 = apart_0 < nearest_0;

#line 2187
                }
                else
                {

#line 2187
                    _S185 = false;

#line 2187
                }

#line 2187
                if(_S185)
                {

#line 2187
                    nearest_0 = apart_0;

#line 2187
                    found_2 = _S186;

#line 2187
                }

#line 2181
                dx_0 = dx_0 + int(1);

#line 2181
            }

#line 2179
            int dz_1 = dz_0 + int(1);

#line 2179
            nearest_1 = nearest_0;

#line 2179
            found_3 = found_2;

#line 2179
            dz_0 = dz_1;

#line 2179
        }

#line 2179
        nearest_0 = nearest_1;

#line 2179
        found_2 = found_3;

#line 2176
    }
    else
    {

#line 2199
        bool along_z_0 = _S184 < 1.5f;
        float _S189 = _S176.fin_2.z;

#line 2200
        if(along_z_0)
        {

#line 2200
            nearest_0 = field_3->origin_0.x;

#line 2200
        }
        else
        {

#line 2200
            nearest_0 = field_3->origin_0.y;

#line 2200
        }
        int _S190 = int(floor((_S189 - nearest_0) / cell_1 + 0.5f)) - int(2);
        if(along_z_0)
        {

#line 2202
            dz_0 = _S179.y;

#line 2202
        }
        else
        {

#line 2202
            dz_0 = _S179.x;

#line 2202
        }
        if(along_z_0)
        {

#line 2203
            _S180 = band_2;

#line 2203
        }

#line 2203
        nearest_0 = 2.0f;

#line 2203
        found_2 = _S183;

#line 2203
        band_2 = int(0);
        for(;;)
        {

#line 2204
            if(band_2 < int(4))
            {
            }
            else
            {

#line 2204
                break;
            }

#line 2204
            nearest_1 = nearest_0;

#line 2204
            found_3 = found_2;

#line 2204
            dx_0 = int(0);

            for(;;)
            {

#line 2206
                if(dx_0 < int(2))
                {
                }
                else
                {

#line 2206
                    break;
                }
                int along_3 = dz_0 + dx_0 * _S180;

#line 2208
                int2 _S191;
                if(along_z_0)
                {

#line 2209
                    _S191 = int2(_S190 + band_2, along_3);

#line 2209
                }
                else
                {

#line 2209
                    _S191 = int2(along_3, _S190 + band_2);

#line 2209
                }

#line 2209
                GrassInstance_0 _S192 = grass_cell_0(_S191, &kernelContext_31);

#line 2209
                thread GrassInstance_0 _S193 = _S192;

#line 2209
                float _S194 = grass_strand_reach_0(&_S193, lift_2, _S181, _S182, &kernelContext_31);

#line 2209
                float offset_2;



                if(along_z_0)
                {

#line 2213
                    offset_2 = _S176.rest_2.z - _S192.root_0.z;

#line 2213
                }
                else
                {

#line 2213
                    offset_2 = _S176.rest_2.x - _S192.root_0.x;

#line 2213
                }
                float apart_1 = abs(offset_2) / max(_S194, 9.99999997475242708e-07f);
                if(_S194 > 0.0f)
                {

#line 2215
                    _S185 = apart_1 < nearest_1;

#line 2215
                }
                else
                {

#line 2215
                    _S185 = false;

#line 2215
                }

#line 2215
                if(_S185)
                {

#line 2215
                    nearest_1 = apart_1;

#line 2215
                    found_3 = _S192;

#line 2215
                }

#line 2206
                dx_0 = dx_0 + int(1);

#line 2206
            }

#line 2204
            int band_3 = band_2 + int(1);

#line 2204
            nearest_0 = nearest_1;

#line 2204
            found_2 = found_3;

#line 2204
            band_2 = band_3;

#line 2204
        }

#line 2176
    }

#line 2223
    if(!(nearest_0 < 1.0f))
    {
        discard_fragment();

#line 2223
    }

#line 2223
    GrassBlade_0 _S195 = grass_row_0(found_2.lanes_0.y, &kernelContext_31);

#line 2229
    float along_blade_0 = saturate(lift_2 / found_2.root_0.w);

    float3 color_5 = mix(_S195.root_color_0.xyz, _S195.tip_color_0.xyz, float3(along_blade_0) ) * float3((1.0f - 0.18000000715255737f * found_2.facing_0.w)) ;
    float _S196 = grass_patch_0(found_2.lanes_0.z);

#line 2232
    thread GrassBlade_0 _S197 = _S195;

#line 2232
    float3 _S198 = grass_style_0(color_5, &_S197, along_blade_0, _S196);

#line 2232
    float3 color_6 = _S198 * float3(_S176.occlusion_4) ;

#line 2232
    float3 normal_9;

    if((_S195.flags_0.y) == 1U)
    {

#line 2234
        normal_9 = float3(0.0f, 1.0f, 0.0f);

#line 2234
    }
    else
    {

#line 2234
        normal_9 = normalize(_S176.normal_8);

#line 2234
    }

#line 2234
    float3 _S199 = grass_light_0(_S176.world_position_10, normal_9, position_6.xy, &kernelContext_31);

#line 2234
    pixelOutput_2 _S200 = { float4(color_6 * _S199, 1.0f) };
    return _S200;
}


#line 2235
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


#line 2235
[[vertex]] vertexMain_Result_0 vertexMain(uint index_5 [[vertex_id]], uint instance_id_0 [[instance_id]], uint base_instance_0 [[base_instance]], GrassTile_0 constant* tile_12 [[buffer(2)]], GrassParams_0 constant* grass_4 [[buffer(1)]], GrassInstance_natural_0 device* instances_4 [[buffer(3)]], GrassBlade_natural_0 device* blades_5 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_4 [[buffer(0)]], GrassField_0 constant* field_4 [[buffer(7)]], uint device* cluster_lights_4 [[buffer(6)]], GpuLight_natural_0 device* lights_4 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_4 [[texture(1)]], sampler shadow_sampler_4 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_4 [[texture(0)]], sampler grassCardSampler_4 [[sampler(0)]], texture2d<float, access::sample> grassGround_4 [[texture(2)]], WindParams_0 constant* wind_4 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_4 [[texture(3)]], sampler windSampler_4 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_4 [[texture(4)]], GrassInstance_natural_0 device* grassCells_4 [[buffer(8)]])
{

#line 2235
    thread KernelContext_0 kernelContext_32;

#line 2235
    (&kernelContext_32)->tile_1 = tile_12;

#line 2235
    (&kernelContext_32)->grass_0 = grass_4;

#line 2235
    (&kernelContext_32)->instances_0 = instances_4;

#line 2235
    (&kernelContext_32)->blades_1 = blades_5;

#line 2235
    (&kernelContext_32)->frame_0 = frame_4;

#line 2235
    (&kernelContext_32)->field_0 = field_4;

#line 2235
    (&kernelContext_32)->cluster_lights_0 = cluster_lights_4;

#line 2235
    (&kernelContext_32)->lights_0 = lights_4;

#line 2235
    (&kernelContext_32)->shadow_atlas_0 = shadow_atlas_4;

#line 2235
    (&kernelContext_32)->shadow_sampler_0 = shadow_sampler_4;

#line 2235
    (&kernelContext_32)->grassCard_0 = grassCard_4;

#line 2235
    (&kernelContext_32)->grassCardSampler_0 = grassCardSampler_4;

#line 2235
    (&kernelContext_32)->grassGround_0 = grassGround_4;

#line 2235
    (&kernelContext_32)->wind_0 = wind_4;

#line 2235
    (&kernelContext_32)->windDirectionLayer_0 = windDirectionLayer_4;

#line 2235
    (&kernelContext_32)->windSampler_0 = windSampler_4;

#line 2235
    (&kernelContext_32)->windIntensityLayer_0 = windIntensityLayer_4;

#line 2235
    (&kernelContext_32)->grassCells_0 = grassCells_4;

#line 1871
    GrassInstance_natural_0 blade_4 = instances_4[tile_12->slot_0.x * grass_4->limits_0.x + (instance_id_0 - base_instance_0)];

#line 1871
    uint4 _S201 = uint4(blade_4.lanes_0) ;
    uint _S202 = _S201.y;

#line 1872
    GrassBlade_0 _S203 = grass_row_0(_S202, &kernelContext_32);


    uint _S204 = index_5 % 6U;

#line 1875
    float4 _S205 = float4(blade_4.facing_0) ;

#line 1880
    float2 facing_1 = _S205.xy;

#line 1880
    float2 across_0;
    if((index_5 / 6U) == 0U)
    {

#line 1881
        across_0 = facing_1;

#line 1881
    }
    else
    {

#line 1881
        across_0 = float2(- facing_1.y, facing_1.x);

#line 1881
    }

    float _S206 = _S205.z;

#line 1888
    float _S207 = GRASS_CARD_CORNERS_0[_S204].y;

#line 1888
    float4 _S208 = float4(blade_4.root_0) ;
    float3 world_2 = _S208.xyz + float3(across_0.x, 0.0f, across_0.y) * float3((_S206 * (GRASS_CARD_CORNERS_0[_S204].x * 2.0f - 1.0f)))  + float3(0.0f, _S208.w * _S207, 0.0f) + (float4(blade_4.lean_0) ).xyz * float3((_S207 * _S207)) ;

    thread GrassVertex_0 output_5;
    (&output_5)->position_2 = (((float4(world_2, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(0)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(1)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(2)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(0)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(1)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(2)][int(3)], (&kernelContext_32)->frame_0->view_proj_0.data_0[int(3)][int(3)]))));
    (&output_5)->world_position_0 = world_2;

#line 1893
    float3 _S209;
    if((_S203.flags_0.y) == 1U)
    {

#line 1894
        _S209 = float3(0.0f, 1.0f, 0.0f);

#line 1894
    }
    else
    {

#line 1894
        _S209 = (float4(blade_4.ground_0) ).xyz;

#line 1894
    }

#line 1894
    (&output_5)->normal_2 = _S209;

#line 1899
    (&output_5)->color_1 = mix(_S203.root_color_0.xyz, _S203.tip_color_0.xyz, float3(_S207) ) * float3((1.0f - 0.18000000715255737f * _S205.w)) ;
    (&output_5)->uv_0 = GRASS_CARD_CORNERS_0[_S204];

#line 1905
    (&output_5)->level_1 = grassCardLevel_0(2.0f * _S206 * (&kernelContext_32)->grass_0->screen_0.x / max(abs((&output_5)->position_2.w), 0.00009999999747379f));

    (&output_5)->row_2 = min(_S202, max(grass_4->limits_0.y, 1U) - 1U);
    (&output_5)->patch_1 = grass_patch_0(_S201.z);
    GrassVertex_0 _S210 = output_5;

#line 1909
    thread vertexMain_Result_0 _S211;

#line 1909
    (&_S211)->position_7 = _S210.position_2;

#line 1909
    (&_S211)->world_position_11 = _S210.world_position_0;

#line 1909
    (&_S211)->normal_10 = _S210.normal_2;

#line 1909
    (&_S211)->color_7 = _S210.color_1;

#line 1909
    (&_S211)->uv_3 = _S210.uv_0;

#line 1909
    (&_S211)->level_4 = _S210.level_1;

#line 1909
    (&_S211)->row_6 = _S210.row_2;

#line 1909
    (&_S211)->patch_5 = _S210.patch_1;

#line 1909
    return _S211;
}


#line 1909
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


#line 1909
[[vertex]] bladeNearVertexMain_Result_0 bladeNearVertexMain(uint index_6 [[vertex_id]], uint instance_id_1 [[instance_id]], uint base_instance_1 [[base_instance]], GrassTile_0 constant* tile_13 [[buffer(2)]], GrassParams_0 constant* grass_5 [[buffer(1)]], GrassInstance_natural_0 device* instances_5 [[buffer(3)]], GrassBlade_natural_0 device* blades_6 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_5 [[buffer(0)]], GrassField_0 constant* field_5 [[buffer(7)]], uint device* cluster_lights_5 [[buffer(6)]], GpuLight_natural_0 device* lights_5 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_5 [[texture(1)]], sampler shadow_sampler_5 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_5 [[texture(0)]], sampler grassCardSampler_5 [[sampler(0)]], texture2d<float, access::sample> grassGround_5 [[texture(2)]], WindParams_0 constant* wind_5 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_5 [[texture(3)]], sampler windSampler_5 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_5 [[texture(4)]], GrassInstance_natural_0 device* grassCells_5 [[buffer(8)]])
{

#line 1909
    thread KernelContext_0 kernelContext_33;

#line 1909
    (&kernelContext_33)->tile_1 = tile_13;

#line 1909
    (&kernelContext_33)->grass_0 = grass_5;

#line 1909
    (&kernelContext_33)->instances_0 = instances_5;

#line 1909
    (&kernelContext_33)->blades_1 = blades_6;

#line 1909
    (&kernelContext_33)->frame_0 = frame_5;

#line 1909
    (&kernelContext_33)->field_0 = field_5;

#line 1909
    (&kernelContext_33)->cluster_lights_0 = cluster_lights_5;

#line 1909
    (&kernelContext_33)->lights_0 = lights_5;

#line 1909
    (&kernelContext_33)->shadow_atlas_0 = shadow_atlas_5;

#line 1909
    (&kernelContext_33)->shadow_sampler_0 = shadow_sampler_5;

#line 1909
    (&kernelContext_33)->grassCard_0 = grassCard_5;

#line 1909
    (&kernelContext_33)->grassCardSampler_0 = grassCardSampler_5;

#line 1909
    (&kernelContext_33)->grassGround_0 = grassGround_5;

#line 1909
    (&kernelContext_33)->wind_0 = wind_5;

#line 1909
    (&kernelContext_33)->windDirectionLayer_0 = windDirectionLayer_5;

#line 1909
    (&kernelContext_33)->windSampler_0 = windSampler_5;

#line 1909
    (&kernelContext_33)->windIntensityLayer_0 = windIntensityLayer_5;

#line 1909
    (&kernelContext_33)->grassCells_0 = grassCells_5;

#line 1919
    GrassInstance_natural_0 blade_5 = instances_5[(field_5->tiles_0.x * field_5->tiles_0.y + tile_13->slot_0.x) * grass_5->limits_0.x + (instance_id_1 - base_instance_1)];

#line 1919
    thread GrassInstance_natural_0 _S212 = blade_5;

#line 1919
    float2 _S213 = grass_blade_lod_0(length((float4((&_S212)->root_0) ).xyz - frame_5->camera_position_0.xyz), &kernelContext_33);

#line 1919
    _S212 = blade_5;

#line 1919
    GrassVertex_0 _S214 = grass_blade_vertex_0(&_S212, index_6, 7U, _S213, &kernelContext_33);

#line 1919
    thread bladeNearVertexMain_Result_0 _S215;

#line 1919
    (&_S215)->position_8 = _S214.position_2;

#line 1919
    (&_S215)->world_position_12 = _S214.world_position_0;

#line 1919
    (&_S215)->normal_11 = _S214.normal_2;

#line 1919
    (&_S215)->color_8 = _S214.color_1;

#line 1919
    (&_S215)->uv_4 = _S214.uv_0;

#line 1919
    (&_S215)->level_5 = _S214.level_1;

#line 1919
    (&_S215)->row_7 = _S214.row_2;

#line 1919
    (&_S215)->patch_6 = _S214.patch_1;

#line 1919
    return _S215;
}


#line 1919
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


#line 1919
[[vertex]] bladeFarVertexMain_Result_0 bladeFarVertexMain(uint index_7 [[vertex_id]], uint instance_id_2 [[instance_id]], uint base_instance_2 [[base_instance]], GrassTile_0 constant* tile_14 [[buffer(2)]], GrassParams_0 constant* grass_6 [[buffer(1)]], GrassInstance_natural_0 device* instances_6 [[buffer(3)]], GrassBlade_natural_0 device* blades_7 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_6 [[buffer(0)]], GrassField_0 constant* field_6 [[buffer(7)]], uint device* cluster_lights_6 [[buffer(6)]], GpuLight_natural_0 device* lights_6 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_6 [[texture(1)]], sampler shadow_sampler_6 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_6 [[texture(0)]], sampler grassCardSampler_6 [[sampler(0)]], texture2d<float, access::sample> grassGround_6 [[texture(2)]], WindParams_0 constant* wind_6 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_6 [[texture(3)]], sampler windSampler_6 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_6 [[texture(4)]], GrassInstance_natural_0 device* grassCells_6 [[buffer(8)]])
{

#line 1919
    thread KernelContext_0 kernelContext_34;

#line 1919
    (&kernelContext_34)->tile_1 = tile_14;

#line 1919
    (&kernelContext_34)->grass_0 = grass_6;

#line 1919
    (&kernelContext_34)->instances_0 = instances_6;

#line 1919
    (&kernelContext_34)->blades_1 = blades_7;

#line 1919
    (&kernelContext_34)->frame_0 = frame_6;

#line 1919
    (&kernelContext_34)->field_0 = field_6;

#line 1919
    (&kernelContext_34)->cluster_lights_0 = cluster_lights_6;

#line 1919
    (&kernelContext_34)->lights_0 = lights_6;

#line 1919
    (&kernelContext_34)->shadow_atlas_0 = shadow_atlas_6;

#line 1919
    (&kernelContext_34)->shadow_sampler_0 = shadow_sampler_6;

#line 1919
    (&kernelContext_34)->grassCard_0 = grassCard_6;

#line 1919
    (&kernelContext_34)->grassCardSampler_0 = grassCardSampler_6;

#line 1919
    (&kernelContext_34)->grassGround_0 = grassGround_6;

#line 1919
    (&kernelContext_34)->wind_0 = wind_6;

#line 1919
    (&kernelContext_34)->windDirectionLayer_0 = windDirectionLayer_6;

#line 1919
    (&kernelContext_34)->windSampler_0 = windSampler_6;

#line 1919
    (&kernelContext_34)->windIntensityLayer_0 = windIntensityLayer_6;

#line 1919
    (&kernelContext_34)->grassCells_0 = grassCells_6;

#line 1930
    uint capacity_0 = grass_6->limits_0.x;

    float2 _S216 = float2(1.0f, 1.0f);

#line 1932
    thread GrassInstance_natural_0 _S217 = instances_6[tile_14->slot_0.x * capacity_0 + capacity_0 - 1U - (instance_id_2 - base_instance_2)];

#line 1932
    GrassVertex_0 _S218 = grass_blade_vertex_0(&_S217, index_7, 3U, _S216, &kernelContext_34);

#line 1932
    thread bladeFarVertexMain_Result_0 _S219;

#line 1932
    (&_S219)->position_9 = _S218.position_2;

#line 1932
    (&_S219)->world_position_13 = _S218.world_position_0;

#line 1932
    (&_S219)->normal_12 = _S218.normal_2;

#line 1932
    (&_S219)->color_9 = _S218.color_1;

#line 1932
    (&_S219)->uv_5 = _S218.uv_0;

#line 1932
    (&_S219)->level_6 = _S218.level_1;

#line 1932
    (&_S219)->row_8 = _S218.row_2;

#line 1932
    (&_S219)->patch_7 = _S218.patch_1;

#line 1932
    return _S219;
}


#line 1932
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


#line 1932
[[vertex]] shellVertexMain_Result_0 shellVertexMain(uint index_8 [[vertex_id]], uint instance_id_3 [[instance_id]], uint base_instance_3 [[base_instance]], GrassTile_0 constant* tile_15 [[buffer(2)]], GrassParams_0 constant* grass_7 [[buffer(1)]], GrassInstance_natural_0 device* instances_7 [[buffer(3)]], GrassBlade_natural_0 device* blades_8 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_7 [[buffer(0)]], GrassField_0 constant* field_7 [[buffer(7)]], uint device* cluster_lights_7 [[buffer(6)]], GpuLight_natural_0 device* lights_7 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_7 [[texture(1)]], sampler shadow_sampler_7 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_7 [[texture(0)]], sampler grassCardSampler_7 [[sampler(0)]], texture2d<float, access::sample> grassGround_7 [[texture(2)]], WindParams_0 constant* wind_7 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_7 [[texture(3)]], sampler windSampler_7 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_7 [[texture(4)]], GrassInstance_natural_0 device* grassCells_7 [[buffer(8)]])
{

#line 1932
    thread KernelContext_0 kernelContext_35;

#line 1932
    (&kernelContext_35)->tile_1 = tile_15;

#line 1932
    (&kernelContext_35)->grass_0 = grass_7;

#line 1932
    (&kernelContext_35)->instances_0 = instances_7;

#line 1932
    (&kernelContext_35)->blades_1 = blades_8;

#line 1932
    (&kernelContext_35)->frame_0 = frame_7;

#line 1932
    (&kernelContext_35)->field_0 = field_7;

#line 1932
    (&kernelContext_35)->cluster_lights_0 = cluster_lights_7;

#line 1932
    (&kernelContext_35)->lights_0 = lights_7;

#line 1932
    (&kernelContext_35)->shadow_atlas_0 = shadow_atlas_7;

#line 1932
    (&kernelContext_35)->shadow_sampler_0 = shadow_sampler_7;

#line 1932
    (&kernelContext_35)->grassCard_0 = grassCard_7;

#line 1932
    (&kernelContext_35)->grassCardSampler_0 = grassCardSampler_7;

#line 1932
    (&kernelContext_35)->grassGround_0 = grassGround_7;

#line 1932
    (&kernelContext_35)->wind_0 = wind_7;

#line 1932
    (&kernelContext_35)->windDirectionLayer_0 = windDirectionLayer_7;

#line 1932
    (&kernelContext_35)->windSampler_0 = windSampler_7;

#line 1932
    (&kernelContext_35)->windIntensityLayer_0 = windIntensityLayer_7;

#line 1932
    (&kernelContext_35)->grassCells_0 = grassCells_7;

#line 1932
    uint _S220 = instance_id_3 - base_instance_3;

#line 1932
    uint _S221 = grass_shell_count_0(&kernelContext_35);

#line 2028
    uint _S222 = _S221 - 1U;
    uint quad_0 = index_8 / 6U;

#line 2029
    GrassShellVertex_0 _S223 = grass_sheet_vertex_0((&kernelContext_35)->tile_1->tile_0.xy + (float2(float(quad_0 % 16U), float(quad_0 / 16U)) + GRASS_CARD_CORNERS_0[index_8 % 6U]) * float2(((&kernelContext_35)->tile_1->tile_0.z / 16.0f)) , (&kernelContext_35)->field_0->layers_0[_S222 - min(_S220, _S222)].x, float4(0.0f, 1.0f, 0.0f, 0.0f), (&kernelContext_35)->field_0->layers_0[_S222 - min(_S220, _S222)].y, &kernelContext_35);

#line 2029
    thread shellVertexMain_Result_0 _S224;

#line 2029
    (&_S224)->position_10 = _S223.position_5;

#line 2029
    (&_S224)->world_position_14 = _S223.world_position_9;

#line 2029
    (&_S224)->rest_3 = _S223.rest_0;

#line 2029
    (&_S224)->normal_13 = _S223.normal_7;

#line 2029
    (&_S224)->footprint_2 = _S223.footprint_0;

#line 2029
    (&_S224)->occlusion_5 = _S223.occlusion_1;

#line 2029
    (&_S224)->fin_3 = _S223.fin_0;

#line 2029
    return _S224;
}


#line 2029
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


#line 2029
[[vertex]] finVertexMain_Result_0 finVertexMain(uint index_9 [[vertex_id]], GrassTile_0 constant* tile_16 [[buffer(2)]], GrassParams_0 constant* grass_8 [[buffer(1)]], GrassInstance_natural_0 device* instances_8 [[buffer(3)]], GrassBlade_natural_0 device* blades_9 [[buffer(4)]], FrameUniforms_natural_0 constant* frame_8 [[buffer(0)]], GrassField_0 constant* field_8 [[buffer(7)]], uint device* cluster_lights_8 [[buffer(6)]], GpuLight_natural_0 device* lights_8 [[buffer(5)]], depth2d<float, access::sample> shadow_atlas_8 [[texture(1)]], sampler shadow_sampler_8 [[sampler(1)]], texture2d_array<float, access::sample> grassCard_8 [[texture(0)]], sampler grassCardSampler_8 [[sampler(0)]], texture2d<float, access::sample> grassGround_8 [[texture(2)]], WindParams_0 constant* wind_8 [[buffer(9)]], texture2d<float, access::sample> windDirectionLayer_8 [[texture(3)]], sampler windSampler_8 [[sampler(2)]], texture2d<float, access::sample> windIntensityLayer_8 [[texture(4)]], GrassInstance_natural_0 device* grassCells_8 [[buffer(8)]])
{

#line 2029
    thread KernelContext_0 kernelContext_36;

#line 2029
    (&kernelContext_36)->tile_1 = tile_16;

#line 2029
    (&kernelContext_36)->grass_0 = grass_8;

#line 2029
    (&kernelContext_36)->instances_0 = instances_8;

#line 2029
    (&kernelContext_36)->blades_1 = blades_9;

#line 2029
    (&kernelContext_36)->frame_0 = frame_8;

#line 2029
    (&kernelContext_36)->field_0 = field_8;

#line 2029
    (&kernelContext_36)->cluster_lights_0 = cluster_lights_8;

#line 2029
    (&kernelContext_36)->lights_0 = lights_8;

#line 2029
    (&kernelContext_36)->shadow_atlas_0 = shadow_atlas_8;

#line 2029
    (&kernelContext_36)->shadow_sampler_0 = shadow_sampler_8;

#line 2029
    (&kernelContext_36)->grassCard_0 = grassCard_8;

#line 2029
    (&kernelContext_36)->grassCardSampler_0 = grassCardSampler_8;

#line 2029
    (&kernelContext_36)->grassGround_0 = grassGround_8;

#line 2029
    (&kernelContext_36)->wind_0 = wind_8;

#line 2029
    (&kernelContext_36)->windDirectionLayer_0 = windDirectionLayer_8;

#line 2029
    (&kernelContext_36)->windSampler_0 = windSampler_8;

#line 2029
    (&kernelContext_36)->windIntensityLayer_0 = windIntensityLayer_8;

#line 2029
    (&kernelContext_36)->grassCells_0 = grassCells_8;

#line 2066
    uint quad_1 = index_9 / 6U;
    uint _S225 = index_9 % 6U;
    uint _S226 = max(field_8->tiles_0.z / 4U, 1U);

    uint _S227 = quad_1 / (_S226 * 64U);

#line 2070
    uint _S228 = min(_S227, 1U);
    uint fin_line_0 = quad_1 / 64U % _S226;



    float band_4 = 4.0f * (&kernelContext_36)->field_0->origin_0.w;
    float across_1 = (float(fin_line_0) + 0.5f) * band_4;
    float stride_0 = (&kernelContext_36)->tile_1->tile_0.z / 16.0f;
    float _S229 = float(quad_1 / 4U % 16U);

#line 2078
    float along_4 = (_S229 + GRASS_CARD_CORNERS_0[_S225].x) * stride_0;
    float middle_along_0 = (_S229 + 0.5f) * stride_0;
    float share_2 = (float(quad_1 % 4U) + GRASS_CARD_CORNERS_0[_S225].y) / 4.0f;



    bool along_z_1 = _S228 == 0U;
    float2 _S230 = (&kernelContext_36)->tile_1->tile_0.xy;

#line 2085
    float2 side_2;

#line 2085
    if(along_z_1)
    {

#line 2085
        side_2 = float2(across_1, along_4);

#line 2085
    }
    else
    {

#line 2085
        side_2 = float2(along_4, across_1);

#line 2085
    }

#line 2085
    float2 rest_5 = _S230 + side_2;

    float2 _S231 = (&kernelContext_36)->tile_1->tile_0.xy;

#line 2087
    if(along_z_1)
    {

#line 2087
        side_2 = float2(across_1, middle_along_0);

#line 2087
    }
    else
    {

#line 2087
        side_2 = float2(middle_along_0, across_1);

#line 2087
    }

#line 2087
    float2 middle_0 = _S231 + side_2;
    if(along_z_1)
    {

#line 2088
        side_2 = float2(0.5f * band_4, 0.0f);

#line 2088
    }
    else
    {

#line 2088
        side_2 = float2(0.0f, 0.5f * band_4);

#line 2088
    }

#line 2088
    GrassGround_0 _S232 = grass_ground_under_0(middle_0, &kernelContext_36);

#line 2088
    GrassGround_0 _S233 = grass_ground_under_0(middle_0 - side_2, &kernelContext_36);

#line 2088
    GrassGround_0 _S234 = grass_ground_under_0(middle_0 + side_2, &kernelContext_36);

#line 2094
    float3 eye_0 = (&kernelContext_36)->frame_0->camera_position_0.xyz - float3(middle_0.x, _S232.height_1 + 0.5f * (&kernelContext_36)->field_0->stack_0.x, middle_0.y);
    float3 view_1 = eye_0 / float3(max(length(eye_0), 9.99999997475242708e-07f)) ;
    float facing_near_0 = dot(_S233.normal_6, view_1);
    float facing_far_0 = dot(_S234.normal_6, view_1);

#line 2097
    float graze_0;

    if((facing_near_0 * facing_far_0) <= 0.0f)
    {

#line 2099
        graze_0 = 0.0f;

#line 2099
    }
    else
    {

#line 2099
        graze_0 = min(abs(facing_near_0), abs(facing_far_0));

#line 2099
    }

    float fade_2 = saturate((0.5f - graze_0) / 0.19999998807907104f);


    if(along_z_1)
    {

#line 2104
        graze_0 = (&kernelContext_36)->tile_1->tile_0.x;

#line 2104
    }
    else
    {

#line 2104
        graze_0 = (&kernelContext_36)->tile_1->tile_0.y;

#line 2104
    }

    float4 _S235 = float4(float(_S228) + 1.0f, fade_2, graze_0 + across_1, 0.0f);

#line 2106
    float _S236 = grass_occlusion_at_0(share_2, &kernelContext_36);

#line 2106
    GrassShellVertex_0 _S237 = grass_sheet_vertex_0(rest_5, share_2, _S235, _S236, &kernelContext_36);

#line 2105
    thread GrassShellVertex_0 output_6 = _S237;

#line 2105
    bool _S238;

    if(fade_2 <= 0.0f)
    {

#line 2107
        _S238 = true;

#line 2107
    }
    else
    {

#line 2107
        _S238 = ((&kernelContext_36)->field_0->stack_0.z) <= 0.0f;

#line 2107
    }

#line 2107
    if(_S238)
    {


        (&output_6)->position_5 = float4(0.0f, 0.0f, -1.0f, 1.0f);

#line 2107
    }

#line 2113
    GrassShellVertex_0 _S239 = output_6;

#line 2113
    thread finVertexMain_Result_0 _S240;

#line 2113
    (&_S240)->position_11 = _S239.position_5;

#line 2113
    (&_S240)->world_position_15 = _S239.world_position_9;

#line 2113
    (&_S240)->rest_4 = _S239.rest_0;

#line 2113
    (&_S240)->normal_14 = _S239.normal_7;

#line 2113
    (&_S240)->footprint_3 = _S239.footprint_0;

#line 2113
    (&_S240)->occlusion_6 = _S239.occlusion_1;

#line 2113
    (&_S240)->fin_4 = _S239.fin_0;

#line 2113
    return _S240;
}

