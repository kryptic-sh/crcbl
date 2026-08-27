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


#line 2160
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_0, float tile_metres_0)
{
    float3 axis_0 = abs(normal_0);

    float _S2 = axis_0.x;

#line 2164
    float _S3 = axis_0.y;

#line 2164
    bool _S4;

#line 2164
    if(_S2 >= _S3)
    {

#line 2164
        _S4 = _S2 >= (axis_0.z);

#line 2164
    }
    else
    {

#line 2164
        _S4 = false;

#line 2164
    }

#line 2164
    float2 planar_0;

#line 2164
    if(_S4)
    {

#line 2164
        planar_0 = world_position_1.zy;

#line 2164
    }
    else
    {

        if(_S3 >= (axis_0.z))
        {

#line 2168
            planar_0 = world_position_1.xz;

#line 2168
        }
        else
        {

#line 2168
            planar_0 = world_position_1.xy;

#line 2168
        }

#line 2164
    }

#line 2176
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


#line 1970 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S5 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S6 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S7 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S8 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1980
    uint _S9 = uint(pixel_0.x) / _S8;

#line 1980
    uint _S10 = min(_S9, _S5 - 1U);
    uint _S11 = uint(pixel_0.y) / _S8;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1991
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S7 - 1U))) * _S6 + min(_S11, _S6 - 1U)) * _S5 + _S10;
}


#line 1935
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 1949
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 1956
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


#line 1615
float sun_visibility_0(float3 world_position_2, float3 to_light_2, float n_dot_l_1, float3 geometric_normal_1, KernelContext_0 thread* kernelContext_2)
{

#line 1616
    uint cascade_0;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 1628
    float _S15 = length(world_position_2 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 1628
    uint index_0 = 0U;

    for(;;)
    {

#line 1630
        if(index_0 < 2U)
        {
        }
        else
        {

#line 1630
            cascade_0 = 1U;

#line 1630
            break;
        }
        if(_S15 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 1632
            cascade_0 = index_0;


            break;
        }

#line 1630
        index_0 = index_0 + 1U;

#line 1630
    }

#line 1667
    float texel_world_0 = 2.0f * kernelContext_2->frame_0->cascade_far_0[cascade_0] / 768.0f;

#line 1674
    float4 clip_0 = (((float4(world_position_2 + geometric_normal_1 * float3((texel_world_0 * kernelContext_2->frame_0->shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2)))  + to_light_2 * float3((texel_world_0 * kernelContext_2->frame_0->shadow_params_0.z)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1678
    bool _S16;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1679
        _S16 = true;

#line 1679
    }
    else
    {

#line 1679
        _S16 = (ndc_0.z) <= 0.0f;

#line 1679
    }

#line 1679
    if(_S16)
    {



        return 1.0f;
    }

#line 1684
    float _S17 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z, kernelContext_2);

#line 1700
    return _S17;
}


#line 1886
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S18 = axis_2.x;

#line 1889
    float _S19 = axis_2.y;

#line 1889
    bool _S20;

#line 1889
    if(_S18 >= _S19)
    {

#line 1889
        _S20 = _S18 >= (axis_2.z);

#line 1889
    }
    else
    {

#line 1889
        _S20 = false;

#line 1889
    }

#line 1889
    uint _S21;

#line 1889
    if(_S20)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1891
            _S21 = 0U;

#line 1891
        }
        else
        {

#line 1891
            _S21 = 1U;

#line 1891
        }

#line 1891
        return _S21;
    }
    if(_S19 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1895
            _S21 = 2U;

#line 1895
        }
        else
        {

#line 1895
            _S21 = 3U;

#line 1895
        }

#line 1895
        return _S21;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1897
        _S21 = 4U;

#line 1897
    }
    else
    {

#line 1897
        _S21 = 5U;

#line 1897
    }

#line 1897
    return _S21;
}


#line 225
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 1803
float punctual_visibility_0(uint tile_3, float3 world_position_3, float3 to_light_3, float n_dot_l_2, float texel_world_1, float3 geometric_normal_2, KernelContext_0 thread* kernelContext_3)
{

#line 1815
    float4 clip_1 = (((float4(world_position_3 + geometric_normal_2 * float3((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_2, to_light_3)))  + to_light_3 * float3((texel_world_1 * 2.0f)) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 1822
    float _S22 = clip_1.w;

#line 1822
    if(_S22 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S22) ;

#line 1826
    bool _S23;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1827
        _S23 = true;

#line 1827
    }
    else
    {

#line 1827
        _S23 = (ndc_1.z) <= 0.0f;

#line 1827
    }

#line 1827
    if(_S23)
    {

#line 1827
        _S23 = true;

#line 1827
    }
    else
    {

#line 1827
        _S23 = (ndc_1.z) > 1.0f;

#line 1827
    }

#line 1827
    if(_S23)
    {

#line 1834
        return 1.0f;
    }

#line 1834
    float _S24 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 1840
    return _S24;
}


#line 1905
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_4, float3 to_light_4, float n_dot_l_3, float3 geometric_normal_3, KernelContext_0 thread* kernelContext_4)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_4 - (float4(light_0->position_1) ).xyz;

#line 1913
    float _S25 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_4, to_light_4, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_3, kernelContext_4);

#line 1919
    return _S25;
}


#line 1847
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_5, float3 to_light_5, float n_dot_l_4, float3 geometric_normal_4, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 1854
    float4 _S26 = float4(light_1->direction_0) ;

#line 1861
    float cos_outer_1 = _S26.w;

#line 1861
    float _S27 = punctual_visibility_0(tile_4, world_position_5, to_light_5, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (float4(light_1->position_1) ).xyz, normalize(_S26.xyz)), 0.0f) / 768.0f, geometric_normal_4, kernelContext_5);

#line 1868
    return _S27;
}


#line 1283
float decode_specular_albedo_0(float2 texel_1)
{
    return (texel_1.x * 65280.0f + texel_1.y * 255.0f) / 65535.0f;
}


#line 1300
float specular_albedo_at_0(float n_dot_v_1, float roughness_1, KernelContext_0 thread* kernelContext_6)
{

#line 1300
    texture2d<float, access::sample> _S28 = kernelContext_6->specular_albedo_0;

    thread uint width_0;
    thread uint height_0;
    (*((&width_0)) = (_S28).get_width(0)),(*((&height_0)) = (_S28).get_height(0));
    float2 extent_1 = float2(float(width_0), float(height_0));
    float2 scaled_0 = float2(saturate(n_dot_v_1), saturate(roughness_1)) * extent_1 - float2(0.5f) ;

#line 1306
    float2 _S29 = float2(1.0f) ;
    float2 _S30 = extent_1 - _S29;

#line 1307
    float2 low_0 = clamp(floor(scaled_0), float2(0.0f, 0.0f), _S30);

    float2 weight_0 = clamp(scaled_0 - low_0, float2(0.0f) , float2(1.0f) );

    int2 _S31 = int2(low_0);
    int2 _S32 = int2(min(low_0 + _S29, _S30));
    int _S33 = _S31.x;

#line 1313
    int _S34 = _S31.y;

#line 1313
    int3 _S35 = int3(_S33, _S34, int(0));
    int _S36 = _S32.x;

#line 1314
    int3 _S37 = int3(_S36, _S34, int(0));
    float _S38 = weight_0.x;
    int _S39 = _S32.y;

#line 1316
    int3 _S40 = int3(_S33, _S39, int(0));
    int3 _S41 = int3(_S36, _S39, int(0));

    return mix(mix(decode_specular_albedo_0(((kernelContext_6->specular_albedo_0).read(vec<uint,2>(((_S35)).xy), uint(((_S35)).z)).xy)), decode_specular_albedo_0(((kernelContext_6->specular_albedo_0).read(vec<uint,2>(((_S37)).xy), uint(((_S37)).z)).xy)), _S38), mix(decode_specular_albedo_0(((kernelContext_6->specular_albedo_0).read(vec<uint,2>(((_S40)).xy), uint(((_S40)).z)).xy)), decode_specular_albedo_0(((kernelContext_6->specular_albedo_0).read(vec<uint,2>(((_S41)).xy), uint(((_S41)).z)).xy)), _S38), weight_0.y);
}


#line 1338
float3 specular_compensation_0(float3 f0_1, float n_dot_v_2, float roughness_2, KernelContext_0 thread* kernelContext_7)
{

#line 1338
    float _S42 = specular_albedo_at_0(n_dot_v_2, roughness_2, kernelContext_7);



    return float3(1.0f, 1.0f, 1.0f) + f0_1 * float3((1.0f / clamp(_S42, 0.00009999999747379f, 1.0f) - 1.0f)) ;
}


#line 2099
float3 sky_irradiance_0(float3 normal_2, KernelContext_0 thread* kernelContext_8)
{
    float4 basis_1 = float4(normal_2, 1.0f);
    return max(float3(dot(kernelContext_8->frame_0->sky_sh_r_0, basis_1), dot(kernelContext_8->frame_0->sky_sh_g_0, basis_1), dot(kernelContext_8->frame_0->sky_sh_b_0, basis_1)), float3(0.0f, 0.0f, 0.0f));
}


#line 702
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 2001
GpuProbe_0 probe_at_0(uint3 cell_0, KernelContext_0 thread* kernelContext_9)
{

    GpuProbe_natural_0 _S43 = kernelContext_9->probes_0[min((cell_0.z * kernelContext_9->frame_0->probe_counts_0.y + cell_0.y) * kernelContext_9->frame_0->probe_counts_0.x + cell_0.x, max(kernelContext_9->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 2004
    GpuProbe_0 _S44 = { float4(_S43.sh_r_0) , float4(_S43.sh_g_0) , float4(_S43.sh_b_0)  };

#line 2004
    return _S44;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_0)
{
    thread GpuProbe_0 blended_0;
    float4 _S45 = float4(t_0) ;

#line 2012
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S45);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S45);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S45);
    return blended_0;
}


#line 2052
float3 probe_irradiance_0(float3 world_position_6, float3 normal_3, KernelContext_0 thread* kernelContext_10)
{

#line 2052
    float3 _S46 = float3(1.0f) ;

#line 2057
    float3 _S47 = float3(0.0f, 0.0f, 0.0f);

#line 2057
    float3 last_0 = max(float3(kernelContext_10->frame_0->probe_counts_0.xyz) - _S46, _S47);
    float3 grid_1 = clamp((world_position_6 - kernelContext_10->frame_0->probe_origin_0.xyz) * kernelContext_10->frame_0->probe_inv_spacing_0.xyz, _S47, last_0);

    float3 base_2 = floor(grid_1);
    float3 f_0 = grid_1 - base_2;

    uint3 _S48 = uint3(base_2);



    uint3 _S49 = uint3(min(base_2 + _S46, last_0));

#line 2074
    uint _S50 = _S48.x;

#line 2074
    uint _S51 = _S48.y;

#line 2074
    uint _S52 = _S48.z;

#line 2074
    GpuProbe_0 _S53 = probe_at_0(uint3(_S50, _S51, _S52), kernelContext_10);

#line 2074
    uint _S54 = _S49.x;

#line 2074
    GpuProbe_0 _S55 = probe_at_0(uint3(_S54, _S51, _S52), kernelContext_10);

#line 2074
    float _S56 = f_0.x;

#line 2074
    thread GpuProbe_0 _S57 = _S53;

#line 2074
    thread GpuProbe_0 _S58 = _S55;

#line 2074
    GpuProbe_0 _S59 = lerp_probe_0(&_S57, &_S58, _S56);
    uint _S60 = _S49.y;

#line 2075
    GpuProbe_0 _S61 = probe_at_0(uint3(_S50, _S60, _S52), kernelContext_10);

#line 2075
    GpuProbe_0 _S62 = probe_at_0(uint3(_S54, _S60, _S52), kernelContext_10);

#line 2075
    thread GpuProbe_0 _S63 = _S61;

#line 2075
    thread GpuProbe_0 _S64 = _S62;

#line 2075
    GpuProbe_0 _S65 = lerp_probe_0(&_S63, &_S64, _S56);
    uint _S66 = _S49.z;

#line 2076
    GpuProbe_0 _S67 = probe_at_0(uint3(_S50, _S51, _S66), kernelContext_10);

#line 2076
    GpuProbe_0 _S68 = probe_at_0(uint3(_S54, _S51, _S66), kernelContext_10);

#line 2076
    thread GpuProbe_0 _S69 = _S67;

#line 2076
    thread GpuProbe_0 _S70 = _S68;

#line 2076
    GpuProbe_0 _S71 = lerp_probe_0(&_S69, &_S70, _S56);

#line 2076
    GpuProbe_0 _S72 = probe_at_0(uint3(_S50, _S60, _S66), kernelContext_10);

#line 2076
    GpuProbe_0 _S73 = probe_at_0(uint3(_S54, _S60, _S66), kernelContext_10);

#line 2076
    thread GpuProbe_0 _S74 = _S72;

#line 2076
    thread GpuProbe_0 _S75 = _S73;

#line 2076
    GpuProbe_0 _S76 = lerp_probe_0(&_S74, &_S75, _S56);

    float _S77 = f_0.y;

#line 2078
    thread GpuProbe_0 _S78 = _S59;

#line 2078
    thread GpuProbe_0 _S79 = _S65;

#line 2078
    GpuProbe_0 _S80 = lerp_probe_0(&_S78, &_S79, _S77);

#line 2078
    thread GpuProbe_0 _S81 = _S71;

#line 2078
    thread GpuProbe_0 _S82 = _S76;

#line 2078
    GpuProbe_0 _S83 = lerp_probe_0(&_S81, &_S82, _S77);

    float _S84 = f_0.z;

#line 2080
    thread GpuProbe_0 _S85 = _S80;

#line 2080
    thread GpuProbe_0 _S86 = _S83;

#line 2080
    GpuProbe_0 _S87 = lerp_probe_0(&_S85, &_S86, _S84);

    float4 basis_2 = float4(normal_3, 1.0f);
    return max(float3(dot(_S87.sh_r_0, basis_2), dot(_S87.sh_g_0, basis_2), dot(_S87.sh_b_0, basis_2)), _S47);
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


    float _S88 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);

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
        float _S89 = kernel_0 * _S88 + FOG_KERNEL_0[term_0];

#line 1410
        int term_1 = term_0 - int(1);

#line 1410
        kernel_0 = _S89;

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
        float _S90 = - d_0;

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
            float _S91 = series_0 * _S90 + FOG_RATIO_KERNEL_0[term_2];

#line 1433
            int term_3 = term_2 - int(1);

#line 1433
            series_0 = _S91;

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


#line 2125
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
};


#line 2125
struct pixelInput_0
{
    float3 world_position_7 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_2 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 2180
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S92 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_1 [[texture(3)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 2180
    thread KernelContext_0 kernelContext_11;

#line 2180
    (&kernelContext_11)->draw_0 = draw_1;

#line 2180
    (&kernelContext_11)->visible_instances_0 = visible_instances_1;

#line 2180
    (&kernelContext_11)->instances_0 = instances_1;

#line 2180
    (&kernelContext_11)->meshes_0 = meshes_1;

#line 2180
    (&kernelContext_11)->vertices_0 = vertices_1;

#line 2180
    (&kernelContext_11)->frame_0 = frame_1;

#line 2180
    (&kernelContext_11)->materials_0 = materials_1;

#line 2180
    (&kernelContext_11)->base_color_textures_0 = base_color_textures_1;

#line 2180
    (&kernelContext_11)->base_color_sampler_0 = base_color_sampler_1;

#line 2180
    (&kernelContext_11)->cluster_lights_0 = cluster_lights_1;

#line 2180
    (&kernelContext_11)->lights_0 = lights_1;

#line 2180
    (&kernelContext_11)->shadow_atlas_0 = shadow_atlas_1;

#line 2180
    (&kernelContext_11)->shadow_sampler_0 = shadow_sampler_1;

#line 2180
    (&kernelContext_11)->specular_albedo_0 = specular_albedo_1;

#line 2180
    (&kernelContext_11)->ambient_occlusion_0 = ambient_occlusion_1;

#line 2180
    (&kernelContext_11)->probes_0 = probes_1;

#line 2186
    float3 normal_4 = normalize(_S92.world_normal_0);

#line 2204
    if((frame_1->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S92.color_2.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return tint_0;
    }

    if((frame_1->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 2217
        float3 _S93 = float3(0.5f) ;

#line 2224
        (&normals_0)->lit_0 = float4(normal_4 * _S93 + _S93, 1.0f);

#line 2230
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_11)->frame_0->camera_position_0.xyz - _S92.world_position_7);



    float3 _S94 = geometric_normal_of_0(_S92.world_position_7, normal_4);

#line 2238
    thread GpuMaterial_natural_0 _S95 = (&kernelContext_11)->materials_0[_S92.material_2];

#line 2238
    float2 uv_2;

#line 2257
    if(((&_S95)->tiling_0) == 1U)
    {

#line 2257
        uv_2 = physical_tile_uv_0(_S92.world_position_7, normal_4, (&_S95)->tile_metres_1);

#line 2257
    }
    else
    {

#line 2257
        uv_2 = _S92.uv_1;

#line 2257
    }

#line 2262
    float3 _S96 = float3(uv_2, float((&_S95)->base_color_texture_0));
    float4 albedo_0 = _S92.color_2 * float4((&_S95)->base_color_0)  * (((&kernelContext_11)->base_color_textures_0).sample(((&kernelContext_11)->base_color_sampler_0), ((_S96)).xy, uint(((_S96)).z)));

#line 2269
    float metallic_1 = saturate((&_S95)->metallic_0);
    float roughness_3 = clamp((&_S95)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_3 * roughness_3;
    float _S97 = alpha_0 * alpha_0;

#line 2278
    float3 _S98 = albedo_0.xyz;

#line 2278
    float3 f0_2 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S98, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S98 * float3((1.0f - metallic_1)) ;

#line 2285
    float _S99 = max(dot(normal_4, to_eye_0), 0.00009999999747379f);

#line 2295
    float2 _S100 = position_2.xy;

#line 2295
    uint _S101 = froxel_of_0(_S100, (((float4(_S92.world_position_7, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_11)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_11)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_11);

#line 2295
    uint base_3 = _S101 * 17U;

#line 2300
    uint _S102 = min((&kernelContext_11)->cluster_lights_0[base_3], 16U);

#line 2306
    float3 _S103 = float3(0.0f, 0.0f, 0.0f);

#line 2306
    uint slot_0 = 0U;

#line 2306
    float3 direct_0 = _S103;

#line 2306
    float3 gloss_0 = _S103;

    for(;;)
    {

#line 2308
        if(slot_0 < _S102)
        {
        }
        else
        {

#line 2308
            break;
        }

#line 2308
        thread GpuLight_natural_0 _S104 = (&kernelContext_11)->lights_0[(&kernelContext_11)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 2308
        uint _S105 = (&_S104)->kind_0;

#line 2317
        bool _S106 = ((&_S104)->kind_0) == 0U;

#line 2317
        float3 to_light_6;

#line 2317
        float reach_0;

#line 2317
        if(_S106)
        {

#line 2317
            to_light_6 = normalize((float4((&_S104)->direction_0) ).xyz);

#line 2317
            reach_0 = 1.0f;

#line 2317
        }
        else
        {

#line 2317
            float4 _S107 = float4((&_S104)->position_1) ;

#line 2324
            float3 offset_0 = _S107.xyz - _S92.world_position_7;
            float distance_2 = length(offset_0);
            float3 to_light_7 = offset_0 / float3(max(distance_2, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_2, _S107.w);
            if(_S105 == 2U)
            {

#line 2328
                float4 _S108 = float4((&_S104)->direction_0) ;

#line 2328
                reach_0 = reach_1 * spot_cone_0(to_light_7, _S108.xyz, _S108.w, (&_S104)->cos_inner_0);

#line 2328
            }
            else
            {

#line 2328
                reach_0 = reach_1;

#line 2328
            }

#line 2328
            to_light_6 = to_light_7;

#line 2317
        }

#line 2335
        float n_dot_l_5 = dot(normal_4, to_light_6);
        float _S109 = max(n_dot_l_5, 0.0f);

#line 2342
        float3 half_vector_0 = normalize(to_light_6 + to_eye_0);

#line 2349
        float3 specular_0 = ggx_lobe_0(_S97, f0_2, _S109, _S99, max(dot(normal_4, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S109) ;

#line 2349
        float reach_2;

#line 2364
        if(_S106)
        {

#line 2364
            float _S110 = sun_visibility_0(_S92.world_position_7, to_light_6, n_dot_l_5, _S94, &kernelContext_11);

#line 2364
            reach_2 = _S110;

#line 2364
        }
        else
        {

            if(_S105 == 1U)
            {

#line 2368
                uint _S111 = (&_S104)->shadow_tile_0;

#line 2380
                if(((&_S104)->shadow_tile_0) <= 8U)
                {

#line 2380
                    float _S112 = point_visibility_0(&_S104, _S111, _S92.world_position_7, to_light_6, n_dot_l_5, _S94, &kernelContext_11);

#line 2380
                    reach_2 = reach_0 * _S112;

#line 2380
                }
                else
                {

#line 2380
                    reach_2 = reach_0;

#line 2380
                }

#line 2368
            }
            else
            {

#line 2368
                uint _S113 = (&_S104)->shadow_tile_0;

#line 2386
                if(((&_S104)->shadow_tile_0) < 14U)
                {

#line 2386
                    float _S114 = spot_visibility_0(&_S104, _S113, _S92.world_position_7, to_light_6, n_dot_l_5, _S94, &kernelContext_11);

#line 2386
                    reach_2 = reach_0 * _S114;

#line 2386
                }
                else
                {

#line 2386
                    reach_2 = reach_0;

#line 2386
                }

#line 2368
            }

#line 2364
        }

#line 2394
        float3 _S115 = (float4((&_S104)->color_1) ).xyz;

#line 2394
        float3 direct_1 = direct_0 + _S115 * float3((_S109 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S115 * (specular_0 * float3(reach_2) );

#line 2308
        slot_0 = slot_0 + 1U;

#line 2308
        direct_0 = direct_1;

#line 2308
        gloss_0 = gloss_1;

#line 2308
    }

#line 2308
    float3 _S116 = specular_compensation_0(f0_2, _S99, roughness_3, &kernelContext_11);

#line 2409
    float3 gloss_2 = gloss_0 * _S116;

#line 2409
    texture2d<float, access::sample> _S117 = (&kernelContext_11)->ambient_occlusion_0;

#line 2427
    thread uint occlusion_width_0;
    thread uint occlusion_height_0;
    (*((&occlusion_width_0)) = (_S117).get_width(0)),(*((&occlusion_height_0)) = (_S117).get_height(0));


    int3 _S118 = int3(min(int2(_S100), int2(int(occlusion_width_0), int(occlusion_height_0)) - int2(int(1)) ), int(0));

#line 2432
    float occluded_0 = (((&kernelContext_11)->ambient_occlusion_0).read(vec<uint,2>(((_S118)).xy), uint(((_S118)).z)).x);

#line 2450
    float3 _S119 = frame_1->ambient_0.xyz;

#line 2450
    float3 _S120 = sky_irradiance_0(normal_4, &kernelContext_11);

#line 2450
    float3 _S121 = _S119 + _S120;

#line 2450
    float3 _S122 = probe_irradiance_0(_S92.world_position_7, normal_4, &kernelContext_11);

#line 2471
    float3 lit_1 = diffuse_albedo_0 * ((_S121 + _S122) * float3(occluded_0)  + direct_0) + gloss_2;

#line 2471
    float3 _S123 = emissive_of_0(&_S95);

#line 2507
    float fog_survives_0 = fog_transmittance_0(fog_optical_depth_0((&kernelContext_11)->frame_0->fog_params_0.x, (&kernelContext_11)->frame_0->fog_params_0.y, (&kernelContext_11)->frame_0->camera_position_0.y - (&kernelContext_11)->frame_0->fog_params_0.z, _S92.world_position_7.y - (&kernelContext_11)->frame_0->fog_params_0.z, length((&kernelContext_11)->frame_0->camera_position_0.xyz - _S92.world_position_7)));


    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4((lit_1 + _S123) * float3(fog_survives_0)  + (&kernelContext_11)->frame_0->fog_color_0.xyz * float3((1.0f - fog_survives_0)) , albedo_0.w);

#line 2519
    (&output_0)->reflectivity_0 = float4(f0_2, saturate(1.0f - roughness_3 / 0.5f));
    return output_0;
}


#line 2520
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
};


#line 1096
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_9;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_4;
};


#line 1096
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> specular_albedo_2 [[texture(3)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 1096
    thread KernelContext_0 kernelContext_12;

#line 1096
    (&kernelContext_12)->draw_0 = draw_2;

#line 1096
    (&kernelContext_12)->visible_instances_0 = visible_instances_2;

#line 1096
    (&kernelContext_12)->instances_0 = instances_2;

#line 1096
    (&kernelContext_12)->meshes_0 = meshes_2;

#line 1096
    (&kernelContext_12)->vertices_0 = vertices_2;

#line 1096
    (&kernelContext_12)->frame_0 = frame_2;

#line 1096
    (&kernelContext_12)->materials_0 = materials_2;

#line 1096
    (&kernelContext_12)->base_color_textures_0 = base_color_textures_2;

#line 1096
    (&kernelContext_12)->base_color_sampler_0 = base_color_sampler_2;

#line 1096
    (&kernelContext_12)->cluster_lights_0 = cluster_lights_2;

#line 1096
    (&kernelContext_12)->lights_0 = lights_2;

#line 1096
    (&kernelContext_12)->shadow_atlas_0 = shadow_atlas_2;

#line 1096
    (&kernelContext_12)->shadow_sampler_0 = shadow_sampler_2;

#line 1096
    (&kernelContext_12)->specular_albedo_0 = specular_albedo_2;

#line 1096
    (&kernelContext_12)->ambient_occlusion_0 = ambient_occlusion_2;

#line 1096
    (&kernelContext_12)->probes_0 = probes_2;

#line 1096
    GpuInstance_natural_0 device* _S124 = instances_2+visible_instances_2[draw_2->base_0 + instance_id_0];

#line 1142
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1142
    uint base_vertex_2;

#line 1151
    if(((_S124->flags_0) & 2U) != 0U)
    {

#line 1151
        base_vertex_2 = _S124->base_vertex_0;

#line 1151
    }
    else
    {

#line 1151
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1151
    }

    MeshVertex_natural_0 vertex_0 = (&kernelContext_12)->vertices_0[index_1 + base_vertex_2];

#line 1153
    matrix<float,int(4),int(4)>  _S125 = matrix<float,int(4),int(4)> (_S124->transform_0.data_0[int(0)][int(0)], _S124->transform_0.data_0[int(1)][int(0)], _S124->transform_0.data_0[int(2)][int(0)], _S124->transform_0.data_0[int(3)][int(0)], _S124->transform_0.data_0[int(0)][int(1)], _S124->transform_0.data_0[int(1)][int(1)], _S124->transform_0.data_0[int(2)][int(1)], _S124->transform_0.data_0[int(3)][int(1)], _S124->transform_0.data_0[int(0)][int(2)], _S124->transform_0.data_0[int(1)][int(2)], _S124->transform_0.data_0[int(2)][int(2)], _S124->transform_0.data_0[int(3)][int(2)], _S124->transform_0.data_0[int(0)][int(3)], _S124->transform_0.data_0[int(1)][int(3)], _S124->transform_0.data_0[int(2)][int(3)], _S124->transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S125)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_12)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_9 = world_0.xyz;

#line 1165
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_1) ).xyz) * (normal_basis_0(matrix<float,int(3),int(3)> (_S125[int(0)].xyz, _S125[int(1)].xyz, _S125[int(2)].xyz)))));

#line 1165
    float4 _S126;

#line 1172
    if(((&kernelContext_12)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1172
        _S126 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1172
    }
    else
    {

#line 1172
        _S126 = float4(vertex_0.color_0) ;

#line 1172
    }

#line 1171
    (&output_1)->color_4 = _S126;

#line 1178
    (&output_1)->material_4 = _S124->material_0;
    (&output_1)->uv_4 = (float4(vertex_0.uv_0) ).xy;
    VertexOutput_0 _S127 = output_1;

#line 1180
    thread vertexMain_Result_0 _S128;

#line 1180
    (&_S128)->position_3 = _S127.position_4;

#line 1180
    (&_S128)->world_position_8 = _S127.world_position_9;

#line 1180
    (&_S128)->world_normal_1 = _S127.world_normal_2;

#line 1180
    (&_S128)->color_3 = _S127.color_4;

#line 1180
    (&_S128)->material_3 = _S127.material_4;

#line 1180
    (&_S128)->uv_3 = _S127.uv_4;

#line 1180
    return _S128;
}

