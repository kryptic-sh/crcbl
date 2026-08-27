#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 990 "shaders/mesh.slang"
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_0)
{
    return matrix<float,int(3),int(3)> (cross(basis_0[int(1)], basis_0[int(2)]), cross(basis_0[int(2)], basis_0[int(0)]), cross(basis_0[int(0)], basis_0[int(1)]));
}


#line 1217
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1228
    float3 _S1;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1229
        _S1 = - facet_1;

#line 1229
    }
    else
    {

#line 1229
        _S1 = facet_1;

#line 1229
    }

#line 1229
    return _S1;
}


#line 1791
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_0, float tile_metres_0)
{
    float3 axis_0 = abs(normal_0);

    float _S2 = axis_0.x;

#line 1795
    float _S3 = axis_0.y;

#line 1795
    bool _S4;

#line 1795
    if(_S2 >= _S3)
    {

#line 1795
        _S4 = _S2 >= (axis_0.z);

#line 1795
    }
    else
    {

#line 1795
        _S4 = false;

#line 1795
    }

#line 1795
    float2 planar_0;

#line 1795
    if(_S4)
    {

#line 1795
        planar_0 = world_position_1.zy;

#line 1795
    }
    else
    {

        if(_S3 >= (axis_0.z))
        {

#line 1799
            planar_0 = world_position_1.xz;

#line 1799
        }
        else
        {

#line 1799
            planar_0 = world_position_1.xy;

#line 1799
        }

#line 1795
    }

#line 1807
    return planar_0 / float2(max(tile_metres_0, 0.00009999999747379f)) ;
}


#line 724
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 1037
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 1037
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
    uint base_vertex_0;
    uint pad0_1;
    uint pad1_1;
    uint pad2_0;
};


#line 506
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


#line 1043
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_1;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 1043
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 1043
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
    texture2d<float, access::sample> ambient_occlusion_0;
    GpuProbe_natural_0 device* probes_0;
};


#line 1622 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S5 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S6 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S7 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S8 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1632
    uint _S9 = uint(pixel_0.x) / _S8;

#line 1632
    uint _S10 = min(_S9, _S5 - 1U);
    uint _S11 = uint(pixel_0.y) / _S8;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1643
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S7 - 1U))) * _S6 + min(_S11, _S6 - 1U)) * _S5 + _S10;
}


#line 1587
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 1601
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 1608
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 1140
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 1147
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 1154
    float _S12 = 1.0f - alpha2_0;

#line 1159
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S12 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S12 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 1244
float shadow_slope_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));

    return min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f);
}


#line 228
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 4.0f);
}


#line 1258
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 1265
    float2 grid_0 = float2(4.0f, 4.0f);
    float2 _S13 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 1266
    int y_0 = int(-1);

#line 1266
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 1268
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 1268
            break;
        }

#line 1268
        int x_0 = int(-1);

        for(;;)
        {

#line 1270
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 1270
                break;
            }



            float _S14 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S13, float2(1.0f)  - _S13))), (reference_0), level((0.0f))));

#line 1274
            float visibility_1 = visibility_0 + _S14;

#line 1270
            x_0 = x_0 + int(1);

#line 1270
            visibility_0 = visibility_1;

#line 1270
        }

#line 1268
        y_0 = y_0 + int(1);

#line 1268
    }

#line 1278
    return visibility_0 / 9.0f;
}


#line 1294
float sun_visibility_0(float3 world_position_2, float3 to_light_2, float n_dot_l_1, float3 geometric_normal_1, KernelContext_0 thread* kernelContext_2)
{

#line 1295
    uint cascade_0;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 1307
    float _S15 = length(world_position_2 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 1307
    uint index_0 = 0U;

    for(;;)
    {

#line 1309
        if(index_0 < 2U)
        {
        }
        else
        {

#line 1309
            cascade_0 = 1U;

#line 1309
            break;
        }
        if(_S15 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 1311
            cascade_0 = index_0;


            break;
        }

#line 1309
        index_0 = index_0 + 1U;

#line 1309
    }

#line 1345
    float4 clip_0 = (((float4(world_position_2 + to_light_2 * float3((2.0f * kernelContext_2->frame_0->cascade_far_0[cascade_0] / 768.0f * (kernelContext_2->frame_0->shadow_params_0.z + kernelContext_2->frame_0->shadow_params_0.w * shadow_slope_0(geometric_normal_1, to_light_2)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));

#line 1350
    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1350
    bool _S16;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1351
        _S16 = true;

#line 1351
    }
    else
    {

#line 1351
        _S16 = (ndc_0.z) <= 0.0f;

#line 1351
    }

#line 1351
    if(_S16)
    {



        return 1.0f;
    }

#line 1356
    float _S17 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z, kernelContext_2);

#line 1370
    return _S17;
}


#line 1538
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S18 = axis_2.x;

#line 1541
    float _S19 = axis_2.y;

#line 1541
    bool _S20;

#line 1541
    if(_S18 >= _S19)
    {

#line 1541
        _S20 = _S18 >= (axis_2.z);

#line 1541
    }
    else
    {

#line 1541
        _S20 = false;

#line 1541
    }

#line 1541
    uint _S21;

#line 1541
    if(_S20)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1543
            _S21 = 0U;

#line 1543
        }
        else
        {

#line 1543
            _S21 = 1U;

#line 1543
        }

#line 1543
        return _S21;
    }
    if(_S19 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1547
            _S21 = 2U;

#line 1547
        }
        else
        {

#line 1547
            _S21 = 3U;

#line 1547
        }

#line 1547
        return _S21;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1549
        _S21 = 4U;

#line 1549
    }
    else
    {

#line 1549
        _S21 = 5U;

#line 1549
    }

#line 1549
    return _S21;
}


#line 216
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 1460
float punctual_visibility_0(uint tile_3, float3 world_position_3, float3 to_light_3, float n_dot_l_2, float texel_world_0, float3 geometric_normal_2, KernelContext_0 thread* kernelContext_3)
{

#line 1467
    float4 clip_1 = (((float4(world_position_3 + to_light_3 * float3((texel_world_0 * (2.0f + 4.0f * shadow_slope_0(geometric_normal_2, to_light_3)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 1474
    float _S22 = clip_1.w;

#line 1474
    if(_S22 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S22) ;

#line 1478
    bool _S23;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1479
        _S23 = true;

#line 1479
    }
    else
    {

#line 1479
        _S23 = (ndc_1.z) <= 0.0f;

#line 1479
    }

#line 1479
    if(_S23)
    {

#line 1479
        _S23 = true;

#line 1479
    }
    else
    {

#line 1479
        _S23 = (ndc_1.z) > 1.0f;

#line 1479
    }

#line 1479
    if(_S23)
    {

#line 1486
        return 1.0f;
    }

#line 1486
    float _S24 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 1492
    return _S24;
}


#line 1557
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_4, float3 to_light_4, float n_dot_l_3, float3 geometric_normal_3, KernelContext_0 thread* kernelContext_4)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_4 - (float4(light_0->position_1) ).xyz;

#line 1565
    float _S25 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_4, to_light_4, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 768.0f, geometric_normal_3, kernelContext_4);

#line 1571
    return _S25;
}


#line 1499
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_5, float3 to_light_5, float n_dot_l_4, float3 geometric_normal_4, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 1506
    float4 _S26 = float4(light_1->direction_0) ;

#line 1513
    float cos_outer_1 = _S26.w;

#line 1513
    float _S27 = punctual_visibility_0(tile_4, world_position_5, to_light_5, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (float4(light_1->position_1) ).xyz, normalize(_S26.xyz)), 0.0f) / 768.0f, geometric_normal_4, kernelContext_5);

#line 1520
    return _S27;
}


#line 635
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 1653
GpuProbe_0 probe_at_0(uint3 cell_0, KernelContext_0 thread* kernelContext_6)
{

    GpuProbe_natural_0 _S28 = kernelContext_6->probes_0[min((cell_0.z * kernelContext_6->frame_0->probe_counts_0.y + cell_0.y) * kernelContext_6->frame_0->probe_counts_0.x + cell_0.x, max(kernelContext_6->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 1656
    GpuProbe_0 _S29 = { float4(_S28.sh_r_0) , float4(_S28.sh_g_0) , float4(_S28.sh_b_0)  };

#line 1656
    return _S29;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_0)
{
    thread GpuProbe_0 blended_0;
    float4 _S30 = float4(t_0) ;

#line 1664
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S30);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S30);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S30);
    return blended_0;
}


#line 1704
float3 probe_irradiance_0(float3 world_position_6, float3 normal_2, KernelContext_0 thread* kernelContext_7)
{

#line 1704
    float3 _S31 = float3(1.0f) ;

#line 1709
    float3 _S32 = float3(0.0f, 0.0f, 0.0f);

#line 1709
    float3 last_0 = max(float3(kernelContext_7->frame_0->probe_counts_0.xyz) - _S31, _S32);
    float3 grid_1 = clamp((world_position_6 - kernelContext_7->frame_0->probe_origin_0.xyz) * kernelContext_7->frame_0->probe_inv_spacing_0.xyz, _S32, last_0);

    float3 base_2 = floor(grid_1);
    float3 f_0 = grid_1 - base_2;

    uint3 _S33 = uint3(base_2);



    uint3 _S34 = uint3(min(base_2 + _S31, last_0));

#line 1726
    uint _S35 = _S33.x;

#line 1726
    uint _S36 = _S33.y;

#line 1726
    uint _S37 = _S33.z;

#line 1726
    GpuProbe_0 _S38 = probe_at_0(uint3(_S35, _S36, _S37), kernelContext_7);

#line 1726
    uint _S39 = _S34.x;

#line 1726
    GpuProbe_0 _S40 = probe_at_0(uint3(_S39, _S36, _S37), kernelContext_7);

#line 1726
    float _S41 = f_0.x;

#line 1726
    thread GpuProbe_0 _S42 = _S38;

#line 1726
    thread GpuProbe_0 _S43 = _S40;

#line 1726
    GpuProbe_0 _S44 = lerp_probe_0(&_S42, &_S43, _S41);
    uint _S45 = _S34.y;

#line 1727
    GpuProbe_0 _S46 = probe_at_0(uint3(_S35, _S45, _S37), kernelContext_7);

#line 1727
    GpuProbe_0 _S47 = probe_at_0(uint3(_S39, _S45, _S37), kernelContext_7);

#line 1727
    thread GpuProbe_0 _S48 = _S46;

#line 1727
    thread GpuProbe_0 _S49 = _S47;

#line 1727
    GpuProbe_0 _S50 = lerp_probe_0(&_S48, &_S49, _S41);
    uint _S51 = _S34.z;

#line 1728
    GpuProbe_0 _S52 = probe_at_0(uint3(_S35, _S36, _S51), kernelContext_7);

#line 1728
    GpuProbe_0 _S53 = probe_at_0(uint3(_S39, _S36, _S51), kernelContext_7);

#line 1728
    thread GpuProbe_0 _S54 = _S52;

#line 1728
    thread GpuProbe_0 _S55 = _S53;

#line 1728
    GpuProbe_0 _S56 = lerp_probe_0(&_S54, &_S55, _S41);

#line 1728
    GpuProbe_0 _S57 = probe_at_0(uint3(_S35, _S45, _S51), kernelContext_7);

#line 1728
    GpuProbe_0 _S58 = probe_at_0(uint3(_S39, _S45, _S51), kernelContext_7);

#line 1728
    thread GpuProbe_0 _S59 = _S57;

#line 1728
    thread GpuProbe_0 _S60 = _S58;

#line 1728
    GpuProbe_0 _S61 = lerp_probe_0(&_S59, &_S60, _S41);

    float _S62 = f_0.y;

#line 1730
    thread GpuProbe_0 _S63 = _S44;

#line 1730
    thread GpuProbe_0 _S64 = _S50;

#line 1730
    GpuProbe_0 _S65 = lerp_probe_0(&_S63, &_S64, _S62);

#line 1730
    thread GpuProbe_0 _S66 = _S56;

#line 1730
    thread GpuProbe_0 _S67 = _S61;

#line 1730
    GpuProbe_0 _S68 = lerp_probe_0(&_S66, &_S67, _S62);

    float _S69 = f_0.z;

#line 1732
    thread GpuProbe_0 _S70 = _S65;

#line 1732
    thread GpuProbe_0 _S71 = _S68;

#line 1732
    GpuProbe_0 _S72 = lerp_probe_0(&_S70, &_S71, _S69);

    float4 basis_1 = float4(normal_2, 1.0f);
    return max(float3(dot(_S72.sh_r_0, basis_1), dot(_S72.sh_g_0, basis_1), dot(_S72.sh_b_0, basis_1)), _S32);
}


#line 608
float3 emissive_of_0(const GpuMaterial_natural_0 thread* material_1)
{
    return float3(material_1->emissive_r_0, material_1->emissive_g_0, material_1->emissive_b_0);
}


#line 1756
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
};


#line 1756
struct pixelInput_0
{
    float3 world_position_7 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_2 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 1811
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S73 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 1811
    thread KernelContext_0 kernelContext_8;

#line 1811
    (&kernelContext_8)->draw_0 = draw_1;

#line 1811
    (&kernelContext_8)->visible_instances_0 = visible_instances_1;

#line 1811
    (&kernelContext_8)->instances_0 = instances_1;

#line 1811
    (&kernelContext_8)->meshes_0 = meshes_1;

#line 1811
    (&kernelContext_8)->vertices_0 = vertices_1;

#line 1811
    (&kernelContext_8)->frame_0 = frame_1;

#line 1811
    (&kernelContext_8)->materials_0 = materials_1;

#line 1811
    (&kernelContext_8)->base_color_textures_0 = base_color_textures_1;

#line 1811
    (&kernelContext_8)->base_color_sampler_0 = base_color_sampler_1;

#line 1811
    (&kernelContext_8)->cluster_lights_0 = cluster_lights_1;

#line 1811
    (&kernelContext_8)->lights_0 = lights_1;

#line 1811
    (&kernelContext_8)->shadow_atlas_0 = shadow_atlas_1;

#line 1811
    (&kernelContext_8)->shadow_sampler_0 = shadow_sampler_1;

#line 1811
    (&kernelContext_8)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1811
    (&kernelContext_8)->probes_0 = probes_1;

#line 1817
    float3 normal_3 = normalize(_S73.world_normal_0);

#line 1835
    if((frame_1->ambient_0.w) >= 1.5f)
    {
        thread FragmentOutput_0 tint_0;



        (&tint_0)->lit_0 = float4(_S73.color_2.xyz, 1.0f);
        (&tint_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return tint_0;
    }

    if((frame_1->ambient_0.w) >= 0.5f)
    {
        thread FragmentOutput_0 normals_0;

#line 1848
        float3 _S74 = float3(0.5f) ;

#line 1855
        (&normals_0)->lit_0 = float4(normal_3 * _S74 + _S74, 1.0f);

#line 1861
        (&normals_0)->reflectivity_0 = float4(0.0f, 0.0f, 0.0f, 0.0f);
        return normals_0;
    }

    float3 to_eye_0 = normalize((&kernelContext_8)->frame_0->camera_position_0.xyz - _S73.world_position_7);



    float3 _S75 = geometric_normal_of_0(_S73.world_position_7, normal_3);

#line 1869
    thread GpuMaterial_natural_0 _S76 = (&kernelContext_8)->materials_0[_S73.material_2];

#line 1869
    float2 uv_2;

#line 1888
    if(((&_S76)->tiling_0) == 1U)
    {

#line 1888
        uv_2 = physical_tile_uv_0(_S73.world_position_7, normal_3, (&_S76)->tile_metres_1);

#line 1888
    }
    else
    {

#line 1888
        uv_2 = _S73.uv_1;

#line 1888
    }

#line 1893
    float3 _S77 = float3(uv_2, float((&_S76)->base_color_texture_0));
    float4 albedo_0 = _S73.color_2 * float4((&_S76)->base_color_0)  * (((&kernelContext_8)->base_color_textures_0).sample(((&kernelContext_8)->base_color_sampler_0), ((_S77)).xy, uint(((_S77)).z)));

#line 1900
    float metallic_1 = saturate((&_S76)->metallic_0);
    float roughness_1 = clamp((&_S76)->roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_1 * roughness_1;
    float _S78 = alpha_0 * alpha_0;

#line 1909
    float3 _S79 = albedo_0.xyz;

#line 1909
    float3 f0_1 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S79, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S79 * float3((1.0f - metallic_1)) ;

#line 1916
    float _S80 = max(dot(normal_3, to_eye_0), 0.00009999999747379f);

#line 1926
    float2 _S81 = position_2.xy;

#line 1926
    uint _S82 = froxel_of_0(_S81, (((float4(_S73.world_position_7, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_8);

#line 1926
    uint base_3 = _S82 * 17U;

#line 1931
    uint _S83 = min((&kernelContext_8)->cluster_lights_0[base_3], 16U);

#line 1937
    float3 _S84 = float3(0.0f, 0.0f, 0.0f);

#line 1937
    uint slot_0 = 0U;

#line 1937
    float3 direct_0 = _S84;

#line 1937
    float3 gloss_0 = _S84;

    for(;;)
    {

#line 1939
        if(slot_0 < _S83)
        {
        }
        else
        {

#line 1939
            break;
        }

#line 1939
        thread GpuLight_natural_0 _S85 = (&kernelContext_8)->lights_0[(&kernelContext_8)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 1939
        uint _S86 = (&_S85)->kind_0;

#line 1948
        bool _S87 = ((&_S85)->kind_0) == 0U;

#line 1948
        float3 to_light_6;

#line 1948
        float reach_0;

#line 1948
        if(_S87)
        {

#line 1948
            to_light_6 = normalize((float4((&_S85)->direction_0) ).xyz);

#line 1948
            reach_0 = 1.0f;

#line 1948
        }
        else
        {

#line 1948
            float4 _S88 = float4((&_S85)->position_1) ;

#line 1955
            float3 offset_0 = _S88.xyz - _S73.world_position_7;
            float distance_1 = length(offset_0);
            float3 to_light_7 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S88.w);
            if(_S86 == 2U)
            {

#line 1959
                float4 _S89 = float4((&_S85)->direction_0) ;

#line 1959
                reach_0 = reach_1 * spot_cone_0(to_light_7, _S89.xyz, _S89.w, (&_S85)->cos_inner_0);

#line 1959
            }
            else
            {

#line 1959
                reach_0 = reach_1;

#line 1959
            }

#line 1959
            to_light_6 = to_light_7;

#line 1948
        }

#line 1966
        float n_dot_l_5 = dot(normal_3, to_light_6);
        float _S90 = max(n_dot_l_5, 0.0f);

#line 1973
        float3 half_vector_0 = normalize(to_light_6 + to_eye_0);

#line 1980
        float3 specular_0 = ggx_lobe_0(_S78, f0_1, _S90, _S80, max(dot(normal_3, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S90) ;

#line 1980
        float reach_2;

#line 1995
        if(_S87)
        {

#line 1995
            float _S91 = sun_visibility_0(_S73.world_position_7, to_light_6, n_dot_l_5, _S75, &kernelContext_8);

#line 1995
            reach_2 = _S91;

#line 1995
        }
        else
        {

            if(_S86 == 1U)
            {

#line 1999
                uint _S92 = (&_S85)->shadow_tile_0;

#line 2011
                if(((&_S85)->shadow_tile_0) <= 8U)
                {

#line 2011
                    float _S93 = point_visibility_0(&_S85, _S92, _S73.world_position_7, to_light_6, n_dot_l_5, _S75, &kernelContext_8);

#line 2011
                    reach_2 = reach_0 * _S93;

#line 2011
                }
                else
                {

#line 2011
                    reach_2 = reach_0;

#line 2011
                }

#line 1999
            }
            else
            {

#line 1999
                uint _S94 = (&_S85)->shadow_tile_0;

#line 2017
                if(((&_S85)->shadow_tile_0) < 14U)
                {

#line 2017
                    float _S95 = spot_visibility_0(&_S85, _S94, _S73.world_position_7, to_light_6, n_dot_l_5, _S75, &kernelContext_8);

#line 2017
                    reach_2 = reach_0 * _S95;

#line 2017
                }
                else
                {

#line 2017
                    reach_2 = reach_0;

#line 2017
                }

#line 1999
            }

#line 1995
        }

#line 2025
        float3 _S96 = (float4((&_S85)->color_1) ).xyz;

#line 2025
        float3 direct_1 = direct_0 + _S96 * float3((_S90 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S96 * (specular_0 * float3(reach_2) );

#line 1939
        slot_0 = slot_0 + 1U;

#line 1939
        direct_0 = direct_1;

#line 1939
        gloss_0 = gloss_1;

#line 1939
    }

#line 1939
    texture2d<float, access::sample> _S97 = (&kernelContext_8)->ambient_occlusion_0;

#line 2045
    thread uint occlusion_width_0;
    thread uint occlusion_height_0;
    (*((&occlusion_width_0)) = (_S97).get_width(0)),(*((&occlusion_height_0)) = (_S97).get_height(0));


    int3 _S98 = int3(min(int2(_S81), int2(int(occlusion_width_0), int(occlusion_height_0)) - int2(int(1)) ), int(0));

#line 2050
    float occluded_0 = (((&kernelContext_8)->ambient_occlusion_0).read(vec<uint,2>(((_S98)).xy), uint(((_S98)).z)).x);

#line 2063
    float3 _S99 = frame_1->ambient_0.xyz;

#line 2063
    float3 _S100 = probe_irradiance_0(_S73.world_position_7, normal_3, &kernelContext_8);

#line 2083
    float3 lit_1 = diffuse_albedo_0 * ((_S99 + _S100) * float3(occluded_0)  + direct_0) + gloss_0;

#line 2083
    float3 _S101 = emissive_of_0(&_S76);

#line 2097
    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4(lit_1 + _S101, albedo_0.w);

#line 2106
    (&output_0)->reflectivity_0 = float4(f0_1, saturate(1.0f - roughness_1 / 0.5f));
    return output_0;
}


#line 2107
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
};


#line 997
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_9;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_4;
};


#line 997
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 997
    thread KernelContext_0 kernelContext_9;

#line 997
    (&kernelContext_9)->draw_0 = draw_2;

#line 997
    (&kernelContext_9)->visible_instances_0 = visible_instances_2;

#line 997
    (&kernelContext_9)->instances_0 = instances_2;

#line 997
    (&kernelContext_9)->meshes_0 = meshes_2;

#line 997
    (&kernelContext_9)->vertices_0 = vertices_2;

#line 997
    (&kernelContext_9)->frame_0 = frame_2;

#line 997
    (&kernelContext_9)->materials_0 = materials_2;

#line 997
    (&kernelContext_9)->base_color_textures_0 = base_color_textures_2;

#line 997
    (&kernelContext_9)->base_color_sampler_0 = base_color_sampler_2;

#line 997
    (&kernelContext_9)->cluster_lights_0 = cluster_lights_2;

#line 997
    (&kernelContext_9)->lights_0 = lights_2;

#line 997
    (&kernelContext_9)->shadow_atlas_0 = shadow_atlas_2;

#line 997
    (&kernelContext_9)->shadow_sampler_0 = shadow_sampler_2;

#line 997
    (&kernelContext_9)->ambient_occlusion_0 = ambient_occlusion_2;

#line 997
    (&kernelContext_9)->probes_0 = probes_2;

#line 1037
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 1043
    GpuMesh_0 mesh_2 = meshes_2[draw_2->mesh_0];

#line 1043
    uint base_vertex_2;

#line 1052
    if(((instance_0.flags_0) & 2U) != 0U)
    {

#line 1052
        base_vertex_2 = instance_0.base_vertex_0;

#line 1052
    }
    else
    {

#line 1052
        base_vertex_2 = mesh_2.base_vertex_1;

#line 1052
    }

    MeshVertex_natural_0 vertex_0 = (&kernelContext_9)->vertices_0[index_1 + base_vertex_2];

#line 1054
    matrix<float,int(4),int(4)>  _S102 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S102)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> ((&kernelContext_9)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_9)->frame_0->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_9 = world_0.xyz;

#line 1066
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_1) ).xyz) * (normal_basis_0(matrix<float,int(3),int(3)> (_S102[int(0)].xyz, _S102[int(1)].xyz, _S102[int(2)].xyz)))));

#line 1066
    float4 _S103;

#line 1073
    if(((&kernelContext_9)->frame_0->ambient_0.w) >= 1.5f)
    {

#line 1073
        _S103 = float4(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);

#line 1073
    }
    else
    {

#line 1073
        _S103 = float4(vertex_0.color_0) ;

#line 1073
    }

#line 1072
    (&output_1)->color_4 = _S103;

#line 1079
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_4 = (float4(vertex_0.uv_0) ).xy;
    VertexOutput_0 _S104 = output_1;

#line 1081
    thread vertexMain_Result_0 _S105;

#line 1081
    (&_S105)->position_3 = _S104.position_4;

#line 1081
    (&_S105)->world_position_8 = _S104.world_position_9;

#line 1081
    (&_S105)->world_normal_1 = _S104.world_normal_2;

#line 1081
    (&_S105)->color_3 = _S104.color_4;

#line 1081
    (&_S105)->material_3 = _S104.material_4;

#line 1081
    (&_S105)->uv_3 = _S104.uv_4;

#line 1081
    return _S105;
}

