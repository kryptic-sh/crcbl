#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 1513 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 1508
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1780
constant array<float2, int(32)> SHADOW_DISC_0 = { float2(0.125f, 0.0f), float2(-0.15964500606060028f, 0.14624799787998199f), float2(0.02443600073456764f, -0.27843800187110901f), float2(0.2012220025062561f, 0.26245900988578796f), float2(-0.36926800012588501f, -0.06531800329685211f), float2(0.34980198740959167f, -0.22251600027084351f), float2(-0.11700200289487839f, 0.43524199724197388f), float2(-0.22313599288463593f, -0.42963400483131409f), float2(0.48411500453948975f, 0.17679800093173981f), float2(-0.50364100933074951f, 0.20789599418640137f), float2(0.24278800189495087f, -0.51882398128509521f), float2(0.17941400408744812f, 0.57200098037719727f), float2(-0.54075700044631958f, -0.31338000297546387f), float2(0.63437002897262573f, -0.13946400582790375f), float2(-0.38714599609375f, 0.55067497491836548f), float2(-0.0894400030374527f, -0.69019997119903564f), float2(0.5490720272064209f, 0.46275800466537476f), float2(-0.73887801170349121f, 0.0305550005286932f), float2(0.5389549732208252f, -0.53633201122283936f), float2(-0.03605800122022629f, 0.77979201078414917f), float2(-0.51281797885894775f, -0.61452698707580566f), float2(0.81235998868942261f, 0.10930199921131134f), float2(-0.68831098079681396f, 0.47890898585319519f), float2(0.18808600306510925f, -0.83606100082397461f), float2(0.43503299355506897f, 0.75919097661972046f), float2(-0.85044801235198975f, -0.27131599187850952f), float2(0.82610201835632324f, -0.38168001174926758f), float2(-0.35788801312446594f, 0.85515600442886353f), float2(-0.31940698623657227f, -0.88803398609161377f), float2(0.84990900754928589f, 0.44668799638748169f), float2(-0.94403499364852905f, 0.24884499609470367f), float2(0.53659600019454956f, -0.83452999591827393f) };

#line 1840
constant array<uint, int(5)> SHADOW_PROBE_INDEX_0 = { 0U, 23U, 25U, 27U, 29U };

#line 1992
constant array<float2, int(16)> SHADOW_SEARCH_DISC_0 = { float2(0.17677700519561768f, 0.0f), float2(-0.22577199339866638f, 0.20682600140571594f), float2(0.0345579981803894f, -0.39377099275588989f), float2(0.28457099199295044f, 0.37117299437522888f), float2(-0.52222299575805664f, -0.09237399697303772f), float2(0.49469500780105591f, -0.31468498706817627f), float2(-0.16546599566936493f, 0.6155250072479248f), float2(-0.31556099653244019f, -0.60759401321411133f), float2(0.68464201688766479f, 0.25003001093864441f), float2(-0.71225601434707642f, 0.2940090000629425f), float2(0.3433539867401123f, -0.73372900485992432f), float2(0.25372999906539917f, 0.80893200635910034f), float2(-0.76474601030349731f, -0.44318601489067078f), float2(0.89713400602340698f, -0.19723199307918549f), float2(-0.54750698804855347f, 0.77877199649810791f), float2(-0.12648700177669525f, -0.97609001398086548f) };

#line 1855
constant array<float2, int(16)> SHADOW_ROTATIONS_0 = { float2(1.0f, 0.0f), float2(0.92387998104095459f, 0.38268300890922546f), float2(0.70710700750350952f, 0.70710700750350952f), float2(0.38268300890922546f, 0.92387998104095459f), float2(0.0f, 1.0f), float2(-0.38268300890922546f, 0.92387998104095459f), float2(-0.70710700750350952f, 0.70710700750350952f), float2(-0.92387998104095459f, 0.38268300890922546f), float2(-1.0f, 0.0f), float2(-0.92387998104095459f, -0.38268300890922546f), float2(-0.70710700750350952f, -0.70710700750350952f), float2(-0.38268300890922546f, -0.92387998104095459f), float2(-0.0f, -1.0f), float2(0.38268300890922546f, -0.92387998104095459f), float2(0.70710700750350952f, -0.70710700750350952f), float2(0.92387998104095459f, -0.38268300890922546f) };

#line 1883
constant array<uint, int(16)> SHADOW_DITHER_0 = { 0U, 8U, 2U, 10U, 12U, 4U, 14U, 6U, 3U, 11U, 1U, 9U, 15U, 7U, 13U, 5U };

#line 1166
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_0)
{
    return matrix<float,int(3),int(3)> (cross(basis_0[int(1)], basis_0[int(2)]), cross(basis_0[int(2)], basis_0[int(0)]), cross(basis_0[int(0)], basis_0[int(1)]));
}


#line 2905
float2 motion_vector_0(float4 current_0, float4 previous_0)
{
    float _S1 = previous_0.w;

#line 2907
    if(_S1 <= 0.0f)
    {
        return float2(0.0f, 0.0f);
    }
    return (current_0.xy / float2(current_0.w)  - previous_0.xy / float2(_S1) ) * float2(0.5f, -0.5f);
}


#line 868
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1235
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1235
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    _MatrixStorage_float4x4_ColMajornatural_0 previous_transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint previous_base_vertex_0;
    uint pad1_1;
    uint pad2_0;
};


#line 650
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


#line 1241
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 1241
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1241
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
    _MatrixStorage_float4x4_ColMajornatural_1 previous_view_proj_0;
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


#line 2873 "shaders/mesh.slang"
float occlusion_at_0(float2 position_2, KernelContext_0 thread* kernelContext_0)
{

#line 2873
    texture2d<float, access::sample> _S2 = kernelContext_0->ambient_occlusion_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S2).get_width(0)),(*((&height_0)) = (_S2).get_height(0));

    int3 _S3 = int3(min(int2(position_2), int2(int(width_0), int(height_0)) - int2(int(1)) ), int(0));

#line 2879
    return ((kernelContext_0->ambient_occlusion_0).read(vec<uint,2>(((_S3)).xy), uint(((_S3)).z)).x);
}


#line 1648
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1659
    float3 _S4;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1660
        _S4 = - facet_1;

#line 1660
    }
    else
    {

#line 1660
        _S4 = facet_1;

#line 1660
    }

#line 1660
    return _S4;
}


#line 2843
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_1, float tile_metres_1)
{
    float3 axis_0 = abs(normal_1);

    float _S5 = axis_0.x;

#line 2847
    float _S6 = axis_0.y;

#line 2847
    bool _S7;

#line 2847
    if(_S5 >= _S6)
    {

#line 2847
        _S7 = _S5 >= (axis_0.z);

#line 2847
    }
    else
    {

#line 2847
        _S7 = false;

#line 2847
    }

#line 2847
    float2 planar_0;

#line 2847
    if(_S7)
    {

#line 2847
        planar_0 = world_position_1.zy;

#line 2847
    }
    else
    {

        if(_S6 >= (axis_0.z))
        {

#line 2851
            planar_0 = world_position_1.xz;

#line 2851
        }
        else
        {

#line 2851
            planar_0 = world_position_1.xy;

#line 2851
        }

#line 2847
    }

#line 2859
    return planar_0 / float2(max(tile_metres_1, 0.00009999999747379f)) ;
}


#line 2641
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_1)
{
    uint _S8 = max(kernelContext_1->frame_0->cluster_grid_0.x, 1U);
    uint _S9 = max(kernelContext_1->frame_0->cluster_grid_0.y, 1U);
    uint _S10 = max(kernelContext_1->frame_0->cluster_grid_0.z, 1U);
    uint _S11 = max(kernelContext_1->frame_0->cluster_grid_0.w, 1U);

#line 2651
    uint _S12 = uint(pixel_0.x) / _S11;

#line 2651
    uint _S13 = min(_S12, _S8 - 1U);
    uint _S14 = uint(pixel_0.y) / _S11;

    float scale_0 = 24.0f / log2(10000.0f);

#line 2662
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S10 - 1U))) * _S9 + min(_S14, _S9 - 1U)) * _S8 + _S13;
}


#line 2606
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 2620
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 2627
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 1372
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 1379
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1386
    float _S15 = 1.0f - alpha2_0;

#line 1391
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S15 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S15 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 1699
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 2014
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


#line 2130
float sun_penumbra_texels_0(uint cascade_0, float2 tile_uv_1, float reference_0, float2 rotation_0, KernelContext_0 thread* kernelContext_2)
{
    float2 texel_0 = kernelContext_2->frame_0->shadow_params_0.xy;
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S16 = float2(0.5f, 0.5f) * texel_0 * grid_0;


    float2 _S17 = float2(1.0f, 1.0f);

#line 2137
    float2 _S18 = _S17 / texel_0;

#line 2137
    uint index_0 = 0U;

#line 2137
    float sum_0 = 0.0f;

#line 2137
    float found_0 = 0.0f;



    for(;;)
    {

#line 2141
        if(index_0 < 16U)
        {
        }
        else
        {

#line 2141
            break;
        }
        float2 spoke_0 = SHADOW_SEARCH_DISC_0[index_0] * float2(8.0f) ;
        float _S19 = spoke_0.x;

#line 2144
        float _S20 = rotation_0.x;

#line 2144
        float _S21 = spoke_0.y;

#line 2144
        float _S22 = rotation_0.y;

#line 2153
        int3 _S23 = int3(int2(min(atlas_uv_0(cascade_0, clamp(tile_uv_1 + float2(_S19 * _S20 - _S21 * _S22, _S19 * _S22 + _S21 * _S20) * texel_0 * grid_0, _S16, float2(1.0f)  - _S16)) * _S18, _S18 - _S17)), int(0));

#line 2153
        float depth_1 = ((kernelContext_2->shadow_atlas_0).read(vec<uint,2>(((_S23)).xy), uint(((_S23)).z)));
        if(depth_1 > reference_0)
        {

            float found_1 = found_0 + 1.0f;

#line 2157
            sum_0 = sum_0 + depth_1;

#line 2157
            found_0 = found_1;

#line 2154
        }

#line 2141
        index_0 = index_0 + 1U;

#line 2141
    }

#line 2161
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }

#line 2172
    float _S24 = 2.0f * kernelContext_2->frame_0->cascade_far_0[cascade_0];

    return clamp((sum_0 / found_0 - reference_0) * (_S24 + 40.0f) * 0.01999999955296516f / (_S24 / 768.0f), 2.0f, 8.0f);
}


#line 2032
float tile_tap_0(uint tile_1, float2 tile_uv_2, float2 spoke_1, float2 rotation_1, float reference_1, KernelContext_0 thread* kernelContext_3)
{
    float2 texel_1 = kernelContext_3->frame_0->shadow_params_0.xy;

#line 2039
    float2 grid_1 = float2(4.0f, 4.0f);
    float2 tile_min_0 = float2(0.5f, 0.5f) * texel_1 * grid_1;

    float _S25 = spoke_1.x;

#line 2042
    float _S26 = rotation_1.x;

#line 2042
    float _S27 = spoke_1.y;

#line 2042
    float _S28 = rotation_1.y;


    float _S29 = ((kernelContext_3->shadow_atlas_0).sample_compare((kernelContext_3->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_2 + float2(_S25 * _S26 - _S27 * _S28, _S25 * _S28 + _S27 * _S26) * texel_1 * grid_1, tile_min_0, float2(1.0f)  - tile_min_0))), (reference_1), level((0.0f))));

#line 2045
    return _S29;
}


#line 2067
float tile_pcf_0(uint tile_2, float2 tile_uv_3, float reference_2, float2 pixel_2, float radius_1, KernelContext_0 thread* kernelContext_4)
{
    float2 _S30 = shadow_rotation_0(pixel_2);

#line 2069
    uint spot_0 = 0U;

#line 2069
    float probe_0 = 0.0f;


    for(;;)
    {

#line 2072
        if(spot_0 < 5U)
        {
        }
        else
        {

#line 2072
            break;
        }

#line 2072
        float _S31 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * float2(radius_1) , _S30, reference_2, kernelContext_4);

        float probe_1 = probe_0 + _S31;

#line 2072
        spot_0 = spot_0 + 1U;

#line 2072
        probe_0 = probe_1;

#line 2072
    }

#line 2081
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }

#line 2087
    uint index_1 = 0U;

#line 2087
    float visibility_0 = 0.0f;



    for(;;)
    {

#line 2091
        if(index_1 < 32U)
        {
        }
        else
        {

#line 2091
            break;
        }

#line 2091
        float _S32 = tile_tap_0(tile_2, tile_uv_3, SHADOW_DISC_0[index_1] * float2(radius_1) , _S30, reference_2, kernelContext_4);

        float visibility_1 = visibility_0 + _S32;

#line 2091
        index_1 = index_1 + 1U;

#line 2091
        visibility_0 = visibility_1;

#line 2091
    }



    return visibility_0 / 32.0f;
}


#line 2226
float cascade_visibility_0(uint cascade_1, float3 world_position_2, float3 to_light_2, float3 geometric_normal_1, float2 pixel_3, KernelContext_0 thread* kernelContext_5)
{

#line 2257
    float texel_world_0 = 2.0f * kernelContext_5->frame_0->cascade_far_0[cascade_1] / 768.0f;

#line 2264
    float4 clip_0 = (((float4(world_position_2 + geometric_normal_1 * float3((texel_world_0 * kernelContext_5->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_5->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(0)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(1)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(2)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(0)][int(3)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(1)][int(3)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(2)][int(3)], (&kernelContext_5->frame_0->shadow_view_proj_0)->data_2[cascade_1].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 2268
    bool _S33;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 2269
        _S33 = true;

#line 2269
    }
    else
    {

#line 2269
        _S33 = (ndc_0.z) <= 0.0f;

#line 2269
    }

#line 2269
    if(_S33)
    {



        return 1.0f;
    }



    float2 tile_uv_4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);

#line 2296
    float _S34 = ndc_0.z;

#line 2296
    float _S35 = sun_penumbra_texels_0(cascade_1, tile_uv_4, _S34, shadow_rotation_0(pixel_3), kernelContext_5);

#line 2296
    float _S36 = tile_pcf_0(cascade_1, tile_uv_4, _S34, pixel_3, _S35, kernelContext_5);
    return _S36;
}


#line 2313
float sun_visibility_0(float3 world_position_3, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, float2 pixel_4, KernelContext_0 thread* kernelContext_6)
{

#line 2314
    uint cascade_2;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 2326
    float eye_distance_0 = length(world_position_3 - kernelContext_6->frame_0->camera_position_0.xyz);

#line 2326
    uint index_2 = 0U;

    for(;;)
    {

#line 2328
        if(index_2 < 2U)
        {
        }
        else
        {

#line 2328
            cascade_2 = 1U;

#line 2328
            break;
        }
        if(eye_distance_0 < kernelContext_6->frame_0->cascade_far_0[index_2])
        {

#line 2330
            cascade_2 = index_2;


            break;
        }

#line 2328
        index_2 = index_2 + 1U;

#line 2328
    }

#line 2328
    float _S37 = cascade_visibility_0(cascade_2, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_6);

#line 2339
    uint _S38 = cascade_2 + 1U;

#line 2339
    if(_S38 >= 2U)
    {



        return _S37;
    }

#line 2352
    float band_0 = kernelContext_6->frame_0->cascade_far_0[cascade_2] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_6->frame_0->cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S37;
    }

#line 2356
    float _S39 = cascade_visibility_0(_S38, world_position_3, to_light_3, geometric_normal_2, pixel_4, kernelContext_6);

#line 2367
    return mix(_S37, _S39, blend_0);
}


#line 2557
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S40 = axis_2.x;

#line 2560
    float _S41 = axis_2.y;

#line 2560
    bool _S42;

#line 2560
    if(_S40 >= _S41)
    {

#line 2560
        _S42 = _S40 >= (axis_2.z);

#line 2560
    }
    else
    {

#line 2560
        _S42 = false;

#line 2560
    }

#line 2560
    uint _S43;

#line 2560
    if(_S42)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 2562
            _S43 = 0U;

#line 2562
        }
        else
        {

#line 2562
            _S43 = 1U;

#line 2562
        }

#line 2562
        return _S43;
    }
    if(_S41 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 2566
            _S43 = 2U;

#line 2566
        }
        else
        {

#line 2566
            _S43 = 3U;

#line 2566
        }

#line 2566
        return _S43;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 2568
        _S43 = 4U;

#line 2568
    }
    else
    {

#line 2568
        _S43 = 5U;

#line 2568
    }

#line 2568
    return _S43;
}


#line 225
uint light_tile_0(uint tile_3)
{
    return 2U + tile_3;
}


#line 2470
float punctual_visibility_0(uint tile_4, float3 world_position_4, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, float2 pixel_5, KernelContext_0 thread* kernelContext_7)
{

#line 2482
    float4 clip_1 = (((float4(world_position_4 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(0)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(1)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(2)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(0)][int(3)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(1)][int(3)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(2)][int(3)], (&kernelContext_7->frame_0->light_view_proj_0)->data_3[tile_4].data_1[int(3)][int(3)]))));

#line 2489
    float _S44 = clip_1.w;

#line 2489
    if(_S44 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S44) ;

#line 2493
    bool _S45;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 2494
        _S45 = true;

#line 2494
    }
    else
    {

#line 2494
        _S45 = (ndc_1.z) <= 0.0f;

#line 2494
    }

#line 2494
    if(_S45)
    {

#line 2494
        _S45 = true;

#line 2494
    }
    else
    {

#line 2494
        _S45 = (ndc_1.z) > 1.0f;

#line 2494
    }

#line 2494
    if(_S45)
    {

#line 2501
        return 1.0f;
    }

#line 2501
    float _S46 = tile_pcf_0(light_tile_0(tile_4), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f, kernelContext_7);

#line 2511
    return _S46;
}


#line 2576
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_5, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, float2 pixel_6, KernelContext_0 thread* kernelContext_8)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_5 - (float4(light_0->position_1) ).xyz;

#line 2584
    float _S47 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_5, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, pixel_6, kernelContext_8);

#line 2590
    return _S47;
}


#line 2518
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_5, float3 world_position_6, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, float2 pixel_7, KernelContext_0 thread* kernelContext_9)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 2525
    float4 _S48 = float4(light_1->direction_0) ;

#line 2532
    float cos_outer_1 = _S48.w;

#line 2532
    float _S49 = punctual_visibility_0(tile_5, world_position_6, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_6 - (float4(light_1->position_1) ).xyz, normalize(_S48.xyz)), 0.0f) / 768.0f, geometric_normal_5, pixel_7, kernelContext_9);

#line 2539
    return _S49;
}


#line 1416
float decode_specular_albedo_0(float2 texel_2)
{
    return (texel_2.x * 65280.0f + texel_2.y * 255.0f) / 65535.0f;
}


#line 1433
float specular_albedo_at_0(float n_dot_v_1, float roughness_1, KernelContext_0 thread* kernelContext_10)
{

#line 1433
    texture2d<float, access::sample> _S50 = kernelContext_10->specular_albedo_0;

    thread uint width_1;
    thread uint height_1;
    (*((&width_1)) = (_S50).get_width(0)),(*((&height_1)) = (_S50).get_height(0));
    float2 extent_1 = float2(float(width_1), float(height_1));
    float2 scaled_0 = float2(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1439
    float2 _S51 = float2(1.0f) ;
    float2 _S52 = extent_1 - _S51;

#line 1440
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S52);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S53 = int2(low_0);
    int2 _S54 = int2(min(low_0 + _S51, _S52));
    int _S55 = _S53.x;

#line 1446
    int _S56 = _S53.y;

#line 1446
    int3 _S57 = int3(_S55, _S56, int(0));
    int _S58 = _S54.x;

#line 1447
    int3 _S59 = int3(_S58, _S56, int(0));
    float _S60 = weight_0.x;
    int _S61 = _S54.y;

#line 1449
    int3 _S62 = int3(_S55, _S61, int(0));
    int3 _S63 = int3(_S58, _S61, int(0));

    return mix(mix(decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S57)).xy), uint(((_S57)).z)).xy)), decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S59)).xy), uint(((_S59)).z)).xy)), _S60), mix(decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S62)).xy), uint(((_S62)).z)).xy)), decode_specular_albedo_0(((kernelContext_10->specular_albedo_0).read(vec<uint,2>(((_S63)).xy), uint(((_S63)).z)).xy)), _S60), weight_0.y);
}


#line 1471
float3 specular_compensation_0(float3 f0_1, float n_dot_v_2, float roughness_2, KernelContext_0 thread* kernelContext_11)
{

#line 1471
    float _S64 = specular_albedo_at_0(n_dot_v_2, roughness_2, kernelContext_11);



    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(_S64, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 2770
float3 sky_irradiance_0(float3 normal_2, KernelContext_0 thread* kernelContext_12)
{
    float4 basis_1 = float4(normal_2, 1.0f);
    return max(float3(dot(kernelContext_12->frame_0->sky_sh_r_0, basis_1), dot(kernelContext_12->frame_0->sky_sh_g_0, basis_1), dot(kernelContext_12->frame_0->sky_sh_b_0, basis_1)), float3(0.0f, 0.0f, 0.0f));
}


#line 779
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 2672
GpuProbe_0 probe_at_0(uint3 cell_1, KernelContext_0 thread* kernelContext_13)
{

    GpuProbe_natural_0 _S65 = kernelContext_13->probes_0[min((cell_1.z * kernelContext_13->frame_0->probe_counts_0.y + cell_1.y) * kernelContext_13->frame_0->probe_counts_0.x + cell_1.x, max(kernelContext_13->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 2675
    GpuProbe_0 _S66 = { float4(_S65.sh_r_0) , float4(_S65.sh_g_0) , float4(_S65.sh_b_0)  };

#line 2675
    return _S66;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_0)
{
    thread GpuProbe_0 blended_0;
    float4 _S67 = float4(t_0) ;

#line 2683
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S67);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S67);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S67);
    return blended_0;
}


#line 2723
float3 probe_irradiance_0(float3 world_position_7, float3 normal_3, KernelContext_0 thread* kernelContext_14)
{

#line 2723
    float3 _S68 = float3(1.0f) ;

#line 2728
    float3 _S69 = float3(0.0f, 0.0f, 0.0f);

#line 2728
    float3 last_0 = max(float3(kernelContext_14->frame_0->probe_counts_0.xyz) - _S68, _S69);
    float3 grid_2 = clamp((world_position_7 - kernelContext_14->frame_0->probe_origin_0.xyz) * kernelContext_14->frame_0->probe_inv_spacing_0.xyz, _S69, last_0);

    float3 base_2 = floor(grid_2);
    float3 f_0 = grid_2 - base_2;

    uint3 _S70 = uint3(base_2);



    uint3 _S71 = uint3(min(base_2 + _S68, last_0));

#line 2745
    uint _S72 = _S70.x;

#line 2745
    uint _S73 = _S70.y;

#line 2745
    uint _S74 = _S70.z;

#line 2745
    GpuProbe_0 _S75 = probe_at_0(uint3(_S72, _S73, _S74), kernelContext_14);

#line 2745
    uint _S76 = _S71.x;

#line 2745
    GpuProbe_0 _S77 = probe_at_0(uint3(_S76, _S73, _S74), kernelContext_14);

#line 2745
    float _S78 = f_0.x;

#line 2745
    thread GpuProbe_0 _S79 = _S75;

#line 2745
    thread GpuProbe_0 _S80 = _S77;

#line 2745
    GpuProbe_0 _S81 = lerp_probe_0(&_S79, &_S80, _S78);
    uint _S82 = _S71.y;

#line 2746
    GpuProbe_0 _S83 = probe_at_0(uint3(_S72, _S82, _S74), kernelContext_14);

#line 2746
    GpuProbe_0 _S84 = probe_at_0(uint3(_S76, _S82, _S74), kernelContext_14);

#line 2746
    thread GpuProbe_0 _S85 = _S83;

#line 2746
    thread GpuProbe_0 _S86 = _S84;

#line 2746
    GpuProbe_0 _S87 = lerp_probe_0(&_S85, &_S86, _S78);
    uint _S88 = _S71.z;

#line 2747
    GpuProbe_0 _S89 = probe_at_0(uint3(_S72, _S73, _S88), kernelContext_14);

#line 2747
    GpuProbe_0 _S90 = probe_at_0(uint3(_S76, _S73, _S88), kernelContext_14);

#line 2747
    thread GpuProbe_0 _S91 = _S89;

#line 2747
    thread GpuProbe_0 _S92 = _S90;

#line 2747
    GpuProbe_0 _S93 = lerp_probe_0(&_S91, &_S92, _S78);

#line 2747
    GpuProbe_0 _S94 = probe_at_0(uint3(_S72, _S82, _S88), kernelContext_14);

#line 2747
    GpuProbe_0 _S95 = probe_at_0(uint3(_S76, _S82, _S88), kernelContext_14);

#line 2747
    thread GpuProbe_0 _S96 = _S94;

#line 2747
    thread GpuProbe_0 _S97 = _S95;

#line 2747
    GpuProbe_0 _S98 = lerp_probe_0(&_S96, &_S97, _S78);

    float _S99 = f_0.y;

#line 2749
    thread GpuProbe_0 _S100 = _S81;

#line 2749
    thread GpuProbe_0 _S101 = _S87;

#line 2749
    GpuProbe_0 _S102 = lerp_probe_0(&_S100, &_S101, _S99);

#line 2749
    thread GpuProbe_0 _S103 = _S93;

#line 2749
    thread GpuProbe_0 _S104 = _S98;

#line 2749
    GpuProbe_0 _S105 = lerp_probe_0(&_S103, &_S104, _S99);

    float _S106 = f_0.z;

#line 2751
    thread GpuProbe_0 _S107 = _S102;

#line 2751
    thread GpuProbe_0 _S108 = _S105;

#line 2751
    GpuProbe_0 _S109 = lerp_probe_0(&_S107, &_S108, _S106);

    float4 basis_2 = float4(normal_3, 1.0f);
    return max(float3(dot(_S109.sh_r_0, basis_2), dot(_S109.sh_g_0, basis_2), dot(_S109.sh_b_0, basis_2)), _S69);
}


#line 752
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_1)
{
    return float3(material_1->emissive_r_0, material_1->emissive_g_0, material_1->emissive_b_0);
}


#line 1533
float fog_exp_neg_0(float x_0)
{
    float clamped_0 = clamp(x_0, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S110 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 1541
    float kernel_0 = 0.0001984127011383f;

#line 1541
    int term_0 = int(6);

    for(;;)
    {

#line 1543
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 1543
            break;
        }
        float _S111 = kernel_0 * _S110 + FOG_KERNEL_0[term_0];

#line 1543
        int term_1 = term_0 - int(1);

#line 1543
        kernel_0 = _S111;

#line 1543
        term_0 = term_1;

#line 1543
    }

#line 1550
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 1560
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S112 = - d_0;

#line 1564
        float series_0 = 0.00833333376795053f;

#line 1564
        int term_2 = int(3);

        for(;;)
        {

#line 1566
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 1566
                break;
            }
            float _S113 = series_0 * _S112 + FOG_RATIO_KERNEL_0[term_2];

#line 1566
            int term_3 = term_2 - int(1);

#line 1566
            series_0 = _S113;

#line 1566
            term_2 = term_3;

#line 1566
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 1594
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_1)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_1, 0.0f, 32.0f);
    }

#line 1605
    return clamp(density_0 * distance_1 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 1613
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 2796
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
    float2 motion_0 [[color(2)]];
};


#line 2796
struct pixelInput_0
{
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_2 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
    float4 clip_position_0 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_0 [[user(TEXCOORD_3)]];
};


#line 2915
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S114 [[stage_in]], float4 position_3 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_1 [[texture(3)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 2915
    thread KernelContext_0 kernelContext_15;

#line 2915
    (&kernelContext_15)->draw_0 = draw_1;

#line 2915
    (&kernelContext_15)->visible_instances_0 = visible_instances_1;

#line 2915
    (&kernelContext_15)->instances_0 = instances_1;

#line 2915
    (&kernelContext_15)->meshes_0 = meshes_1;

#line 2915
    (&kernelContext_15)->vertices_0 = vertices_1;

#line 2915
    (&kernelContext_15)->frame_0 = frame_1;

#line 2915
    (&kernelContext_15)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2915
    (&kernelContext_15)->materials_0 = materials_1;

#line 2915
    (&kernelContext_15)->base_color_textures_0 = base_color_textures_1;

#line 2915
    (&kernelContext_15)->base_color_sampler_0 = base_color_sampler_1;

#line 2915
    (&kernelContext_15)->cluster_lights_0 = cluster_lights_1;

#line 2915
    (&kernelContext_15)->lights_0 = lights_1;

#line 2915
    (&kernelContext_15)->shadow_atlas_0 = shadow_atlas_1;

#line 2915
    (&kernelContext_15)->shadow_sampler_0 = shadow_sampler_1;

#line 2915
    (&kernelContext_15)->specular_albedo_0 = specular_albedo_1;

#line 2915
    (&kernelContext_15)->probes_0 = probes_1;

#line 2921
    float3 normal_4 = normalize(_S114.world_normal_0);

#line 2926
    float2 motion_1 = motion_vector_0(_S114.clip_position_0, _S114.previous_clip_position_0);

#line 2935
    if((frame_1->ambient_0.w) >= 4.5f)
    {
        thread FragmentOutput_0 moved_0;
        (&moved_0)->lit_0 = float4(motion_1 * float2(8.0f)  + float2(0.5f) , 0.0f, 1.0f);


        (&moved_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&moved_0)->motion_0 = motion_1;
        return moved_0;
    }

#line 2977
    if((frame_1->ambient_0.w) >= 3.5f)
    {

#line 2977
        float _S115 = occlusion_at_0(position_3.xy, &kernelContext_15);

        thread FragmentOutput_0 occlusion_0;

#line 2988
        (&occlusion_0)->lit_0 = float4(_S115, _S115, _S115, 1.0f);


        (&occlusion_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&occlusion_0)->motion_0 = motion_1;
        return occlusion_0;
    }

    if((frame_1->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S114.color_2.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&tint_0)->motion_0 = motion_1;
        return tint_0;
    }

    if((frame_1->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 3010
        float3 _S116 = float3(0.5f) ;

#line 3017
        (&normals_0)->lit_0 = float4(normal_4 * _S116 + _S116, 1.0f);

#line 3023
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 1.0f);
        (&normals_0)->motion_0 = motion_1;
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_15)->frame_0->camera_position_0.xyz - _S114.world_position_8);



    float3 _S117 = geometric_normal_of_0(_S114.world_position_8, normal_4);

#line 3032
    thread GpuMaterial_natural_0 _S118 = (&kernelContext_15)->materials_0[_S114.material_2];

#line 3032
    float2 uv_2;

#line 3051
    if(((&_S118)->tiling_0) == 1U)
    {

#line 3051
        uv_2 = physical_tile_uv_0(_S114.world_position_8, normal_4, (&_S118)->tile_metres_0);

#line 3051
    }
    else
    {

#line 3051
        uv_2 = _S114.uv_1;

#line 3051
    }

#line 3056
    float3 _S119 = float3(uv_2, float((&_S118)->base_color_texture_0));
    float4 albedo_0 = _S114.color_2 * float4((&_S118)->base_color_0)  * (((&kernelContext_15)->base_color_textures_0).sample(((&kernelContext_15)->base_color_sampler_0), ((_S119)).xy, uint(((_S119)).z)));

#line 3063
    float metallic_1 = saturate((&_S118)->metallic_0);
    float roughness_3 = clamp((&_S118)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_3 * roughness_3;
    float _S120 = alpha_0 * alpha_0;

#line 3072
    float3 _S121 = albedo_0.xyz;

#line 3072
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S121, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S121 * float3((1.0f - metallic_1)) ;

#line 3079
    float _S122 = max(dot(normal_4, to_eye_0), 0.00009999999747379f);

#line 3089
    float2 _S123 = position_3.xy;

#line 3089
    uint _S124 = froxel_of_0(_S123, (((float4(_S114.world_position_8, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_15)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_15);

#line 3089
    uint base_3 = _S124 * 17U;

#line 3094
    uint _S125 = min((&kernelContext_15)->cluster_lights_0[base_3], 16U);

#line 3100
    float3 _S126 = float3(0.0f, 0.0f, 0.0f);

#line 3100
    uint slot_0 = 0U;

#line 3100
    float3 direct_0 = _S126;

#line 3100
    float3 gloss_0 = _S126;

    for(;;)
    {

#line 3102
        if(slot_0 < _S125)
        {
        }
        else
        {

#line 3102
            break;
        }

#line 3102
        thread GpuLight_natural_0 _S127 = (&kernelContext_15)->lights_0[(&kernelContext_15)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 3102
        uint _S128 = (&_S127)->kind_0;

#line 3111
        bool _S129 = ((&_S127)->kind_0) == 0U;

#line 3111
        float3 to_light_7;

#line 3111
        float reach_0;

#line 3111
        if(_S129)
        {

#line 3111
            to_light_7 = normalize((float4((&_S127)->direction_0) ).xyz);

#line 3111
            reach_0 = 1.0f;

#line 3111
        }
        else
        {

#line 3111
            float4 _S130 = float4((&_S127)->position_1) ;

#line 3118
            float3 offset_0 = _S130.xyz - _S114.world_position_8;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S130.w);
            if(_S128 == 2U)
            {

#line 3122
                float4 _S131 = float4((&_S127)->direction_0) ;

#line 3122
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S131.xyz, _S131.w, (&_S127)->cos_inner_0);

#line 3122
            }
            else
            {

#line 3122
                reach_0 = reach_1;

#line 3122
            }

#line 3122
            to_light_7 = to_light_8;

#line 3111
        }

#line 3129
        float n_dot_l_5 = dot(normal_4, to_light_7);
        float _S132 = max(n_dot_l_5, 0.0f);

#line 3136
        float3 half_vector_0 = normalize(to_light_7 + to_eye_0);

#line 3143
        float3 specular_0 = ggx_lobe_0(_S120, f0_2, _S132, _S122, max(dot(normal_4, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S132) ;

#line 3143
        float reach_2;

#line 3158
        if(_S129)
        {

#line 3158
            float _S133 = sun_visibility_0(_S114.world_position_8, to_light_7, n_dot_l_5, _S117, _S123, &kernelContext_15);

#line 3158
            reach_2 = _S133;

#line 3158
        }
        else
        {


            if(_S128 == 1U)
            {

#line 3163
                uint _S134 = (&_S127)->shadow_tile_0;

#line 3175
                if(((&_S127)->shadow_tile_0) <= 8U)
                {

#line 3175
                    float _S135 = point_visibility_0(&_S127, _S134, _S114.world_position_8, to_light_7, n_dot_l_5, _S117, _S123, &kernelContext_15);

#line 3175
                    reach_2 = reach_0 * _S135;

#line 3175
                }
                else
                {

#line 3175
                    reach_2 = reach_0;

#line 3175
                }

#line 3163
            }
            else
            {

#line 3163
                uint _S136 = (&_S127)->shadow_tile_0;

#line 3181
                if(((&_S127)->shadow_tile_0) < 14U)
                {

#line 3181
                    float _S137 = spot_visibility_0(&_S127, _S136, _S114.world_position_8, to_light_7, n_dot_l_5, _S117, _S123, &kernelContext_15);

#line 3181
                    reach_2 = reach_0 * _S137;

#line 3181
                }
                else
                {

#line 3181
                    reach_2 = reach_0;

#line 3181
                }

#line 3163
            }

#line 3158
        }

#line 3189
        float3 _S138 = (float4((&_S127)->color_1) ).xyz;

#line 3189
        float3 direct_1 = direct_0 + _S138 * float3((_S132 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S138 * (specular_0 * float3(reach_2) );

#line 3102
        slot_0 = slot_0 + 1U;

#line 3102
        direct_0 = direct_1;

#line 3102
        gloss_0 = gloss_1;

#line 3102
    }

#line 3102
    float3 _S139 = specular_compensation_0(f0_2, _S122, roughness_3, &kernelContext_15);

#line 3204
    float3 gloss_2 = gloss_0 * _S139;

#line 3204
    float _S140 = occlusion_at_0(_S123, &kernelContext_15);

#line 3240
    float3 _S141 = frame_1->ambient_0.xyz;

#line 3240
    float3 _S142 = sky_irradiance_0(normal_4, &kernelContext_15);

#line 3240
    float3 _S143 = _S141 + _S142;

#line 3240
    float3 _S144 = probe_irradiance_0(_S114.world_position_8, normal_4, &kernelContext_15);

#line 3261
    float3 lit_1 = diffuse_albedo_0 * ((_S143 + _S144) * float3(_S140)  + direct_0) + gloss_2;

#line 3261
    float3 _S145 = emissive_of_0(&_S118);

#line 3297
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_15)->frame_0->fog_params_0.x, (&kernelContext_15)->frame_0->fog_params_0.y, (&kernelContext_15)->frame_0->camera_position_0.y - (&kernelContext_15)->frame_0->fog_params_0.z, _S114.world_position_8.y - (&kernelContext_15)->frame_0->fog_params_0.z, length((&kernelContext_15)->frame_0->camera_position_0.xyz - _S114.world_position_8)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S145) * float3(fog_survives_0)  + (&kernelContext_15)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);


    (&output_0)->reflectivity_0 = float4(f0_2, floor(roughness_3 * 255.0f + 0.5f) / 255.0f);

    (&output_0)->motion_0 = motion_1;
    return output_0;
}


#line 3310
struct vertexMain_Result_0
{
    float4 position_4 [[position]];
    float3 world_position_9 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
    float4 clip_position_1 [[user(TEXCOORD_2)]];
    float4 previous_clip_position_1 [[user(TEXCOORD_3)]];
};


#line 1173
struct VertexOutput_0
{
    float4 position_5;
    float3 world_position_10;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_4;
    float4 clip_position_2;
    float4 previous_clip_position_2;
};


#line 1173
[[vertex]] vertexMain_Result_0 vertexMain(uint index_3 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_2 [[texture(3)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1173
    thread KernelContext_0 kernelContext_16;

#line 1173
    (&kernelContext_16)->draw_0 = draw_2;

#line 1173
    (&kernelContext_16)->visible_instances_0 = visible_instances_2;

#line 1173
    (&kernelContext_16)->instances_0 = instances_2;

#line 1173
    (&kernelContext_16)->meshes_0 = meshes_2;

#line 1173
    (&kernelContext_16)->vertices_0 = vertices_2;

#line 1173
    (&kernelContext_16)->frame_0 = frame_2;

#line 1173
    (&kernelContext_16)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1173
    (&kernelContext_16)->materials_0 = materials_2;

#line 1173
    (&kernelContext_16)->base_color_textures_0 = base_color_textures_2;

#line 1173
    (&kernelContext_16)->base_color_sampler_0 = base_color_sampler_2;

#line 1173
    (&kernelContext_16)->cluster_lights_0 = cluster_lights_2;

#line 1173
    (&kernelContext_16)->lights_0 = lights_2;

#line 1173
    (&kernelContext_16)->shadow_atlas_0 = shadow_atlas_2;

#line 1173
    (&kernelContext_16)->shadow_sampler_0 = shadow_sampler_2;

#line 1173
    (&kernelContext_16)->specular_albedo_0 = specular_albedo_2;

#line 1173
    (&kernelContext_16)->probes_0 = probes_2;

#line 1173
    GpuInstance_natural_0 device* _S146 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1241
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1249
    bool _S147 = ((_S146->flags_0) & 2U) != 0U;

#line 1249
    uint base_vertex_2;
    if(_S147)
    {

#line 1250
        base_vertex_2 = _S146->base_vertex_0;

#line 1250
    }
    else
    {

#line 1250
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1250
    }

    MeshVertex_natural_0 vertex_0 = (&kernelContext_16)->vertices_0[index_3 + base_vertex_2];

#line 1252
    uint previous_base_0;

#line 1261
    if(_S147)
    {

#line 1261
        previous_base_0 = _S146->previous_base_vertex_0;

#line 1261
    }
    else
    {

#line 1261
        previous_base_0 = base_vertex_2;

#line 1261
    }

    float3 previous_position_0 = (float4((&kernelContext_16)->vertices_0[index_3 + previous_base_0].position_0) ).xyz;

#line 1263
    matrix<float,int(4),int(4)>  _S148 = matrix<float,int(4),int(4)> (_S146->transform_0.data_0[int(0)][int(0)], _S146->transform_0.data_0[int(1)][int(0)], _S146->transform_0.data_0[int(2)][int(0)], _S146->transform_0.data_0[int(3)][int(0)], _S146->transform_0.data_0[int(0)][int(1)], _S146->transform_0.data_0[int(1)][int(1)], _S146->transform_0.data_0[int(2)][int(1)], _S146->transform_0.data_0[int(3)][int(1)], _S146->transform_0.data_0[int(0)][int(2)], _S146->transform_0.data_0[int(1)][int(2)], _S146->transform_0.data_0[int(2)][int(2)], _S146->transform_0.data_0[int(3)][int(2)], _S146->transform_0.data_0[int(0)][int(3)], _S146->transform_0.data_0[int(1)][int(3)], _S146->transform_0.data_0[int(2)][int(3)], _S146->transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S148)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_5 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_16)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_10 = world_0.xyz;

#line 1275
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (normal_basis_0(matrix<float,int(3),int(3)> (_S148[int(0)].xyz, _S148[int(1)].xyz, _S148[int(2)].xyz)))));

#line 1275
    float4 _S149;

#line 1282
    if(((&kernelContext_16)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1282
        _S149 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1282
    }
    else
    {

#line 1282
        _S149 = float4(vertex_0.color_0) ;

#line 1282
    }

#line 1281
    (&output_1)->color_4 = _S149;

#line 1288
    (&output_1)->material_4 = _S146->material_0;
    (&output_1)->uv_4 = (float4(vertex_0.uv_0) ).xy;

#line 1295
    (&output_1)->clip_position_2 = (&output_1)->position_5;
    (&output_1)->previous_clip_position_2 = ((((((float4(previous_position_0, 1.0f)) * (matrix<float,int(4),int(4)> (_S146->previous_transform_0.data_0[int(0)][int(0)], _S146->previous_transform_0.data_0[int(1)][int(0)], _S146->previous_transform_0.data_0[int(2)][int(0)], _S146->previous_transform_0.data_0[int(3)][int(0)], _S146->previous_transform_0.data_0[int(0)][int(1)], _S146->previous_transform_0.data_0[int(1)][int(1)], _S146->previous_transform_0.data_0[int(2)][int(1)], _S146->previous_transform_0.data_0[int(3)][int(1)], _S146->previous_transform_0.data_0[int(0)][int(2)], _S146->previous_transform_0.data_0[int(1)][int(2)], _S146->previous_transform_0.data_0[int(2)][int(2)], _S146->previous_transform_0.data_0[int(3)][int(2)], _S146->previous_transform_0.data_0[int(0)][int(3)], _S146->previous_transform_0.data_0[int(1)][int(3)], _S146->previous_transform_0.data_0[int(2)][int(3)], _S146->previous_transform_0.data_0[int(3)][int(3)]))))) * (matrix<float,int(4),int(4)> ((&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(0)][int(0)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(1)][int(0)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(2)][int(0)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(3)][int(0)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(0)][int(1)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(1)][int(1)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(2)][int(1)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(3)][int(1)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(0)][int(2)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(1)][int(2)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(2)][int(2)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(3)][int(2)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(0)][int(3)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(1)][int(3)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(2)][int(3)], (&kernelContext_16)->frame_0->previous_view_proj_0.data_1[int(3)][int(3)]))));


    VertexOutput_0 _S150 = output_1;

#line 1299
    thread vertexMain_Result_0 _S151;

#line 1299
    (&_S151)->position_4 = _S150.position_5;

#line 1299
    (&_S151)->world_position_9 = _S150.world_position_10;

#line 1299
    (&_S151)->world_normal_1 = _S150.world_normal_2;

#line 1299
    (&_S151)->color_3 = _S150.color_4;

#line 1299
    (&_S151)->material_3 = _S150.material_4;

#line 1299
    (&_S151)->uv_3 = _S150.uv_4;

#line 1299
    (&_S151)->clip_position_1 = _S150.clip_position_2;

#line 1299
    (&_S151)->previous_clip_position_1 = _S150.previous_clip_position_2;

#line 1299
    return _S151;
}

