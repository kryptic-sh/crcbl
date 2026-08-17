#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 1029 "shaders/mesh.slang"
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 1040
    float3 _S1;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 1041
        _S1 = - facet_1;

#line 1041
    }
    else
    {

#line 1041
        _S1 = facet_1;

#line 1041
    }

#line 1041
    return _S1;
}


#line 1603
float2 physical_tile_uv_0(float3 world_position_1, float3 normal_0, float tile_metres_0)
{
    float3 axis_0 = abs(normal_0);

    float _S2 = axis_0.x;

#line 1607
    float _S3 = axis_0.y;

#line 1607
    bool _S4;

#line 1607
    if(_S2 >= _S3)
    {

#line 1607
        _S4 = _S2 >= (axis_0.z);

#line 1607
    }
    else
    {

#line 1607
        _S4 = false;

#line 1607
    }

#line 1607
    float2 planar_0;

#line 1607
    if(_S4)
    {

#line 1607
        planar_0 = world_position_1.zy;

#line 1607
    }
    else
    {

        if(_S3 >= (axis_0.z))
        {

#line 1611
            planar_0 = world_position_1.xz;

#line 1611
        }
        else
        {

#line 1611
            planar_0 = world_position_1.xy;

#line 1611
        }

#line 1607
    }

#line 1619
    return planar_0 / float2(max(tile_metres_0, 0.00009999999747379f)) ;
}


#line 603
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 867
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 867
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 406
struct GpuMesh_0
{
    uint base_vertex_0;
    uint base_index_0;
    uint index_count_0;
    float min_x_0;
    float min_y_0;
    float min_z_0;
    float max_x_0;
    float max_y_0;
    float max_z_0;
};


#line 873
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_1;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 873
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 873
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E6_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(6)> data_3;
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
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E6_0 light_view_proj_0;
    float4 probe_origin_0;
    float4 probe_inv_spacing_0;
    uint4 probe_counts_0;
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
    uint pad0_1;
    uint pad1_1;
    uint pad2_0;
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


#line 1434 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S5 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S6 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S7 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S8 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1444
    uint _S9 = uint(pixel_0.x) / _S8;

#line 1444
    uint _S10 = min(_S9, _S5 - 1U);
    uint _S11 = uint(pixel_0.y) / _S8;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1455
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S7 - 1U))) * _S6 + min(_S11, _S6 - 1U)) * _S5 + _S10;
}


#line 1399
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 1413
float spot_cone_0(float3 to_light_0, float3 axis_1, float cos_outer_0, float cos_inner_1)
{

#line 1420
    return saturate((dot(- to_light_0, normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 952
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 959
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 966
    float _S12 = 1.0f - alpha2_0;

#line 971
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S12 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S12 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 1056
float shadow_slope_0(float3 geometric_normal_0, float3 to_light_1)
{
    float cosine_0 = saturate(dot(geometric_normal_0, to_light_1));

    return min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f);
}


#line 223
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 2.0f);
}


#line 1070
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 1077
    float2 grid_0 = float2(4.0f, 2.0f);
    float2 _S13 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 1078
    int y_0 = int(-1);

#line 1078
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 1080
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 1080
            break;
        }

#line 1080
        int x_0 = int(-1);

        for(;;)
        {

#line 1082
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 1082
                break;
            }



            float _S14 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S13, float2(1.0f)  - _S13))), (reference_0), level((0.0f))));

#line 1086
            float visibility_1 = visibility_0 + _S14;

#line 1082
            x_0 = x_0 + int(1);

#line 1082
            visibility_0 = visibility_1;

#line 1082
        }

#line 1080
        y_0 = y_0 + int(1);

#line 1080
    }

#line 1090
    return visibility_0 / 9.0f;
}


#line 1106
float sun_visibility_0(float3 world_position_2, float3 to_light_2, float n_dot_l_1, float3 geometric_normal_1, KernelContext_0 thread* kernelContext_2)
{

#line 1107
    uint cascade_0;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 1119
    float _S15 = length(world_position_2 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 1119
    uint index_0 = 0U;

    for(;;)
    {

#line 1121
        if(index_0 < 2U)
        {
        }
        else
        {

#line 1121
            cascade_0 = 1U;

#line 1121
            break;
        }
        if(_S15 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 1123
            cascade_0 = index_0;


            break;
        }

#line 1121
        index_0 = index_0 + 1U;

#line 1121
    }

#line 1157
    float4 clip_0 = (((float4(world_position_2 + to_light_2 * float3((2.0f * kernelContext_2->frame_0->cascade_far_0[cascade_0] / 1024.0f * (kernelContext_2->frame_0->shadow_params_0.z + kernelContext_2->frame_0->shadow_params_0.w * shadow_slope_0(geometric_normal_1, to_light_2)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));

#line 1162
    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1162
    bool _S16;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1163
        _S16 = true;

#line 1163
    }
    else
    {

#line 1163
        _S16 = (ndc_0.z) <= 0.0f;

#line 1163
    }

#line 1163
    if(_S16)
    {



        return 1.0f;
    }

#line 1168
    float _S17 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z, kernelContext_2);

#line 1182
    return _S17;
}


#line 1350
uint point_face_0(float3 from_light_0)
{
    float3 axis_2 = abs(from_light_0);
    float _S18 = axis_2.x;

#line 1353
    float _S19 = axis_2.y;

#line 1353
    bool _S20;

#line 1353
    if(_S18 >= _S19)
    {

#line 1353
        _S20 = _S18 >= (axis_2.z);

#line 1353
    }
    else
    {

#line 1353
        _S20 = false;

#line 1353
    }

#line 1353
    uint _S21;

#line 1353
    if(_S20)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1355
            _S21 = 0U;

#line 1355
        }
        else
        {

#line 1355
            _S21 = 1U;

#line 1355
        }

#line 1355
        return _S21;
    }
    if(_S19 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1359
            _S21 = 2U;

#line 1359
        }
        else
        {

#line 1359
            _S21 = 3U;

#line 1359
        }

#line 1359
        return _S21;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1361
        _S21 = 4U;

#line 1361
    }
    else
    {

#line 1361
        _S21 = 5U;

#line 1361
    }

#line 1361
    return _S21;
}


#line 211
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 1272
float punctual_visibility_0(uint tile_3, float3 world_position_3, float3 to_light_3, float n_dot_l_2, float texel_world_0, float3 geometric_normal_2, KernelContext_0 thread* kernelContext_3)
{

#line 1279
    float4 clip_1 = (((float4(world_position_3 + to_light_3 * float3((texel_world_0 * (2.0f + 4.0f * shadow_slope_0(geometric_normal_2, to_light_3)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 1286
    float _S22 = clip_1.w;

#line 1286
    if(_S22 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S22) ;

#line 1290
    bool _S23;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1291
        _S23 = true;

#line 1291
    }
    else
    {

#line 1291
        _S23 = (ndc_1.z) <= 0.0f;

#line 1291
    }

#line 1291
    if(_S23)
    {

#line 1291
        _S23 = true;

#line 1291
    }
    else
    {

#line 1291
        _S23 = (ndc_1.z) > 1.0f;

#line 1291
    }

#line 1291
    if(_S23)
    {

#line 1298
        return 1.0f;
    }

#line 1298
    float _S24 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 1304
    return _S24;
}


#line 1369
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_4, float3 to_light_4, float n_dot_l_3, float3 geometric_normal_3, KernelContext_0 thread* kernelContext_4)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_4 - (float4(light_0->position_1) ).xyz;

#line 1377
    float _S25 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_4, to_light_4, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 1024.0f, geometric_normal_3, kernelContext_4);

#line 1383
    return _S25;
}


#line 1311
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_5, float3 to_light_5, float n_dot_l_4, float3 geometric_normal_4, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 1318
    float4 _S26 = float4(light_1->direction_0) ;

#line 1325
    float cos_outer_1 = _S26.w;

#line 1325
    float _S27 = punctual_visibility_0(tile_4, world_position_5, to_light_5, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_5 - (float4(light_1->position_1) ).xyz, normalize(_S26.xyz)), 0.0f) / 1024.0f, geometric_normal_4, kernelContext_5);

#line 1332
    return _S27;
}


#line 514
struct GpuProbe_0
{
    float4 sh_r_0;
    float4 sh_g_0;
    float4 sh_b_0;
};


#line 1465
GpuProbe_0 probe_at_0(uint3 cell_0, KernelContext_0 thread* kernelContext_6)
{

    GpuProbe_natural_0 _S28 = kernelContext_6->probes_0[min((cell_0.z * kernelContext_6->frame_0->probe_counts_0.y + cell_0.y) * kernelContext_6->frame_0->probe_counts_0.x + cell_0.x, max(kernelContext_6->frame_0->probe_counts_0.w, 1U) - 1U)];

#line 1468
    GpuProbe_0 _S29 = { float4(_S28.sh_r_0) , float4(_S28.sh_g_0) , float4(_S28.sh_b_0)  };

#line 1468
    return _S29;
}



GpuProbe_0 lerp_probe_0(const GpuProbe_0 thread* a_0, const GpuProbe_0 thread* b_0, float t_0)
{
    thread GpuProbe_0 blended_0;
    float4 _S30 = float4(t_0) ;

#line 1476
    (&blended_0)->sh_r_0 = mix(a_0->sh_r_0, b_0->sh_r_0, _S30);
    (&blended_0)->sh_g_0 = mix(a_0->sh_g_0, b_0->sh_g_0, _S30);
    (&blended_0)->sh_b_0 = mix(a_0->sh_b_0, b_0->sh_b_0, _S30);
    return blended_0;
}


#line 1516
float3 probe_irradiance_0(float3 world_position_6, float3 normal_2, KernelContext_0 thread* kernelContext_7)
{

#line 1516
    float3 _S31 = float3(1.0f) ;

#line 1521
    float3 _S32 = float3(0.0f, 0.0f, 0.0f);

#line 1521
    float3 last_0 = max(float3(kernelContext_7->frame_0->probe_counts_0.xyz) - _S31, _S32);
    float3 grid_1 = clamp((world_position_6 - kernelContext_7->frame_0->probe_origin_0.xyz) * kernelContext_7->frame_0->probe_inv_spacing_0.xyz, _S32, last_0);

    float3 base_2 = floor(grid_1);
    float3 f_0 = grid_1 - base_2;

    uint3 _S33 = uint3(base_2);



    uint3 _S34 = uint3(min(base_2 + _S31, last_0));

#line 1538
    uint _S35 = _S33.x;

#line 1538
    uint _S36 = _S33.y;

#line 1538
    uint _S37 = _S33.z;

#line 1538
    GpuProbe_0 _S38 = probe_at_0(uint3(_S35, _S36, _S37), kernelContext_7);

#line 1538
    uint _S39 = _S34.x;

#line 1538
    GpuProbe_0 _S40 = probe_at_0(uint3(_S39, _S36, _S37), kernelContext_7);

#line 1538
    float _S41 = f_0.x;

#line 1538
    thread GpuProbe_0 _S42 = _S38;

#line 1538
    thread GpuProbe_0 _S43 = _S40;

#line 1538
    GpuProbe_0 _S44 = lerp_probe_0(&_S42, &_S43, _S41);
    uint _S45 = _S34.y;

#line 1539
    GpuProbe_0 _S46 = probe_at_0(uint3(_S35, _S45, _S37), kernelContext_7);

#line 1539
    GpuProbe_0 _S47 = probe_at_0(uint3(_S39, _S45, _S37), kernelContext_7);

#line 1539
    thread GpuProbe_0 _S48 = _S46;

#line 1539
    thread GpuProbe_0 _S49 = _S47;

#line 1539
    GpuProbe_0 _S50 = lerp_probe_0(&_S48, &_S49, _S41);
    uint _S51 = _S34.z;

#line 1540
    GpuProbe_0 _S52 = probe_at_0(uint3(_S35, _S36, _S51), kernelContext_7);

#line 1540
    GpuProbe_0 _S53 = probe_at_0(uint3(_S39, _S36, _S51), kernelContext_7);

#line 1540
    thread GpuProbe_0 _S54 = _S52;

#line 1540
    thread GpuProbe_0 _S55 = _S53;

#line 1540
    GpuProbe_0 _S56 = lerp_probe_0(&_S54, &_S55, _S41);

#line 1540
    GpuProbe_0 _S57 = probe_at_0(uint3(_S35, _S45, _S51), kernelContext_7);

#line 1540
    GpuProbe_0 _S58 = probe_at_0(uint3(_S39, _S45, _S51), kernelContext_7);

#line 1540
    thread GpuProbe_0 _S59 = _S57;

#line 1540
    thread GpuProbe_0 _S60 = _S58;

#line 1540
    GpuProbe_0 _S61 = lerp_probe_0(&_S59, &_S60, _S41);

    float _S62 = f_0.y;

#line 1542
    thread GpuProbe_0 _S63 = _S44;

#line 1542
    thread GpuProbe_0 _S64 = _S50;

#line 1542
    GpuProbe_0 _S65 = lerp_probe_0(&_S63, &_S64, _S62);

#line 1542
    thread GpuProbe_0 _S66 = _S56;

#line 1542
    thread GpuProbe_0 _S67 = _S61;

#line 1542
    GpuProbe_0 _S68 = lerp_probe_0(&_S66, &_S67, _S62);

    float _S69 = f_0.z;

#line 1544
    thread GpuProbe_0 _S70 = _S65;

#line 1544
    thread GpuProbe_0 _S71 = _S68;

#line 1544
    GpuProbe_0 _S72 = lerp_probe_0(&_S70, &_S71, _S69);

    float4 basis_0 = float4(normal_2, 1.0f);
    return max(float3(dot(_S72.sh_r_0, basis_0), dot(_S72.sh_g_0, basis_0), dot(_S72.sh_b_0, basis_0)), _S32);
}


#line 1568
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
};


#line 1568
struct pixelInput_0
{
    float3 world_position_7 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 1623
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S73 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]], GpuProbe_natural_0 device* probes_1 [[buffer(9)]])
{

#line 1623
    thread KernelContext_0 kernelContext_8;

#line 1623
    (&kernelContext_8)->draw_0 = draw_1;

#line 1623
    (&kernelContext_8)->visible_instances_0 = visible_instances_1;

#line 1623
    (&kernelContext_8)->instances_0 = instances_1;

#line 1623
    (&kernelContext_8)->meshes_0 = meshes_1;

#line 1623
    (&kernelContext_8)->vertices_0 = vertices_1;

#line 1623
    (&kernelContext_8)->frame_0 = frame_1;

#line 1623
    (&kernelContext_8)->materials_0 = materials_1;

#line 1623
    (&kernelContext_8)->base_color_textures_0 = base_color_textures_1;

#line 1623
    (&kernelContext_8)->base_color_sampler_0 = base_color_sampler_1;

#line 1623
    (&kernelContext_8)->cluster_lights_0 = cluster_lights_1;

#line 1623
    (&kernelContext_8)->lights_0 = lights_1;

#line 1623
    (&kernelContext_8)->shadow_atlas_0 = shadow_atlas_1;

#line 1623
    (&kernelContext_8)->shadow_sampler_0 = shadow_sampler_1;

#line 1623
    (&kernelContext_8)->ambient_occlusion_0 = ambient_occlusion_1;

#line 1623
    (&kernelContext_8)->probes_0 = probes_1;



    float3 normal_3 = normalize(_S73.world_normal_0);
    float3 to_eye_0 = normalize(frame_1->camera_position_0.xyz - _S73.world_position_7);



    float3 _S74 = geometric_normal_of_0(_S73.world_position_7, normal_3);



    GpuMaterial_natural_0 material_2 = materials_1[_S73.material_1];

#line 1636
    float2 uv_2;

#line 1651
    if((material_2.tiling_0) == 1U)
    {

#line 1651
        uv_2 = physical_tile_uv_0(_S73.world_position_7, normal_3, material_2.tile_metres_1);

#line 1651
    }
    else
    {

#line 1651
        uv_2 = _S73.uv_1;

#line 1651
    }

#line 1656
    float3 _S75 = float3(uv_2, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S73.color_2 * float4(material_2.base_color_0)  * (((&kernelContext_8)->base_color_textures_0).sample(((&kernelContext_8)->base_color_sampler_0), ((_S75)).xy, uint(((_S75)).z)));

#line 1663
    float metallic_1 = saturate(material_2.metallic_0);
    float roughness_1 = clamp(material_2.roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_1 * roughness_1;
    float _S76 = alpha_0 * alpha_0;

#line 1672
    float3 _S77 = albedo_0.xyz;

#line 1672
    float3 f0_1 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S77, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S77 * float3((1.0f - metallic_1)) ;

#line 1679
    float _S78 = max(dot(normal_3, to_eye_0), 0.00009999999747379f);

#line 1689
    float2 _S79 = position_2.xy;

#line 1689
    uint _S80 = froxel_of_0(_S79, (((float4(_S73.world_position_7, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(0)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(1)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(2)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(0)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(1)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(2)][int(3)], (&kernelContext_8)->frame_0->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_8);

#line 1689
    uint base_3 = _S80 * 17U;

#line 1694
    uint _S81 = min((&kernelContext_8)->cluster_lights_0[base_3], 16U);

#line 1700
    float3 _S82 = float3(0.0f, 0.0f, 0.0f);

#line 1700
    uint slot_0 = 0U;

#line 1700
    float3 direct_0 = _S82;

#line 1700
    float3 gloss_0 = _S82;

    for(;;)
    {

#line 1702
        if(slot_0 < _S81)
        {
        }
        else
        {

#line 1702
            break;
        }

#line 1702
        thread GpuLight_natural_0 _S83 = (&kernelContext_8)->lights_0[(&kernelContext_8)->cluster_lights_0[base_3 + 1U + slot_0]];

#line 1702
        uint _S84 = (&_S83)->kind_0;

#line 1711
        bool _S85 = ((&_S83)->kind_0) == 0U;

#line 1711
        float3 to_light_6;

#line 1711
        float reach_0;

#line 1711
        if(_S85)
        {

#line 1711
            to_light_6 = normalize((float4((&_S83)->direction_0) ).xyz);

#line 1711
            reach_0 = 1.0f;

#line 1711
        }
        else
        {

#line 1711
            float4 _S86 = float4((&_S83)->position_1) ;

#line 1718
            float3 offset_0 = _S86.xyz - _S73.world_position_7;
            float distance_1 = length(offset_0);
            float3 to_light_7 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S86.w);
            if(_S84 == 2U)
            {

#line 1722
                float4 _S87 = float4((&_S83)->direction_0) ;

#line 1722
                reach_0 = reach_1 * spot_cone_0(to_light_7, _S87.xyz, _S87.w, (&_S83)->cos_inner_0);

#line 1722
            }
            else
            {

#line 1722
                reach_0 = reach_1;

#line 1722
            }

#line 1722
            to_light_6 = to_light_7;

#line 1711
        }

#line 1729
        float n_dot_l_5 = dot(normal_3, to_light_6);
        float _S88 = max(n_dot_l_5, 0.0f);

#line 1736
        float3 half_vector_0 = normalize(to_light_6 + to_eye_0);

#line 1743
        float3 specular_0 = ggx_lobe_0(_S76, f0_1, _S88, _S78, max(dot(normal_3, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S88) ;

#line 1743
        float reach_2;

#line 1758
        if(_S85)
        {

#line 1758
            float _S89 = sun_visibility_0(_S73.world_position_7, to_light_6, n_dot_l_5, _S74, &kernelContext_8);

#line 1758
            reach_2 = _S89;

#line 1758
        }
        else
        {

            if(_S84 == 1U)
            {

#line 1762
                uint _S90 = (&_S83)->shadow_tile_0;

#line 1774
                if(((&_S83)->shadow_tile_0) <= 0U)
                {

#line 1774
                    float _S91 = point_visibility_0(&_S83, _S90, _S73.world_position_7, to_light_6, n_dot_l_5, _S74, &kernelContext_8);

#line 1774
                    reach_2 = reach_0 * _S91;

#line 1774
                }
                else
                {

#line 1774
                    reach_2 = reach_0;

#line 1774
                }

#line 1762
            }
            else
            {

#line 1762
                uint _S92 = (&_S83)->shadow_tile_0;

#line 1780
                if(((&_S83)->shadow_tile_0) < 6U)
                {

#line 1780
                    float _S93 = spot_visibility_0(&_S83, _S92, _S73.world_position_7, to_light_6, n_dot_l_5, _S74, &kernelContext_8);

#line 1780
                    reach_2 = reach_0 * _S93;

#line 1780
                }
                else
                {

#line 1780
                    reach_2 = reach_0;

#line 1780
                }

#line 1762
            }

#line 1758
        }

#line 1788
        float3 _S94 = (float4((&_S83)->color_1) ).xyz;

#line 1788
        float3 direct_1 = direct_0 + _S94 * float3((_S88 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S94 * (specular_0 * float3(reach_2) );

#line 1702
        slot_0 = slot_0 + 1U;

#line 1702
        direct_0 = direct_1;

#line 1702
        gloss_0 = gloss_1;

#line 1702
    }

#line 1702
    texture2d<float, access::sample> _S95 = (&kernelContext_8)->ambient_occlusion_0;

#line 1808
    thread uint occlusion_width_0;
    thread uint occlusion_height_0;
    (*((&occlusion_width_0)) = (_S95).get_width(0)),(*((&occlusion_height_0)) = (_S95).get_height(0));


    int3 _S96 = int3(min(int2(_S79), int2(int(occlusion_width_0), int(occlusion_height_0)) - int2(int(1)) ), int(0));

#line 1813
    float occluded_0 = (((&kernelContext_8)->ambient_occlusion_0).read(vec<uint,2>(((_S96)).xy), uint(((_S96)).z)).x);

#line 1826
    float3 _S97 = (&kernelContext_8)->frame_0->ambient_0.xyz;

#line 1826
    float3 _S98 = probe_irradiance_0(_S73.world_position_7, normal_3, &kernelContext_8);

#line 1848
    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4(diffuse_albedo_0 * ((_S97 + _S98) * float3(occluded_0)  + direct_0) + gloss_0, albedo_0.w);

#line 1857
    (&output_0)->reflectivity_0 = float4(f0_1, saturate(1.0f - roughness_1 / 0.5f));
    return output_0;
}


#line 1858
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_8 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_3 [[user(TEXCOORD_1)]];
};


#line 827
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_9;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_4;
};


#line 827
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]], GpuProbe_natural_0 device* probes_2 [[buffer(9)]])
{

#line 827
    thread KernelContext_0 kernelContext_9;

#line 827
    (&kernelContext_9)->draw_0 = draw_2;

#line 827
    (&kernelContext_9)->visible_instances_0 = visible_instances_2;

#line 827
    (&kernelContext_9)->instances_0 = instances_2;

#line 827
    (&kernelContext_9)->meshes_0 = meshes_2;

#line 827
    (&kernelContext_9)->vertices_0 = vertices_2;

#line 827
    (&kernelContext_9)->frame_0 = frame_2;

#line 827
    (&kernelContext_9)->materials_0 = materials_2;

#line 827
    (&kernelContext_9)->base_color_textures_0 = base_color_textures_2;

#line 827
    (&kernelContext_9)->base_color_sampler_0 = base_color_sampler_2;

#line 827
    (&kernelContext_9)->cluster_lights_0 = cluster_lights_2;

#line 827
    (&kernelContext_9)->lights_0 = lights_2;

#line 827
    (&kernelContext_9)->shadow_atlas_0 = shadow_atlas_2;

#line 827
    (&kernelContext_9)->shadow_sampler_0 = shadow_sampler_2;

#line 827
    (&kernelContext_9)->ambient_occlusion_0 = ambient_occlusion_2;

#line 827
    (&kernelContext_9)->probes_0 = probes_2;

#line 867
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 874
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[draw_2->mesh_0].base_vertex_0];

#line 874
    matrix<float,int(4),int(4)>  _S99 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S99)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_9 = world_0.xyz;

#line 885
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_1) ).xyz) * (matrix<float,int(3),int(3)> (_S99[int(0)].xyz, _S99[int(1)].xyz, _S99[int(2)].xyz))));
    (&output_1)->color_4 = float4(vertex_0.color_0) ;

#line 891
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_4 = (float4(vertex_0.uv_0) ).xy;

#line 892
    thread vertexMain_Result_0 _S100;

#line 892
    (&_S100)->position_3 = output_1.position_4;

#line 892
    (&_S100)->world_position_8 = output_1.world_position_9;

#line 892
    (&_S100)->world_normal_1 = output_1.world_normal_2;

#line 892
    (&_S100)->color_3 = output_1.color_4;

#line 892
    (&_S100)->material_3 = output_1.material_4;

#line 892
    (&_S100)->uv_3 = output_1.uv_4;

#line 892
    return _S100;
}

