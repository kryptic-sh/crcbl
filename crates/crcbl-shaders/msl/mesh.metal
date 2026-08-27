#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 1380 "shaders/mesh.slang"
constant array<float, int(5)> FOG_RATIO_KERNEL_0 = { 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f };

#line 1375
constant array<float, int(8)> FOG_KERNEL_0 = { 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f };

#line 1089
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_0)
{
    return matrix<float,int(3),int(3)> (cross(basis_0[int(1)], basis_0[int(2)]), cross(basis_0[int(2)], basis_0[int(0)]), cross(basis_0[int(0)], basis_0[int(1)]));
}


#line 1515
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1526
    float3 _S1;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1527
        _S1 = - facet_1;

#line 1527
    }
    else
    {

#line 1527
        _S1 = facet_1;

#line 1527
    }

#line 1527
    return _S1;
}


#line 2241
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_0, float tile_metres_0)
{
    float3 axis_0 = abs(normal_0);

    float _S2 = axis_0.x;

#line 2245
    float _S3 = axis_0.y;

#line 2245
    bool _S4;

#line 2245
    if(_S2 >= _S3)
    {

#line 2245
        _S4 = _S2 >= (axis_0.z);

#line 2245
    }
    else
    {

#line 2245
        _S4 = false;

#line 2245
    }

#line 2245
    float2 planar_0;

#line 2245
    if(_S4)
    {

#line 2245
        planar_0 = world_position_1.zy;

#line 2245
    }
    else
    {

        if(_S3 >= (axis_0.z))
        {

#line 2249
            planar_0 = world_position_1.xz;

#line 2249
        }
        else
        {

#line 2249
            planar_0 = world_position_1.xy;

#line 2249
        }

#line 2245
    }

#line 2257
    return planar_0 / float2(max(tile_metres_0, 0.00009999999747379f)) ;
}


#line 791
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1136
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1136
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


#line 573
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


#line 1142
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_1;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 1142
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1142
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
    float tile_metres_1;
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
    GpuMaterial_natural_0 device* materials_0;
    texture2d_array<float, access::sample> base_color_textures_0;
    sampler base_color_sampler_0;
    uint device* cluster_lights_0;
    GpuLight_natural_0 device* lights_0;
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
    texture2d<float, access::sample> specular_albedo_0;
    texture2d<float, access::sample> ambient_occlusion_0;
    GpuProbe_natural_0 device* probes_0;
};


#line 2051 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S5 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S6 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S7 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S8 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 2061
    uint _S9 = uint(pixel_0.x) / _S8;

#line 2061
    uint _S10 = min(_S9, _S5 - 1U);
    uint _S11 = uint(pixel_0.y) / _S8;

    float scale_0 = 24.0f / log2(10000.0f);

#line 2072
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S7 - 1U))) * _S6 + min(_S11, _S6 - 1U)) * _S5 + _S10;
}


#line 2016
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 2030
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 2037
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 1239
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 1246
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1253
    float _S12 = 1.0f - alpha2_0;

#line 1258
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S12 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S12 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 1566
float shadow_normal_offset_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_0 * cosine_0));
}


#line 237
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 1579
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 1586
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S13 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 1587
    int y_0 = int(-1);

#line 1587
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 1589
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 1589
            break;
        }

#line 1589
        int x_0 = int(-1);

        for(;;)
        {

#line 1591
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 1591
                break;
            }



            float _S14 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S13, float2(1.0f)  - _S13))), (reference_0), level((0.0f))));

#line 1595
            float visibility_1 = visibility_0 + _S14;

#line 1591
            x_0 = x_0 + int(1);

#line 1591
            visibility_0 = visibility_1;

#line 1591
        }

#line 1589
        y_0 = y_0 + int(1);

#line 1589
    }

#line 1599
    return visibility_0 / 9.0f;
}


#line 1649
float cascade_visibility_0(uint cascade_0, float3 world_position_2, float3 to_light_2, float3 geometric_normal_1, KernelContext_0 thread* kernelContext_2)
{

#line 1680
    float texel_world_0 = 2.0f * kernelContext_2->frame_0->cascade_far_0[cascade_0] / 768.0f;

#line 1687
    float4 clip_0 = (((float4(world_position_2 + geometric_normal_1 * float3((texel_world_0 * kernelContext_2->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_2->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1691
    bool _S15;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1692
        _S15 = true;

#line 1692
    }
    else
    {

#line 1692
        _S15 = (ndc_0.z) <= 0.0f;

#line 1692
    }

#line 1692
    if(_S15)
    {



        return 1.0f;
    }

#line 1697
    float _S16 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z, kernelContext_2);

#line 1713
    return _S16;
}


#line 1729
float sun_visibility_0(float3 world_position_3, float3 to_light_3, float n_dot_l_1, float3 geometric_normal_2, KernelContext_0 thread* kernelContext_3)
{

#line 1730
    uint cascade_1;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 1742
    float eye_distance_0 = length(world_position_3 - kernelContext_3->frame_0->camera_position_0.xyz);

#line 1742
    uint index_0 = 0U;

    for(;;)
    {

#line 1744
        if(index_0 < 2U)
        {
        }
        else
        {

#line 1744
            cascade_1 = 1U;

#line 1744
            break;
        }
        if(eye_distance_0 < kernelContext_3->frame_0->cascade_far_0[index_0])
        {

#line 1746
            cascade_1 = index_0;


            break;
        }

#line 1744
        index_0 = index_0 + 1U;

#line 1744
    }

#line 1744
    float _S17 = cascade_visibility_0(cascade_1, world_position_3, to_light_3, geometric_normal_2, kernelContext_3);

#line 1754
    uint _S18 = cascade_1 + 1U;

#line 1754
    if(_S18 >= 2U)
    {



        return _S17;
    }

#line 1767
    float band_0 = kernelContext_3->frame_0->cascade_far_0[cascade_1] * 0.10000000149011612f;
    float blend_0 = saturate((eye_distance_0 - (kernelContext_3->frame_0->cascade_far_0[cascade_1] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return _S17;
    }

#line 1771
    float _S19 = cascade_visibility_0(_S18, world_position_3, to_light_3, geometric_normal_2, kernelContext_3);

#line 1781
    return mix(_S17, _S19, blend_0);
}


#line 1967
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S20 = axis_2.x;

#line 1970
    float _S21 = axis_2.y;

#line 1970
    bool _S22;

#line 1970
    if(_S20 >= _S21)
    {

#line 1970
        _S22 = _S20 >= (axis_2.z);

#line 1970
    }
    else
    {

#line 1970
        _S22 = false;

#line 1970
    }

#line 1970
    uint _S23;

#line 1970
    if(_S22)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1972
            _S23 = 0U;

#line 1972
        }
        else
        {

#line 1972
            _S23 = 1U;

#line 1972
        }

#line 1972
        return _S23;
    }
    if(_S21 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1976
            _S23 = 2U;

#line 1976
        }
        else
        {

#line 1976
            _S23 = 3U;

#line 1976
        }

#line 1976
        return _S23;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1978
        _S23 = 4U;

#line 1978
    }
    else
    {

#line 1978
        _S23 = 5U;

#line 1978
    }

#line 1978
    return _S23;
}


#line 225
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 1884
float punctual_visibility_0(uint tile_3, float3 world_position_4, float3 to_light_4, float n_dot_l_2, float texel_world_1, float3 geometric_normal_3, KernelContext_0 thread* kernelContext_4)
{

#line 1896
    float4 clip_1 = (((float4(world_position_4 + geometric_normal_3 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4)))  + to_light_4 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_4->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 1903
    float _S24 = clip_1.w;

#line 1903
    if(_S24 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S24) ;

#line 1907
    bool _S25;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1908
        _S25 = true;

#line 1908
    }
    else
    {

#line 1908
        _S25 = (ndc_1.z) <= 0.0f;

#line 1908
    }

#line 1908
    if(_S25)
    {

#line 1908
        _S25 = true;

#line 1908
    }
    else
    {

#line 1908
        _S25 = (ndc_1.z) > 1.0f;

#line 1908
    }

#line 1908
    if(_S25)
    {

#line 1915
        return 1.0f;
    }

#line 1915
    float _S26 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_4);

#line 1921
    return _S26;
}


#line 1986
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_5, float3 to_light_5, float n_dot_l_3, float3 geometric_normal_4, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_5 - (float4(light_0->position_1) ).xyz;

#line 1994
    float _S27 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_5, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_4, kernelContext_5);

#line 2000
    return _S27;
}


#line 1928
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_6, float3 to_light_6, float n_dot_l_4, float3 geometric_normal_5, KernelContext_0 thread* kernelContext_6)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 1935
    float4 _S28 = float4(light_1->direction_0) ;

#line 1942
    float cos_outer_1 = _S28.w;

#line 1942
    float _S29 = punctual_visibility_0(tile_4, world_position_6, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_6 - (float4(light_1->position_1) ).xyz, normalize(_S28.xyz)), 0.0f) / 768.0f, geometric_normal_5, kernelContext_6);

#line 1949
    return _S29;
}


#line 1283
float decode_specular_albedo_0(float2 texel_1)
{
    return (texel_1.x * 65280.0f + texel_1.y * 255.0f) / 65535.0f;
}


#line 1300
float specular_albedo_at_0(float n_dot_v_1, float roughness_1, KernelContext_0 thread* kernelContext_7)
{

#line 1300
    texture2d<float, access::sample> _S30 = kernelContext_7->specular_albedo_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S30).get_width(0)),(*((&height_0)) = (_S30).get_height(0));
    float2 extent_1 = float2(float(width_0), float(height_0));
    float2 scaled_0 = float2(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1306
    float2 _S31 = float2(1.0f) ;
    float2 _S32 = extent_1 - _S31;

#line 1307
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S32);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S33 = int2(low_0);
    int2 _S34 = int2(min(low_0 + _S31, _S32));
    int _S35 = _S33.x;

#line 1313
    int _S36 = _S33.y;

#line 1313
    int3 _S37 = int3(_S35, _S36, int(0));
    int _S38 = _S34.x;

#line 1314
    int3 _S39 = int3(_S38, _S36, int(0));
    float _S40 = weight_0.x;
    int _S41 = _S34.y;

#line 1316
    int3 _S42 = int3(_S35, _S41, int(0));
    int3 _S43 = int3(_S38, _S41, int(0));

    return mix(mix(decode_specular_albedo_0(((kernelContext_7->specular_albedo_0).read(vec<uint,2>(((_S37)).xy), uint(((_S37)).z)).xy)), decode_specular_albedo_0(((kernelContext_7->specular_albedo_0).read(vec<uint,2>(((_S39)).xy), uint(((_S39)).z)).xy)), _S40), mix(decode_specular_albedo_0(((kernelContext_7->specular_albedo_0).read(vec<uint,2>(((_S42)).xy), uint(((_S42)).z)).xy)), decode_specular_albedo_0(((kernelContext_7->specular_albedo_0).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z)).xy)), _S40), weight_0.y);
}


#line 1338
float3 specular_compensation_0(float3 f0_1, float n_dot_v_2, float roughness_2, KernelContext_0 thread* kernelContext_8)
{

#line 1338
    float _S44 = specular_albedo_at_0(n_dot_v_2, roughness_2, kernelContext_8);



    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(_S44, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 2180
float3 sky_irradiance_0(float3 normal_2, KernelContext_0 thread* kernelContext_9)
{
    float4 basis_1 = float4(normal_2, 1.0f);
    return max(float3(dot(kernelContext_9->frame_0->sky_sh_r_0, basis_1), dot(kernelContext_9->frame_0->sky_sh_g_0, basis_1), dot(kernelContext_9->frame_0->sky_sh_b_0, basis_1)), float3(0.0f, 0.0f, 0.0f));
}


#line 702
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 2082
GpuProbe_0 probe_at_0(uint3 cell_0, KernelContext_0 thread* kernelContext_10)
{

    GpuProbe_natural_0 _S45 = kernelContext_10->probes_0[min((cell_0.z * kernelContext_10->frame_0->probe_counts_0.y + cell_0.y) * kernelContext_10->frame_0->probe_counts_0.x + cell_0.x, max(kernelContext_10->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 2085
    GpuProbe_0 _S46 = { float4(_S45.sh_r_0) , float4(_S45.sh_g_0) , float4(_S45.sh_b_0)  };

#line 2085
    return _S46;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_0)
{
    thread GpuProbe_0 blended_0;
    float4 _S47 = float4(t_0) ;

#line 2093
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S47);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S47);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S47);
    return blended_0;
}


#line 2133
float3 probe_irradiance_0(float3 world_position_7, float3 normal_3, KernelContext_0 thread* kernelContext_11)
{

#line 2133
    float3 _S48 = float3(1.0f) ;

#line 2138
    float3 _S49 = float3(0.0f, 0.0f, 0.0f);

#line 2138
    float3 last_0 = max(float3(kernelContext_11->frame_0->probe_counts_0.xyz) - _S48, _S49);
    float3 grid_1 = clamp((world_position_7 - kernelContext_11->frame_0->probe_origin_0.xyz) * kernelContext_11->frame_0->probe_inv_spacing_0.xyz, _S49, last_0);

    float3 base_2 = floor(grid_1);
    float3 f_0 = grid_1 - base_2;

    uint3 _S50 = uint3(base_2);



    uint3 _S51 = uint3(min(base_2 + _S48, last_0));

#line 2155
    uint _S52 = _S50.x;

#line 2155
    uint _S53 = _S50.y;

#line 2155
    uint _S54 = _S50.z;

#line 2155
    GpuProbe_0 _S55 = probe_at_0(uint3(_S52, _S53, _S54), kernelContext_11);

#line 2155
    uint _S56 = _S51.x;

#line 2155
    GpuProbe_0 _S57 = probe_at_0(uint3(_S56, _S53, _S54), kernelContext_11);

#line 2155
    float _S58 = f_0.x;

#line 2155
    thread GpuProbe_0 _S59 = _S55;

#line 2155
    thread GpuProbe_0 _S60 = _S57;

#line 2155
    GpuProbe_0 _S61 = lerp_probe_0(&_S59, &_S60, _S58);
    uint _S62 = _S51.y;

#line 2156
    GpuProbe_0 _S63 = probe_at_0(uint3(_S52, _S62, _S54), kernelContext_11);

#line 2156
    GpuProbe_0 _S64 = probe_at_0(uint3(_S56, _S62, _S54), kernelContext_11);

#line 2156
    thread GpuProbe_0 _S65 = _S63;

#line 2156
    thread GpuProbe_0 _S66 = _S64;

#line 2156
    GpuProbe_0 _S67 = lerp_probe_0(&_S65, &_S66, _S58);
    uint _S68 = _S51.z;

#line 2157
    GpuProbe_0 _S69 = probe_at_0(uint3(_S52, _S53, _S68), kernelContext_11);

#line 2157
    GpuProbe_0 _S70 = probe_at_0(uint3(_S56, _S53, _S68), kernelContext_11);

#line 2157
    thread GpuProbe_0 _S71 = _S69;

#line 2157
    thread GpuProbe_0 _S72 = _S70;

#line 2157
    GpuProbe_0 _S73 = lerp_probe_0(&_S71, &_S72, _S58);

#line 2157
    GpuProbe_0 _S74 = probe_at_0(uint3(_S52, _S62, _S68), kernelContext_11);

#line 2157
    GpuProbe_0 _S75 = probe_at_0(uint3(_S56, _S62, _S68), kernelContext_11);

#line 2157
    thread GpuProbe_0 _S76 = _S74;

#line 2157
    thread GpuProbe_0 _S77 = _S75;

#line 2157
    GpuProbe_0 _S78 = lerp_probe_0(&_S76, &_S77, _S58);

    float _S79 = f_0.y;

#line 2159
    thread GpuProbe_0 _S80 = _S61;

#line 2159
    thread GpuProbe_0 _S81 = _S67;

#line 2159
    GpuProbe_0 _S82 = lerp_probe_0(&_S80, &_S81, _S79);

#line 2159
    thread GpuProbe_0 _S83 = _S73;

#line 2159
    thread GpuProbe_0 _S84 = _S78;

#line 2159
    GpuProbe_0 _S85 = lerp_probe_0(&_S83, &_S84, _S79);

    float _S86 = f_0.z;

#line 2161
    thread GpuProbe_0 _S87 = _S82;

#line 2161
    thread GpuProbe_0 _S88 = _S85;

#line 2161
    GpuProbe_0 _S89 = lerp_probe_0(&_S87, &_S88, _S86);

    float4 basis_2 = float4(normal_3, 1.0f);
    return max(float3(dot(_S89.sh_r_0, basis_2), dot(_S89.sh_g_0, basis_2), dot(_S89.sh_b_0, basis_2)), _S49);
}


#line 675
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_1)
{
    return float3(material_1->emissive_r_0, material_1->emissive_g_0, material_1->emissive_b_0);
}


#line 1400
float fog_exp_neg_0(float x_1)
{
    float clamped_0 = clamp(x_1, -87.0f, 87.0f);


    float n_0 = floor(clamped_0 * 1.4426950216293335f + 0.5f);


    float _S90 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

#line 1408
    float kernel_0 = 0.0001984127011383f;

#line 1408
    int term_0 = int(6);

    for(;;)
    {

#line 1410
        if(term_0 >= int(0))
        {
        }
        else
        {

#line 1410
            break;
        }
        float _S91 = kernel_0 * _S90 + FOG_KERNEL_0[term_0];

#line 1410
        int term_1 = term_0 - int(1);

#line 1410
        kernel_0 = _S91;

#line 1410
        term_0 = term_1;

#line 1410
    }

#line 1417
    return kernel_0 * (as_type<float>((uint(int(127) - int(n_0)) << 23U)));
}


#line 1427
float fog_one_minus_exp_over_0(float d_0)
{
    if((abs(d_0)) < 0.125f)
    {
        float _S92 = - d_0;

#line 1431
        float series_0 = 0.00833333376795053f;

#line 1431
        int term_2 = int(3);

        for(;;)
        {

#line 1433
            if(term_2 >= int(0))
            {
            }
            else
            {

#line 1433
                break;
            }
            float _S93 = series_0 * _S92 + FOG_RATIO_KERNEL_0[term_2];

#line 1433
            int term_3 = term_2 - int(1);

#line 1433
            series_0 = _S93;

#line 1433
            term_2 = term_3;

#line 1433
        }



        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}


#line 1461
float fog_optical_depth_0(float density_0, float falloff_0, float height_a_0, float height_b_0, float distance_1)
{

    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_1, 0.0f, 32.0f);
    }

#line 1472
    return clamp(density_0 * distance_1 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}


#line 1480
float fog_transmittance_0(float optical_depth_0)
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}


#line 2206
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
};


#line 2206
struct pixelInput_0
{
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_2 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 2261
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S94 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_1 [[texture(3)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 2261
    thread KernelContext_0 kernelContext_12;

#line 2261
    (&kernelContext_12)->draw_0 = draw_1;

#line 2261
    (&kernelContext_12)->visible_instances_0 = visible_instances_1;

#line 2261
    (&kernelContext_12)->instances_0 = instances_1;

#line 2261
    (&kernelContext_12)->meshes_0 = meshes_1;

#line 2261
    (&kernelContext_12)->vertices_0 = vertices_1;

#line 2261
    (&kernelContext_12)->frame_0 = frame_1;

#line 2261
    (&kernelContext_12)->materials_0 = materials_1;

#line 2261
    (&kernelContext_12)->base_color_textures_0 = base_color_textures_1;

#line 2261
    (&kernelContext_12)->base_color_sampler_0 = base_color_sampler_1;

#line 2261
    (&kernelContext_12)->cluster_lights_0 = cluster_lights_1;

#line 2261
    (&kernelContext_12)->lights_0 = lights_1;

#line 2261
    (&kernelContext_12)->shadow_atlas_0 = shadow_atlas_1;

#line 2261
    (&kernelContext_12)->shadow_sampler_0 = shadow_sampler_1;

#line 2261
    (&kernelContext_12)->specular_albedo_0 = specular_albedo_1;

#line 2261
    (&kernelContext_12)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2261
    (&kernelContext_12)->probes_0 = probes_1;

#line 2267
    float3 normal_4 = normalize(_S94.world_normal_0);

#line 2285
    if((frame_1->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S94.color_2.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return tint_0;
    }

    if((frame_1->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 2298
        float3 _S95 = float3(0.5f) ;

#line 2305
        (&normals_0)->lit_0 = float4(normal_4 * _S95 + _S95, 1.0f);

#line 2311
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_12)->frame_0->camera_position_0.xyz - _S94.world_position_8);



    float3 _S96 = geometric_normal_of_0(_S94.world_position_8, normal_4);

#line 2319
    thread GpuMaterial_natural_0 _S97 = (&kernelContext_12)->materials_0[_S94.material_2];

#line 2319
    float2 uv_2;

#line 2338
    if(((&_S97)->tiling_0) == 1U)
    {

#line 2338
        uv_2 = physical_tile_uv_0(_S94.world_position_8, normal_4, (&_S97)->tile_metres_1);

#line 2338
    }
    else
    {

#line 2338
        uv_2 = _S94.uv_1;

#line 2338
    }

#line 2343
    float3 _S98 = float3(uv_2, float((&_S97)->base_color_texture_0));
    float4 albedo_0 = _S94.color_2 * float4((&_S97)->base_color_0)  * (((&kernelContext_12)->base_color_textures_0).sample(((&kernelContext_12)->base_color_sampler_0), ((_S98)).xy, uint(((_S98)).z)));

#line 2350
    float metallic_1 = saturate((&_S97)->metallic_0);
    float roughness_3 = clamp((&_S97)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_3 * roughness_3;
    float _S99 = alpha_0 * alpha_0;

#line 2359
    float3 _S100 = albedo_0.xyz;

#line 2359
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S100, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S100 * float3((1.0f - metallic_1)) ;

#line 2366
    float _S101 = max(dot(normal_4, to_eye_0), 0.00009999999747379f);

#line 2376
    float2 _S102 = position_2.xy;

#line 2376
    uint _S103 = froxel_of_0(_S102, (((float4(_S94.world_position_8, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_12);

#line 2376
    uint base_3 = _S103 * 17U;

#line 2381
    uint _S104 = min((&kernelContext_12)->cluster_lights_0[base_3], 16U);

#line 2387
    float3 _S105 = float3(0.0f, 0.0f, 0.0f);

#line 2387
    uint slot_0 = 0U;

#line 2387
    float3 direct_0 = _S105;

#line 2387
    float3 gloss_0 = _S105;

    for(;;)
    {

#line 2389
        if(slot_0 < _S104)
        {
        }
        else
        {

#line 2389
            break;
        }

#line 2389
        thread GpuLight_natural_0 _S106 = (&kernelContext_12)->lights_0[(&kernelContext_12)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 2389
        uint _S107 = (&_S106)->kind_0;

#line 2398
        bool _S108 = ((&_S106)->kind_0) == 0U;

#line 2398
        float3 to_light_7;

#line 2398
        float reach_0;

#line 2398
        if(_S108)
        {

#line 2398
            to_light_7 = normalize((float4((&_S106)->direction_0) ).xyz);

#line 2398
            reach_0 = 1.0f;

#line 2398
        }
        else
        {

#line 2398
            float4 _S109 = float4((&_S106)->position_1) ;

#line 2405
            float3 offset_0 = _S109.xyz - _S94.world_position_8;
            float distance_2 = length(offset_0);
            float3 to_light_8 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S109.w);
            if(_S107 == 2U)
            {

#line 2409
                float4 _S110 = float4((&_S106)->direction_0) ;

#line 2409
                reach_0 = reach_1 * spot_cone_0(to_light_8, _S110.xyz, _S110.w, (&_S106)->cos_inner_0);

#line 2409
            }
            else
            {

#line 2409
                reach_0 = reach_1;

#line 2409
            }

#line 2409
            to_light_7 = to_light_8;

#line 2398
        }

#line 2416
        float n_dot_l_5 = dot(normal_4, to_light_7);
        float _S111 = max(n_dot_l_5, 0.0f);

#line 2423
        float3 half_vector_0 = normalize(to_light_7 + to_eye_0);

#line 2430
        float3 specular_0 = ggx_lobe_0(_S99, f0_2, _S111, _S101, max(dot(normal_4, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S111) ;

#line 2430
        float reach_2;

#line 2445
        if(_S108)
        {

#line 2445
            float _S112 = sun_visibility_0(_S94.world_position_8, to_light_7, n_dot_l_5, _S96, &kernelContext_12);

#line 2445
            reach_2 = _S112;

#line 2445
        }
        else
        {

            if(_S107 == 1U)
            {

#line 2449
                uint _S113 = (&_S106)->shadow_tile_0;

#line 2461
                if(((&_S106)->shadow_tile_0) <= 8U)
                {

#line 2461
                    float _S114 = point_visibility_0(&_S106, _S113, _S94.world_position_8, to_light_7, n_dot_l_5, _S96, &kernelContext_12);

#line 2461
                    reach_2 = reach_0 * _S114;

#line 2461
                }
                else
                {

#line 2461
                    reach_2 = reach_0;

#line 2461
                }

#line 2449
            }
            else
            {

#line 2449
                uint _S115 = (&_S106)->shadow_tile_0;

#line 2467
                if(((&_S106)->shadow_tile_0) < 14U)
                {

#line 2467
                    float _S116 = spot_visibility_0(&_S106, _S115, _S94.world_position_8, to_light_7, n_dot_l_5, _S96, &kernelContext_12);

#line 2467
                    reach_2 = reach_0 * _S116;

#line 2467
                }
                else
                {

#line 2467
                    reach_2 = reach_0;

#line 2467
                }

#line 2449
            }

#line 2445
        }

#line 2475
        float3 _S117 = (float4((&_S106)->color_1) ).xyz;

#line 2475
        float3 direct_1 = direct_0 + _S117 * float3((_S111 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S117 * (specular_0 * float3(reach_2) );

#line 2389
        slot_0 = slot_0 + 1U;

#line 2389
        direct_0 = direct_1;

#line 2389
        gloss_0 = gloss_1;

#line 2389
    }

#line 2389
    float3 _S118 = specular_compensation_0(f0_2, _S101, roughness_3, &kernelContext_12);

#line 2490
    float3 gloss_2 = gloss_0 * _S118;

#line 2490
    texture2d<float, access::sample> _S119 = (&kernelContext_12)->ambient_occlusion_0;

#line 2508
    thread uint occlusion_width_0;
    thread uint occlusion_height_0;
    (*((&occlusion_width_0)) = (_S119).get_width(0)),(*((&occlusion_height_0)) = (_S119).get_height(0));


    int3 _S120 = int3(min(int2(_S102), int2(int(occlusion_width_0), int(occlusion_height_0)) - int2(int(1)) ), int(0));

#line 2513
    float occluded_0 = (((&kernelContext_12)->ambient_occlusion_0).read(vec<uint,2>(((_S120)).xy), uint(((_S120)).z)).x);

#line 2531
    float3 _S121 = frame_1->ambient_0.xyz;

#line 2531
    float3 _S122 = sky_irradiance_0(normal_4, &kernelContext_12);

#line 2531
    float3 _S123 = _S121 + _S122;

#line 2531
    float3 _S124 = probe_irradiance_0(_S94.world_position_8, normal_4, &kernelContext_12);

#line 2552
    float3 lit_1 = diffuse_albedo_0 * ((_S123 + _S124) * float3(occluded_0)  + direct_0) + gloss_2;

#line 2552
    float3 _S125 = emissive_of_0(&_S97);

#line 2588
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_12)->frame_0->fog_params_0.x, (&kernelContext_12)->frame_0->fog_params_0.y, (&kernelContext_12)->frame_0->camera_position_0.y - (&kernelContext_12)->frame_0->fog_params_0.z, _S94.world_position_8.y - (&kernelContext_12)->frame_0->fog_params_0.z, length((&kernelContext_12)->frame_0->camera_position_0.xyz - _S94.world_position_8)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S125) * float3(fog_survives_0)  + (&kernelContext_12)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);

#line 2600
    (&output_0)->reflectivity_0 = float4(f0_2, saturate(1.0f - roughness_3 / 0.5f));
    return output_0;
}


#line 2601
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_9 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
};


#line 1096
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_10;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_4;
};


#line 1096
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_2 [[texture(3)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1096
    thread KernelContext_0 kernelContext_13;

#line 1096
    (&kernelContext_13)->draw_0 = draw_2;

#line 1096
    (&kernelContext_13)->visible_instances_0 = visible_instances_2;

#line 1096
    (&kernelContext_13)->instances_0 = instances_2;

#line 1096
    (&kernelContext_13)->meshes_0 = meshes_2;

#line 1096
    (&kernelContext_13)->vertices_0 = vertices_2;

#line 1096
    (&kernelContext_13)->frame_0 = frame_2;

#line 1096
    (&kernelContext_13)->materials_0 = materials_2;

#line 1096
    (&kernelContext_13)->base_color_textures_0 = base_color_textures_2;

#line 1096
    (&kernelContext_13)->base_color_sampler_0 = base_color_sampler_2;

#line 1096
    (&kernelContext_13)->cluster_lights_0 = cluster_lights_2;

#line 1096
    (&kernelContext_13)->lights_0 = lights_2;

#line 1096
    (&kernelContext_13)->shadow_atlas_0 = shadow_atlas_2;

#line 1096
    (&kernelContext_13)->shadow_sampler_0 = shadow_sampler_2;

#line 1096
    (&kernelContext_13)->specular_albedo_0 = specular_albedo_2;

#line 1096
    (&kernelContext_13)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1096
    (&kernelContext_13)->probes_0 = probes_2;

#line 1096
    GpuInstance_natural_0 device* _S126 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1142
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1142
    uint base_vertex_2;

#line 1151
    if(((_S126->flags_0) & 2U) != 0U)
    {

#line 1151
        base_vertex_2 = _S126->base_vertex_0;

#line 1151
    }
    else
    {

#line 1151
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1151
    }

    MeshVertex_natural_0 vertex_0 = (&kernelContext_13)->vertices_0[index_1 + base_vertex_2];

#line 1153
    matrix<float,int(4),int(4)>  _S127 = matrix<float,int(4),int(4)> (_S126->transform_0.data_0[int(0)][int(0)], _S126->transform_0.data_0[int(1)][int(0)], _S126->transform_0.data_0[int(2)][int(0)], _S126->transform_0.data_0[int(3)][int(0)], _S126->transform_0.data_0[int(0)][int(1)], _S126->transform_0.data_0[int(1)][int(1)], _S126->transform_0.data_0[int(2)][int(1)], _S126->transform_0.data_0[int(3)][int(1)], _S126->transform_0.data_0[int(0)][int(2)], _S126->transform_0.data_0[int(1)][int(2)], _S126->transform_0.data_0[int(2)][int(2)], _S126->transform_0.data_0[int(3)][int(2)], _S126->transform_0.data_0[int(0)][int(3)], _S126->transform_0.data_0[int(1)][int(3)], _S126->transform_0.data_0[int(2)][int(3)], _S126->transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S127)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_13)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_10 = world_0.xyz;

#line 1165
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_1) ).xyz) * (normal_basis_0(matrix<float,int(3),int(3)> (_S127[int(0)].xyz, _S127[int(1)].xyz, _S127[int(2)].xyz)))));

#line 1165
    float4 _S128;

#line 1172
    if(((&kernelContext_13)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1172
        _S128 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1172
    }
    else
    {

#line 1172
        _S128 = float4(vertex_0.color_0) ;

#line 1172
    }

#line 1171
    (&output_1)->color_4 = _S128;

#line 1178
    (&output_1)->material_4 = _S126->material_0;
    (&output_1)->uv_4 = (float4(vertex_0.uv_0) ).xy;
    VertexOutput_0 _S129 = output_1;

#line 1180
    thread vertexMain_Result_0 _S130;

#line 1180
    (&_S130)->position_3 = _S129.position_4;

#line 1180
    (&_S130)->world_position_9 = _S129.world_position_10;

#line 1180
    (&_S130)->world_normal_1 = _S129.world_normal_2;

#line 1180
    (&_S130)->color_3 = _S129.color_4;

#line 1180
    (&_S130)->material_3 = _S129.material_4;

#line 1180
    (&_S130)->uv_3 = _S129.uv_4;

#line 1180
    return _S130;
}

