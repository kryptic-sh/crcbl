#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 1397 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 1392
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1664
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 1724
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 1876
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 1739
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 1767
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1106
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_0)
{
    return matrix<float,int(3),int(3)> (cross(basis_0[int(1)], basis_0[int(2)]), cross(basis_0[int(2)], basis_0[int(0)]), cross(basis_0[int(0)], basis_0[int(1)]));
}


#line 808
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1153
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1153
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint pad0_1;
    uint pad1_1;
    uint pad2_0;
};


#line 590
struct GpuMesh_0
{
    uint base_vertex_1;
    uint base_index_0;
    uint index_count_0;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
};


#line 1159
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 1159
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1159
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(14)> data_3;
};


#line 3332
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint4 cluster_grid_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E14_0 light_view_proj_0;
    float4 probe_origin_0;
    float4 probe_inv_spacing_0;
    uint4 probe_counts_0;
    float4 lod_params_0;
    float4 fog_params_0;
    float4 fog_color_0;
    float4 sky_sh_r_0;
    float4 sky_sh_g_0;
    float4 sky_sh_b_0;
};


#line 3332
struct GpuMaterial_natural_0
{
    packed_float4 base_color_0;
    uint base_color_texture_0;
    float metallic_0;
    float roughness_0;
    uint tiling_0;
    float tile_metres_0;
    float emissive_r_0;
    float emissive_g_0;
    float emissive_b_0;
};


#line 3332
struct GpuLight_natural_0
{
    packed_float4 position_1;
    packed_float4 color_1;
    packed_float4 direction_0;
    uint kind_0;
    float cos_inner_0;
    uint shadow_tile_0;
    uint pad1_2;
};


#line 3332
struct GpuProbe_natural_0
{
    packed_float4 sh_r_0;
    packed_float4 sh_g_0;
    packed_float4 sh_b_0;
};


#line 3332
struct KernelContext_0
{
    DrawConstants_0 constant* draw_0;
    uint device* visible_instances_0;
    GpuInstance_natural_0 device* instances_0;
    GpuMesh_0 device* meshes_0;
    MeshVertex_natural_0 device* vertices_0;
    FrameUniforms_natural_0 constant* frame_0;
    texture2d<float, access::sample> ambient_occlusion_0;
    GpuMaterial_natural_0 device* materials_0;
    texture2d_array<float, access::sample> base_color_textures_0;
    sampler base_color_sampler_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> specular_albedo_0;
    GpuProbe_natural_0 device* probes_0;
};


#line 2745 "shaders/mesh.slang"
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_0)
{

#line 2745
    texture2d<float, access::sample> _S1 = kernelContext_0->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S1).get_width(0)),(*((&height_0)) = (_S1).get_height(0));

    int3 _S2 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 2751
    return ((kernelContext_0->ambient_occlusion_0).read(vec<uint,2>(((_S2)).xy), uint(((_S2)).z)).x);
}


#line 1532
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1543
    float3 _S3;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1544
        _S3 = - facet_1;

#line 1544
    }
    else
    {

#line 1544
        _S3 = facet_1;

#line 1544
    }

#line 1544
    return _S3;
}


#line 2715
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S4 = axis_0.x;

#line 2719
    float _S5 = axis_0.y;

#line 2719
    bool _S6;

#line 2719
    if(_S4 >= _S5)
    {

#line 2719
        _S6 = _S4 >= (axis_0.z);

#line 2719
    }
    else
    {

#line 2719
        _S6 = false;

#line 2719
    }

#line 2719
    float2 planar_0;

#line 2719
    if(_S6)
    {

#line 2719
        planar_0 = world_position_1.zy;

#line 2719
    }
    else
    {

        if(_S5 >= (axis_0.z))
        {

#line 2723
            planar_0 = world_position_1.xz;

#line 2723
        }
        else
        {

#line 2723
            planar_0 = world_position_1.xy;

#line 2723
        }

#line 2719
    }

#line 2731
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 2525
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_1)
{
    uint _S7 = max(kernelContext_1->frame_0->cluster_grid_0.x, 1U);
    uint _S8 = max(kernelContext_1->frame_0->cluster_grid_0.y, 1U);
    uint _S9 = max(kernelContext_1->frame_0->cluster_grid_0.z, 1U);
    uint _S10 = max(kernelContext_1->frame_0->cluster_grid_0.w, 1U);

#line 2535
    uint _S11 = uint(pixel_0.x) / _S10;

#line 2535
    uint _S12 = min(_S11, _S7 - 1U);
    uint _S13 = uint(pixel_0.y) / _S10;

    float scale_0 = 24.0f / log2(10000.0f);

#line 2546
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S9 - 1U))) * _S8 + min(_S13, _S8 - 1U)) * _S7 + _S12;
}


#line 2490
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 2504
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 2511
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 1256
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 1263
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1270
    float _S14 = 1.0f - alpha2_0;

#line 1275
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S14 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S14 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 1583
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 1898
float2 shadow_rotation_0(float2 pixel_1)
{
    uint2 cell_0 = uint2(pixel_1) & (uint2(3U) );
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * 4U + cell_0.x]];
}


#line 237
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 2014
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_2)
{
    float2 texel_0 = kernelContext_2->frame_0->shadow_params_0.xy;
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S15 = float2(0.5f, 0.5f) * texel_0 * grid_0;


    float2 _S16 = float2(1.0f, 1.0f);

#line 2021
    float2 _S17 = _S16 / texel_0;

#line 2021
    uint index_0 = 0U;

#line 2021
    float sum_0 = 0.0f;

#line 2021
    float found_0 = 0.0f;



    for(;;)
    {

#line 2025
        if(index_0 < 16U)
        {
        }
        else
        {

#line 2025
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_0] * float2(8.0f) ;
        float _S18 = spoke_0.x;

#line 2028
        float _S19 = rotation_0.x;

#line 2028
        float _S20 = spoke_0.y;

#line 2028
        float _S21 = rotation_0.y;

#line 2037
        int3 _S22 = int3(int2(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + float2(_S18 * _S19 - _S20 * _S21, _S18 * _S21 + _S20 * _S19) * texel_0 * grid_0, _S15, float2(1.0f)  - _S15)) * _S17, _S17 - _S16)), int(0));

#line 2037
        float depth_1 = ((kernelContext_2->shadow_atlas_0).read(vec<uint,2>(((_S22)).xy), uint(((_S22)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 2041
            sum_0 = sum_0 + depth_1;

#line 2041
            found_0 = found_1;

#line 2038
        }

#line 2025
        index_0 = index_0 + 1U;

#line 2025
    }

#line 2045
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 2056
    float _S23 = 2.0f * kernelContext_2->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_0 / found_0 - reference_0) * (_S23 + 40.0f) * 0.01999999955296516f / (_S23 / 768.0f), 2.0f, 8.0f);
}


#line 1916
float tile_tap_0(uint tile_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_3)
{
    float2 texel_1 = kernelContext_3->frame_0->shadow_params_0.xy;

#line 1923
    float2 grid_1 = float2(4.0f, 4.0f);
    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_1 * grid_1;

    float _S24 = spoke_1.x;

#line 1926
    float _S25 = rotation_1.x;

#line 1926
    float _S26 = spoke_1.y;

#line 1926
    float _S27 = rotation_1.y;


    float _S28 = ((kernelContext_3->shadow_atlas_0).sample_compare((kernelContext_3->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + float2(_S24 * _S25 - _S26 * _S27, _S24 * _S27 + _S26 * _S25) * texel_1 * grid_1, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 1929
    return _S28;
}


#line 1951
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_1, KernelContext_0 thread* kernelContext_4)
{
    float2 _S29 = shadow_rotation_0(pixel_2);

#line 1953
    uint spot_0 = 0U;

#line 1953
    float probe_0 = 0.0f;


    for(;;)
    {

#line 1956
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 1956
            break;
        }

#line 1956
        float _S30 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_1) , _S29, reference_2, kernelContext_4);

        float probe_1 = probe_0 + _S30;

#line 1956
        spot_0 = spot_0 + 1U;

#line 1956
        probe_0 = probe_1;

#line 1956
    }

#line 1965
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 1971
    uint index_1 = 0U;

#line 1971
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 1975
        if(index_1 < 32U)
        {
        }
        else
        {

#line 1975
            break;
        }

#line 1975
        float _S31 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[index_1] * float2(radius_1) , _S29, reference_2, kernelContext_4);

        float visibility_1 = visibility_0 + _S31;

#line 1975
        index_1 = index_1 + 1U;

#line 1975
        visibility_0 = visibility_1;

#line 1975
    }



    return visibility_0 / 32.0f;
}


#line 2110
float cascade_visibility_0(uint cascade_1, float3 world_position_2, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_5)
{

#line 2141
    float texel_world_0 = 2.0f * kernelContext_5->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 2148
    float4 clip_0 = (((float4(world_position_2 + geometric_normal_1 * float3((texel_world_0 * kernelContext_5->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_5->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 2152
    bool _S32;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 2153
        _S32 = true;

#line 2153
    }
    else
    {

#line 2153
        _S32 = (ndc_0.z) <= 0.0f;

#line 2153
    }

#line 2153
    if(_S32)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 2180
    float _S33 = ndc_0.z;

#line 2180
    float _S34 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S33, shadow_rotation_0(pixel_3), kernelContext_5);

#line 2180
    float _S35 = tile_pcf_0(cascade_1, tile_uv_4, _S33, pixel_3, _S34, kernelContext_5);
    return _S35;
}


#line 2197
float sun_visibility_0(float3 world_position_3, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_6)
{

#line 2198
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 2210
    float eye_distance_0 = length(world_position_3 - kernelContext_6->frame_0->camera_position_0.xyz);

#line 2210
    uint index_2 = 0U;

    for(;;)
    {

#line 2212
        if(index_2 < 2U)
        {
        }
        else
        {

#line 2212
            cascade_2 = 1U;

#line 2212
            break;
        }
        if(eye_distance_0 < kernelContext_6->frame_0->cascade_far_0[index_2])
        {

#line 2214
            cascade_2 = index_2;


            break;
        }

#line 2212
        index_2 = index_2 + 1U;

#line 2212
    }

#line 2212
    float _S36 = cascade_visibility_0(cascade_2, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_6);

#line 2223
    uint _S37 = cascade_2 + 1U;

#line 2223
    if(_S37 >= 2U)
    {



        return _S36;
    }

#line 2236
    float band_0 = kernelContext_6->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_6->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S36;
    }

#line 2240
    float _S38 = cascade_visibility_0(_S37, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_6);

#line 2251
    return mix(_S36, _S38, blend_0);
}


#line 2441
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S39 = axis_2.x;

#line 2444
    float _S40 = axis_2.y;

#line 2444
    bool _S41;

#line 2444
    if(_S39 >= _S40)
    {

#line 2444
        _S41 = _S39 >= (axis_2.z);

#line 2444
    }
    else
    {

#line 2444
        _S41 = false;

#line 2444
    }

#line 2444
    uint _S42;

#line 2444
    if(_S41)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 2446
            _S42 = 0U;

#line 2446
        }
        else
        {

#line 2446
            _S42 = 1U;

#line 2446
        }

#line 2446
        return _S42;
    }
    if(_S40 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 2450
            _S42 = 2U;

#line 2450
        }
        else
        {

#line 2450
            _S42 = 3U;

#line 2450
        }

#line 2450
        return _S42;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 2452
        _S42 = 4U;

#line 2452
    }
    else
    {

#line 2452
        _S42 = 5U;

#line 2452
    }

#line 2452
    return _S42;
}


#line 225
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 2354
float punctual_visibility_0(uint tile_4, float3 world_position_4, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_7)
{

#line 2366
    float4 clip_1 = (((float4(world_position_4 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 2373
    float _S43 = clip_1.w;

#line 2373
    if(_S43 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S43) ;

#line 2377
    bool _S44;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 2378
        _S44 = true;

#line 2378
    }
    else
    {

#line 2378
        _S44 = (ndc_1.z) <= 0.0f;

#line 2378
    }

#line 2378
    if(_S44)
    {

#line 2378
        _S44 = true;

#line 2378
    }
    else
    {

#line 2378
        _S44 = (ndc_1.z) > 1.0f;

#line 2378
    }

#line 2378
    if(_S44)
    {

#line 2385
        return 1.0f;
    }

#line 2385
    float _S45 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_7);

#line 2395
    return _S45;
}


#line 2460
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_5, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_8)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_5 - (float4(light_0->position_1) ).xyz;

#line 2468
    float _S46 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_5, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_8);

#line 2474
    return _S46;
}


#line 2402
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_5, float3 world_position_6, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_9)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 2409
    float4 _S47 = float4(light_1->direction_0) ;

#line 2416
    float cos_outer_1 = _S47.w;

#line 2416
    float _S48 = punctual_visibility_0(tile_5, world_position_6, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_6 - (float4(light_1->position_1) ).xyz, normalize(_S47.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_9);

#line 2423
    return _S48;
}


#line 1300
float decode_specular_albedo_0(float2 texel_2)
{
    return (texel_2.x * 65280.0f + texel_2.y * 255.0f) / 65535.0f;
}


#line 1317
float specular_albedo_at_0(float n_dot_v_1, float roughness_1, KernelContext_0 thread* kernelContext_10)
{

#line 1317
    texture2d<float, access::sample> _S49 = kernelContext_10->specular_albedo_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S49).get_width(0)),(*((&height_1)) = (_S49).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1323
    float2 _S50 = float2(1.0f) ;
    float2 _S51 = extent_1 - _S50;

#line 1324
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S51);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S52 = int2(low_0);
    int2 _S53 = int2(min(low_0 + _S50, _S51));
    int _S54 = _S52.x;

#line 1330
    int _S55 = _S52.y;

#line 1330
    int3 _S56 = int3(_S54, _S55, int(0));
    int _S57 = _S53.x;

#line 1331
    int3 _S58 = int3(_S57, _S55, int(0));
    float _S59 = weight_0.x;
    int _S60 = _S53.y;

#line 1333
    int3 _S61 = int3(_S54, _S60, int(0));
    int3 _S62 = int3(_S57, _S60, int(0));

    return mix(mix(decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S56)).xy), uint(((_S56)).z)).xy)), decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S58)).xy), uint(((_S58)).z)).xy)), _S59), mix(decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S61)).xy), uint(((_S61)).z)).xy)), decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S62)).xy), uint(((_S62)).z)).xy)), _S59), weight_0.y);
}


#line 1355
float3 specular_compensation_0(float3 f0_1, float n_dot_v_2, float roughness_2, KernelContext_0 thread* kernelContext_11)
{

#line 1355
    float _S63 = specular_albedo_at_0(n_dot_v_2, roughness_2, kernelContext_11);



    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(_S63, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 2654
float3 sky_irradiance_0(float3 normal_2, KernelContext_0 thread* kernelContext_12)
{
    float4 basis_1 = float4(normal_2, 1.0f);
    return max(float3(dot(kernelContext_12->frame_0->sky_sh_r_0, basis_1), dot(kernelContext_12->frame_0->sky_sh_g_0, basis_1), dot(kernelContext_12->frame_0->sky_sh_b_0, basis_1)), float3(0.0f, 0.0f, 0.0f));
}


#line 719
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 2556
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_13)
{

    GpuProbe_natural_0 _S64 = kernelContext_13->probes_0[min((cell_1.z * kernelContext_13->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_13->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_13->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 2559
    GpuProbe_0 _S65 = { float4(_S64.sh_r_0) , float4(_S64.sh_g_0) , float4(_S64.sh_b_0)  };

#line 2559
    return _S65;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_0)
{
    thread GpuProbe_0 blended_0;
    float4 _S66 = float4(t_0) ;

#line 2567
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S66);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S66);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S66);
    return blended_0;
}


#line 2607
float3 probe_irradiance_0(float3 world_position_7, float3 normal_3, KernelContext_0 thread* kernelContext_14)
{

#line 2607
    float3 _S67 = float3(1.0f) ;

#line 2612
    float3 _S68 = float3(0.0f, 0.0f, 0.0f);

#line 2612
    float3 last_0 = max(float3(kernelContext_14->frame_0->probe_counts_0.xyz) - _S67, _S68);
    float3 grid_2 = clamp((world_position_7 - kernelContext_14->frame_0->probe_origin_0.xyz) * kernelContext_14->frame_0->probe_inv_spacing_0.xyz, _S68, last_0);

    float3 base_2 = floor(grid_2);
    float3 f_0 = grid_2 - base_2;

    uint3 _S69 = uint3(base_2);



    uint3 _S70 = uint3(min(base_2 + _S67, last_0));

#line 2629
    uint _S71 = _S69.x;

#line 2629
    uint _S72 = _S69.y;

#line 2629
    uint _S73 = _S69.z;

#line 2629
    GpuProbe_0 _S74 = probe_at_0(uint3(_S71, _S72, _S73), kernelContext_14);

#line 2629
    uint _S75 = _S70.x;

#line 2629
    GpuProbe_0 _S76 = probe_at_0(uint3(_S75, _S72, _S73), kernelContext_14);

#line 2629
    float _S77 = f_0.x;

#line 2629
    thread GpuProbe_0 _S78 = _S74;

#line 2629
    thread GpuProbe_0 _S79 = _S76;

#line 2629
    GpuProbe_0 _S80 = lerp_probe_0(&_S78, &_S79, _S77);
    uint _S81 = _S70.y;

#line 2630
    GpuProbe_0 _S82 = probe_at_0(uint3(_S71, _S81, _S73), kernelContext_14);

#line 2630
    GpuProbe_0 _S83 = probe_at_0(uint3(_S75, _S81, _S73), kernelContext_14);

#line 2630
    thread GpuProbe_0 _S84 = _S82;

#line 2630
    thread GpuProbe_0 _S85 = _S83;

#line 2630
    GpuProbe_0 _S86 = lerp_probe_0(&_S84, &_S85, _S77);
    uint _S87 = _S70.z;

#line 2631
    GpuProbe_0 _S88 = probe_at_0(uint3(_S71, _S72, _S87), kernelContext_14);

#line 2631
    GpuProbe_0 _S89 = probe_at_0(uint3(_S75, _S72, _S87), kernelContext_14);

#line 2631
    thread GpuProbe_0 _S90 = _S88;

#line 2631
    thread GpuProbe_0 _S91 = _S89;

#line 2631
    GpuProbe_0 _S92 = lerp_probe_0(&_S90, &_S91, _S77);

#line 2631
    GpuProbe_0 _S93 = probe_at_0(uint3(_S71, _S81, _S87), kernelContext_14);

#line 2631
    GpuProbe_0 _S94 = probe_at_0(uint3(_S75, _S81, _S87), kernelContext_14);

#line 2631
    thread GpuProbe_0 _S95 = _S93;

#line 2631
    thread GpuProbe_0 _S96 = _S94;

#line 2631
    GpuProbe_0 _S97 = lerp_probe_0(&_S95, &_S96, _S77);

    float _S98 = f_0.y;

#line 2633
    thread GpuProbe_0 _S99 = _S80;

#line 2633
    thread GpuProbe_0 _S100 = _S86;

#line 2633
    GpuProbe_0 _S101 = lerp_probe_0(&_S99, &_S100, _S98);

#line 2633
    thread GpuProbe_0 _S102 = _S92;

#line 2633
    thread GpuProbe_0 _S103 = _S97;

#line 2633
    GpuProbe_0 _S104 = lerp_probe_0(&_S102, &_S103, _S98);

    float _S105 = f_0.z;

#line 2635
    thread GpuProbe_0 _S106 = _S101;

#line 2635
    thread GpuProbe_0 _S107 = _S104;

#line 2635
    GpuProbe_0 _S108 = lerp_probe_0(&_S106, &_S107, _S105);

    float4 basis_2 = float4(normal_3, 1.0f);
    return max(float3(dot(_S108.sh_r_0, basis_2), dot(_S108.sh_g_0, basis_2), dot(_S108.sh_b_0, basis_2)), _S68);
}


#line 692
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_1)
{
    return float3(material_1->emissive_r_0, material_1->emissive_g_0, material_1->emissive_b_0);
}


#line 1417
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S109 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 1425
    float kernel_0 = 0.0001984127011383f;

#line 1425
    int term_0 = int(6);

    for(;;)
    {

#line 1427
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 1427
            break;
        }
        float _S110 = kernel_0 * _S109 + FOG_KERNEL_0[term_0];

#line 1427
        int term_1 = term_0 - int(1);

#line 1427
        kernel_0 = _S110;

#line 1427
        term_0 = term_1;

#line 1427
    }

#line 1434
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 1444
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S111 = - d_0;

#line 1448
        float series_0 = 0.00833333376795053f;

#line 1448
        int term_2 = int(3);

        for(;;)
        {

#line 1450
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 1450
                break;
            }
            float _S112 = series_0 * _S111 + FOG_RATIO_KERNEL_0[term_2];

#line 1450
            int term_3 = term_2 - int(1);

#line 1450
            series_0 = _S112;

#line 1450
            term_2 = term_3;

#line 1450
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 1478
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_1)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_1, 0.0f, 32.0f);
    }

#line 1489
    return clamp(density_0 * distance_1 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 1497
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 2680
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
};


#line 2680
struct pixelInput_0
{
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_2 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 2755
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S113 [[stage_in]], float4 position_3 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_1 [[texture(3)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 2755
    thread KernelContext_0 kernelContext_15;

#line 2755
    (&kernelContext_15)->draw_0 = draw_1;

#line 2755
    (&kernelContext_15)->visible_instances_0 = visible_instances_1;

#line 2755
    (&kernelContext_15)->instances_0 = instances_1;

#line 2755
    (&kernelContext_15)->meshes_0 = meshes_1;

#line 2755
    (&kernelContext_15)->vertices_0 = vertices_1;

#line 2755
    (&kernelContext_15)->frame_0 = frame_1;

#line 2755
    (&kernelContext_15)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2755
    (&kernelContext_15)->materials_0 = materials_1;

#line 2755
    (&kernelContext_15)->base_color_textures_0 = base_color_textures_1;

#line 2755
    (&kernelContext_15)->base_color_sampler_0 = base_color_sampler_1;

#line 2755
    (&kernelContext_15)->cluster_lights_0 = cluster_lights_1;

#line 2755
    (&kernelContext_15)->lights_0 = lights_1;

#line 2755
    (&kernelContext_15)->shadow_atlas_0 = shadow_atlas_1;

#line 2755
    (&kernelContext_15)->shadow_sampler_0 = shadow_sampler_1;

#line 2755
    (&kernelContext_15)->specular_albedo_0 = specular_albedo_1;

#line 2755
    (&kernelContext_15)->probes_0 = probes_1;

#line 2761
    float3 normal_4 = normalize(_S113.world_normal_0);

#line 2794
    if((frame_1->ambient_0.w) >= 3.5f)
    {

#line 2794
        float _S114 = occlusion_at_0(position_3.xy, &kernelContext_15);

        thread FragmentOutput_0 occlusion_0;

#line 2805
        (&occlusion_0)->lit_0 = float4(_S114, _S114, _S114, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return occlusion_0;
    }

    if((frame_1->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S113.color_2.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return tint_0;
    }

    if((frame_1->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 2825
        float3 _S115 = float3(0.5f) ;

#line 2832
        (&normals_0)->lit_0 = float4(normal_4 * _S115 + _S115, 1.0f);

#line 2838
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_15)->frame_0->camera_position_0.xyz - _S113.world_position_8);



    float3 _S116 = geometric_normal_of_0(_S113.world_position_8, normal_4);

#line 2846
    thread GpuMaterial_natural_0 _S117 = (&kernelContext_15)->materials_0[_S113.material_2];

#line 2846
    float2 uv_2;

#line 2865
    if(((&_S117)->tiling_0) == 1U)
    {

#line 2865
        uv_2 = physical_tile_uv_0(_S113.world_position_8, normal_4, (&_S117)->tile_metres_0);

#line 2865
    }
    else
    {

#line 2865
        uv_2 = _S113.uv_1;

#line 2865
    }

#line 2870
    float3 _S118 = float3(uv_2, float((&_S117)->base_color_texture_0));
    float4 albedo_0 = _S113.color_2 * float4((&_S117)->base_color_0)  * (((&kernelContext_15)->base_color_textures_0).sample(((&kernelContext_15)->base_color_sampler_0), ((_S118)).xy, uint(((_S118)).z)));

#line 2877
    float metallic_1 = saturate((&_S117)->metallic_0);
    float roughness_3 = clamp((&_S117)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_3 * roughness_3;
    float _S119 = alpha_0 * alpha_0;

#line 2886
    float3 _S120 = albedo_0.xyz;

#line 2886
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S120, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S120 * float3((1.0f - metallic_1)) ;

#line 2893
    float _S121 = max(dot(normal_4, to_eye_0), 0.00009999999747379f);

#line 2903
    float2 _S122 = position_3.xy;

#line 2903
    uint _S123 = froxel_of_0(_S122, (((float4(_S113.world_position_8, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_15);

#line 2903
    uint base_3 = _S123 * 17U;

#line 2908
    uint _S124 = min((&kernelContext_15)->cluster_lights_0[base_3], 16U);

#line 2914
    float3 _S125 = float3(0.0f, 0.0f, 0.0f);

#line 2914
    uint slot_0 = 0U;

#line 2914
    float3 direct_0 = _S125;

#line 2914
    float3 gloss_0 = _S125;

    for(;;)
    {

#line 2916
        if(slot_0 < _S124)
        {
        }
        else
        {

#line 2916
            break;
        }

#line 2916
        thread GpuLight_natural_0 _S126 = (&kernelContext_15)->lights_0[(&kernelContext_15)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 2916
        uint _S127 = (&_S126)->kind_0;

#line 2925
        bool _S128 = ((&_S126)->kind_0) == 0U;

#line 2925
        float3 to_light_7;

#line 2925
        float reach_0;

#line 2925
        if(_S128)
        {

#line 2925
            to_light_7 = normalize((float4((&_S126)->direction_0) ).xyz);

#line 2925
            reach_0 = 1.0f;

#line 2925
        }
        else
        {

#line 2925
            float4 _S129 = float4((&_S126)->position_1) ;

#line 2932
            float3 offset_0 = _S129.xyz - _S113.world_position_8;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S129.w);
            if(_S127 == 2U)
            {

#line 2936
                float4 _S130 = float4((&_S126)->direction_0) ;

#line 2936
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S130.xyz, _S130.w, (&_S126)->cos_inner_0);

#line 2936
            }
            else
            {

#line 2936
                reach_0 = reach_1;

#line 2936
            }

#line 2936
            to_light_7 = to_light_8;

#line 2925
        }

#line 2943
        float n_dot_l_5 = dot(normal_4, to_light_7);
        float _S131 = max(n_dot_l_5, 0.0f);

#line 2950
        float3 half_vector_0 = normalize(to_light_7 + to_eye_0);

#line 2957
        float3 specular_0 = ggx_lobe_0(_S119, f0_2, _S131, _S121, max(dot(normal_4, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S131) ;

#line 2957
        float reach_2;

#line 2972
        if(_S128)
        {

#line 2972
            float _S132 = sun_visibility_0(_S113.world_position_8, to_light_7, n_dot_l_5, _S116, _S122, &kernelContext_15);

#line 2972
            reach_2 = _S132;

#line 2972
        }
        else
        {


            if(_S127 == 1U)
            {

#line 2977
                uint _S133 = (&_S126)->shadow_tile_0;

#line 2989
                if(((&_S126)->shadow_tile_0) <= 8U)
                {

#line 2989
                    float _S134 = point_visibility_0(&_S126, _S133, _S113.world_position_8, to_light_7, n_dot_l_5, _S116, _S122, &kernelContext_15);

#line 2989
                    reach_2 = reach_0 * _S134;

#line 2989
                }
                else
                {

#line 2989
                    reach_2 = reach_0;

#line 2989
                }

#line 2977
            }
            else
            {

#line 2977
                uint _S135 = (&_S126)->shadow_tile_0;

#line 2995
                if(((&_S126)->shadow_tile_0) < 14U)
                {

#line 2995
                    float _S136 = spot_visibility_0(&_S126, _S135, _S113.world_position_8, to_light_7, n_dot_l_5, _S116, _S122, &kernelContext_15);

#line 2995
                    reach_2 = reach_0 * _S136;

#line 2995
                }
                else
                {

#line 2995
                    reach_2 = reach_0;

#line 2995
                }

#line 2977
            }

#line 2972
        }

#line 3003
        float3 _S137 = (float4((&_S126)->color_1) ).xyz;

#line 3003
        float3 direct_1 = direct_0 + _S137 * float3((_S131 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S137 * (specular_0 * float3(reach_2) );

#line 2916
        slot_0 = slot_0 + 1U;

#line 2916
        direct_0 = direct_1;

#line 2916
        gloss_0 = gloss_1;

#line 2916
    }

#line 2916
    float3 _S138 = specular_compensation_0(f0_2, _S121, roughness_3, &kernelContext_15);

#line 3018
    float3 gloss_2 = gloss_0 * _S138;

#line 3018
    float _S139 = occlusion_at_0(_S122, &kernelContext_15);

#line 3054
    float3 _S140 = frame_1->ambient_0.xyz;

#line 3054
    float3 _S141 = sky_irradiance_0(normal_4, &kernelContext_15);

#line 3054
    float3 _S142 = _S140 + _S141;

#line 3054
    float3 _S143 = probe_irradiance_0(_S113.world_position_8, normal_4, &kernelContext_15);

#line 3075
    float3 lit_1 = diffuse_albedo_0 * ((_S142 + _S143) * float3(_S139)  + direct_0) + gloss_2;

#line 3075
    float3 _S144 = emissive_of_0(&_S117);

#line 3111
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_15)->frame_0->fog_params_0.x, (&kernelContext_15)->frame_0->fog_params_0.y, (&kernelContext_15)->frame_0->camera_position_0.y - (&kernelContext_15)->frame_0->fog_params_0.z, _S113.world_position_8.y - (&kernelContext_15)->frame_0->fog_params_0.z, length((&kernelContext_15)->frame_0->camera_position_0.xyz - _S113.world_position_8)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S144) * float3(fog_survives_0)  + (&kernelContext_15)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);

#line 3123
    (&output_0)->reflectivity_0 = float4(f0_2, saturate(1.0f - roughness_3 / 0.5f));
    return output_0;
}


#line 3124
struct vertexMain_Result_0
{
    float4 position_4 [[position]];
    float3 world_position_9 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
};


#line 1113
struct VertexOutput_0
{
    float4 position_5;
    float3 world_position_10;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_4;
};


#line 1113
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_2 [[texture(3)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1113
    thread KernelContext_0 kernelContext_16;

#line 1113
    (&kernelContext_16)->draw_0 = draw_2;

#line 1113
    (&kernelContext_16)->visible_instances_0 = visible_instances_2;

#line 1113
    (&kernelContext_16)->instances_0 = instances_2;

#line 1113
    (&kernelContext_16)->meshes_0 = meshes_2;

#line 1113
    (&kernelContext_16)->vertices_0 = vertices_2;

#line 1113
    (&kernelContext_16)->frame_0 = frame_2;

#line 1113
    (&kernelContext_16)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1113
    (&kernelContext_16)->materials_0 = materials_2;

#line 1113
    (&kernelContext_16)->base_color_textures_0 = base_color_textures_2;

#line 1113
    (&kernelContext_16)->base_color_sampler_0 = base_color_sampler_2;

#line 1113
    (&kernelContext_16)->cluster_lights_0 = cluster_lights_2;

#line 1113
    (&kernelContext_16)->lights_0 = lights_2;

#line 1113
    (&kernelContext_16)->shadow_atlas_0 = shadow_atlas_2;

#line 1113
    (&kernelContext_16)->shadow_sampler_0 = shadow_sampler_2;

#line 1113
    (&kernelContext_16)->specular_albedo_0 = specular_albedo_2;

#line 1113
    (&kernelContext_16)->probes_0 = probes_2;

#line 1113
    GpuInstance_natural_0 device* _S145 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1159
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1159
    uint base_vertex_2;

#line 1168
    if(((_S145->flags_0) & 2U) != 0U)
    {

#line 1168
        base_vertex_2 = _S145->base_vertex_0;

#line 1168
    }
    else
    {

#line 1168
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1168
    }

    MeshVertex_natural_0 vertex_0 = (&kernelContext_16)->vertices_0[index_3 + base_vertex_2];

#line 1170
    matrix<float,int(4),int(4)>  _S146 = matrix<float,int(4),int(4)> (_S145->transform_0.data_0[int(0)][int(0)], _S145->transform_0.data_0[int(1)][int(0)], _S145->transform_0.data_0[int(2)][int(0)], _S145->transform_0.data_0[int(3)][int(0)], _S145->transform_0.data_0[int(0)][int(1)], _S145->transform_0.data_0[int(1)][int(1)], _S145->transform_0.data_0[int(2)][int(1)], _S145->transform_0.data_0[int(3)][int(1)], _S145->transform_0.data_0[int(0)][int(2)], _S145->transform_0.data_0[int(1)][int(2)], _S145->transform_0.data_0[int(2)][int(2)], _S145->transform_0.data_0[int(3)][int(2)], _S145->transform_0.data_0[int(0)][int(3)], _S145->transform_0.data_0[int(1)][int(3)], _S145->transform_0.data_0[int(2)][int(3)], _S145->transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S146)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_5 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_10 = world_0.xyz;

#line 1182
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (normal_basis_0(matrix<float,int(3),int(3)> (_S146[int(0)].xyz, _S146[int(1)].xyz, _S146[int(2)].xyz)))));

#line 1182
    float4 _S147;

#line 1189
    if(((&kernelContext_16)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1189
        _S147 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1189
    }
    else
    {

#line 1189
        _S147 = float4(vertex_0.color_0) ;

#line 1189
    }

#line 1188
    (&output_1)->color_4 = _S147;

#line 1195
    (&output_1)->material_4 = _S145->material_0;
    (&output_1)->uv_4 = (float4(vertex_0.uv_0) ).xy;
    VertexOutput_0 _S148 = output_1;

#line 1197
    thread vertexMain_Result_0 _S149;

#line 1197
    (&_S149)->position_4 = _S148.position_5;

#line 1197
    (&_S149)->world_position_9 = _S148.world_position_10;

#line 1197
    (&_S149)->world_normal_1 = _S148.world_normal_2;

#line 1197
    (&_S149)->color_3 = _S148.color_4;

#line 1197
    (&_S149)->material_3 = _S148.material_4;

#line 1197
    (&_S149)->uv_3 = _S148.uv_4;

#line 1197
    return _S149;
}

