#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 924 "shaders/mesh.slang"
float3 geometric_normal_of_0(float3 world_position_0, float3 shading_normal_0)
{
    float3 facet_0 = cross(dfdx(world_position_0), dfdy(world_position_0));
    float extent_0 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {



        return shading_normal_0;
    }
    float3 facet_1 = facet_0 / float3(extent_0) ;

#line 935
    float3 _S1;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {

#line 936
        _S1 = - facet_1;

#line 936
    }
    else
    {

#line 936
        _S1 = facet_1;

#line 936
    }

#line 936
    return _S1;
}


#line 532
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 769
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 769
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 385
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


#line 775
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 775
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 775
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
};


#line 3332
struct GpuMaterial_natural_0
{
    packed_float4 base_color_0;
    uint base_color_texture_0;
    float metallic_0;
    float roughness_0;
    uint pad0_1;
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
    uint pad1_1;
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
};


#line 1329 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S2 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S3 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S4 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S5 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1339
    uint _S6 = uint(pixel_0.x) / _S5;

#line 1339
    uint _S7 = min(_S6, _S2 - 1U);
    uint _S8 = uint(pixel_0.y) / _S5;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1350
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S4 - 1U))) * _S3 + min(_S8, _S3 - 1U)) * _S2 + _S7;
}


#line 1294
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 1308
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 1315
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 847
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 854
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 861
    float _S9 = 1.0f - alpha2_0;

#line 866
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S9 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S9 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 951
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


#line 965
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 972
    float2 grid_0 = float2(4.0f, 2.0f);
    float2 _S10 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 973
    int y_0 = int(-1);

#line 973
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 975
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 975
            break;
        }

#line 975
        int x_0 = int(-1);

        for(;;)
        {

#line 977
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 977
                break;
            }



            float _S11 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S10, float2(1.0f)  - _S10))), (reference_0), level((0.0f))));

#line 981
            float visibility_1 = visibility_0 + _S11;

#line 977
            x_0 = x_0 + int(1);

#line 977
            visibility_0 = visibility_1;

#line 977
        }

#line 975
        y_0 = y_0 + int(1);

#line 975
    }

#line 985
    return visibility_0 / 9.0f;
}


#line 1001
float sun_visibility_0(float3 world_position_1, float3 to_light_2, float n_dot_l_1, float3 geometric_normal_1, KernelContext_0 thread* kernelContext_2)
{

#line 1002
    uint cascade_0;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 1014
    float _S12 = length(world_position_1 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 1014
    uint index_0 = 0U;

    for(;;)
    {

#line 1016
        if(index_0 < 2U)
        {
        }
        else
        {

#line 1016
            cascade_0 = 1U;

#line 1016
            break;
        }
        if(_S12 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 1018
            cascade_0 = index_0;


            break;
        }

#line 1016
        index_0 = index_0 + 1U;

#line 1016
    }

#line 1052
    float4 clip_0 = (((float4(world_position_1 + to_light_2 * float3((2.0f * kernelContext_2->frame_0->cascade_far_0[cascade_0] / 1024.0f * (kernelContext_2->frame_0->shadow_params_0.z + kernelContext_2->frame_0->shadow_params_0.w * shadow_slope_0(geometric_normal_1, to_light_2)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));

#line 1057
    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 1057
    bool _S13;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 1058
        _S13 = true;

#line 1058
    }
    else
    {

#line 1058
        _S13 = (ndc_0.z) <= 0.0f;

#line 1058
    }

#line 1058
    if(_S13)
    {



        return 1.0f;
    }

#line 1063
    float _S14 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z, kernelContext_2);

#line 1077
    return _S14;
}


#line 1245
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S15 = axis_1.x;

#line 1248
    float _S16 = axis_1.y;

#line 1248
    bool _S17;

#line 1248
    if(_S15 >= _S16)
    {

#line 1248
        _S17 = _S15 >= (axis_1.z);

#line 1248
    }
    else
    {

#line 1248
        _S17 = false;

#line 1248
    }

#line 1248
    uint _S18;

#line 1248
    if(_S17)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1250
            _S18 = 0U;

#line 1250
        }
        else
        {

#line 1250
            _S18 = 1U;

#line 1250
        }

#line 1250
        return _S18;
    }
    if(_S16 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1254
            _S18 = 2U;

#line 1254
        }
        else
        {

#line 1254
            _S18 = 3U;

#line 1254
        }

#line 1254
        return _S18;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1256
        _S18 = 4U;

#line 1256
    }
    else
    {

#line 1256
        _S18 = 5U;

#line 1256
    }

#line 1256
    return _S18;
}


#line 211
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 1167
float punctual_visibility_0(uint tile_3, float3 world_position_2, float3 to_light_3, float n_dot_l_2, float texel_world_0, float3 geometric_normal_2, KernelContext_0 thread* kernelContext_3)
{

#line 1174
    float4 clip_1 = (((float4(world_position_2 + to_light_3 * float3((texel_world_0 * (2.0f + 4.0f * shadow_slope_0(geometric_normal_2, to_light_3)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 1181
    float _S19 = clip_1.w;

#line 1181
    if(_S19 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S19) ;

#line 1185
    bool _S20;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1186
        _S20 = true;

#line 1186
    }
    else
    {

#line 1186
        _S20 = (ndc_1.z) <= 0.0f;

#line 1186
    }

#line 1186
    if(_S20)
    {

#line 1186
        _S20 = true;

#line 1186
    }
    else
    {

#line 1186
        _S20 = (ndc_1.z) > 1.0f;

#line 1186
    }

#line 1186
    if(_S20)
    {

#line 1193
        return 1.0f;
    }

#line 1193
    float _S21 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 1199
    return _S21;
}


#line 1264
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_3, float3 to_light_4, float n_dot_l_3, float3 geometric_normal_3, KernelContext_0 thread* kernelContext_4)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_3 - (float4(light_0->position_1) ).xyz;

#line 1272
    float _S22 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_3, to_light_4, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 1024.0f, geometric_normal_3, kernelContext_4);

#line 1278
    return _S22;
}


#line 1206
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_4, float3 to_light_5, float n_dot_l_4, float3 geometric_normal_4, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 1213
    float4 _S23 = float4(light_1->direction_0) ;

#line 1220
    float cos_outer_1 = _S23.w;

#line 1220
    float _S24 = punctual_visibility_0(tile_4, world_position_4, to_light_5, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_4 - (float4(light_1->position_1) ).xyz, normalize(_S23.xyz)), 0.0f) / 1024.0f, geometric_normal_4, kernelContext_5);

#line 1227
    return _S24;
}


#line 1370
struct FragmentOutput_0
{
    float4 lit_0 [[color(0)]];
    float4 reflectivity_0 [[color(1)]];
};


#line 1370
struct pixelInput_0
{
    float3 world_position_5 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 1393
[[fragment]] FragmentOutput_0 fragmentMain(pixelInput_0 _S25 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]])
{

#line 1393
    thread KernelContext_0 kernelContext_6;

#line 1393
    (&kernelContext_6)->draw_0 = draw_1;

#line 1393
    (&kernelContext_6)->visible_instances_0 = visible_instances_1;

#line 1393
    (&kernelContext_6)->instances_0 = instances_1;

#line 1393
    (&kernelContext_6)->meshes_0 = meshes_1;

#line 1393
    (&kernelContext_6)->vertices_0 = vertices_1;

#line 1393
    (&kernelContext_6)->frame_0 = frame_1;

#line 1393
    (&kernelContext_6)->materials_0 = materials_1;

#line 1393
    (&kernelContext_6)->base_color_textures_0 = base_color_textures_1;

#line 1393
    (&kernelContext_6)->base_color_sampler_0 = base_color_sampler_1;

#line 1393
    (&kernelContext_6)->cluster_lights_0 = cluster_lights_1;

#line 1393
    (&kernelContext_6)->lights_0 = lights_1;

#line 1393
    (&kernelContext_6)->shadow_atlas_0 = shadow_atlas_1;

#line 1393
    (&kernelContext_6)->shadow_sampler_0 = shadow_sampler_1;

#line 1393
    (&kernelContext_6)->ambient_occlusion_0 = ambient_occlusion_1;



    float3 normal_1 = normalize(_S25.world_normal_0);
    float3 to_eye_0 = normalize(frame_1->camera_position_0.xyz - _S25.world_position_5);



    float3 _S26 = geometric_normal_of_0(_S25.world_position_5, normal_1);



    GpuMaterial_natural_0 material_2 = materials_1[_S25.material_1];

#line 1416
    float3 _S27 = float3(_S25.uv_1, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S25.color_2 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S27)).xy, uint(((_S27)).z)));

#line 1423
    float metallic_1 = saturate(material_2.metallic_0);
    float roughness_1 = clamp(material_2.roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_1 * roughness_1;
    float _S28 = alpha_0 * alpha_0;

#line 1432
    float3 _S29 = albedo_0.xyz;

#line 1432
    float3 f0_1 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S29, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S29 * float3((1.0f - metallic_1)) ;

#line 1439
    float _S30 = max(dot(normal_1, to_eye_0), 0.00009999999747379f);

#line 1449
    float2 _S31 = position_2.xy;

#line 1449
    uint _S32 = froxel_of_0(_S31, (((float4(_S25.world_position_5, 1.0f)) * (matrix<float,int(4),int(4)> (frame_1->view_proj_0.data_1[int(0)][int(0)], frame_1->view_proj_0.data_1[int(1)][int(0)], frame_1->view_proj_0.data_1[int(2)][int(0)], frame_1->view_proj_0.data_1[int(3)][int(0)], frame_1->view_proj_0.data_1[int(0)][int(1)], frame_1->view_proj_0.data_1[int(1)][int(1)], frame_1->view_proj_0.data_1[int(2)][int(1)], frame_1->view_proj_0.data_1[int(3)][int(1)], frame_1->view_proj_0.data_1[int(0)][int(2)], frame_1->view_proj_0.data_1[int(1)][int(2)], frame_1->view_proj_0.data_1[int(2)][int(2)], frame_1->view_proj_0.data_1[int(3)][int(2)], frame_1->view_proj_0.data_1[int(0)][int(3)], frame_1->view_proj_0.data_1[int(1)][int(3)], frame_1->view_proj_0.data_1[int(2)][int(3)], frame_1->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_6);

#line 1449
    uint base_2 = _S32 * 17U;

#line 1454
    uint _S33 = min((&kernelContext_6)->cluster_lights_0[base_2], 16U);

#line 1460
    float3 _S34 = float3(0.0f, 0.0f, 0.0f);

#line 1460
    uint slot_0 = 0U;

#line 1460
    float3 direct_0 = _S34;

#line 1460
    float3 gloss_0 = _S34;

    for(;;)
    {

#line 1462
        if(slot_0 < _S33)
        {
        }
        else
        {

#line 1462
            break;
        }

#line 1462
        thread GpuLight_natural_0 _S35 = (&kernelContext_6)->lights_0[(&kernelContext_6)->cluster_lights_0[base_2 + 1U + slot_0]];

#line 1462
        uint _S36 = (&_S35)->kind_0;

#line 1471
        bool _S37 = ((&_S35)->kind_0) == 0U;

#line 1471
        float3 to_light_6;

#line 1471
        float reach_0;

#line 1471
        if(_S37)
        {

#line 1471
            to_light_6 = normalize((float4((&_S35)->direction_0) ).xyz);

#line 1471
            reach_0 = 1.0f;

#line 1471
        }
        else
        {

#line 1471
            float4 _S38 = float4((&_S35)->position_1) ;

#line 1478
            float3 offset_0 = _S38.xyz - _S25.world_position_5;
            float distance_1 = length(offset_0);
            float3 to_light_7 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S38.w);
            if(_S36 == 2U)
            {

#line 1482
                float4 _S39 = float4((&_S35)->direction_0) ;

#line 1482
                reach_0 = reach_1 * spot_cone_0(to_light_7, _S39.xyz, _S39.w, (&_S35)->cos_inner_0);

#line 1482
            }
            else
            {

#line 1482
                reach_0 = reach_1;

#line 1482
            }

#line 1482
            to_light_6 = to_light_7;

#line 1471
        }

#line 1489
        float n_dot_l_5 = dot(normal_1, to_light_6);
        float _S40 = max(n_dot_l_5, 0.0f);

#line 1496
        float3 half_vector_0 = normalize(to_light_6 + to_eye_0);

#line 1503
        float3 specular_0 = ggx_lobe_0(_S28, f0_1, _S40, _S30, max(dot(normal_1, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S40) ;

#line 1503
        float reach_2;

#line 1518
        if(_S37)
        {

#line 1518
            float _S41 = sun_visibility_0(_S25.world_position_5, to_light_6, n_dot_l_5, _S26, &kernelContext_6);

#line 1518
            reach_2 = _S41;

#line 1518
        }
        else
        {

            if(_S36 == 1U)
            {

#line 1522
                uint _S42 = (&_S35)->shadow_tile_0;

#line 1534
                if(((&_S35)->shadow_tile_0) <= 0U)
                {

#line 1534
                    float _S43 = point_visibility_0(&_S35, _S42, _S25.world_position_5, to_light_6, n_dot_l_5, _S26, &kernelContext_6);

#line 1534
                    reach_2 = reach_0 * _S43;

#line 1534
                }
                else
                {

#line 1534
                    reach_2 = reach_0;

#line 1534
                }

#line 1522
            }
            else
            {

#line 1522
                uint _S44 = (&_S35)->shadow_tile_0;

#line 1540
                if(((&_S35)->shadow_tile_0) < 6U)
                {

#line 1540
                    float _S45 = spot_visibility_0(&_S35, _S44, _S25.world_position_5, to_light_6, n_dot_l_5, _S26, &kernelContext_6);

#line 1540
                    reach_2 = reach_0 * _S45;

#line 1540
                }
                else
                {

#line 1540
                    reach_2 = reach_0;

#line 1540
                }

#line 1522
            }

#line 1518
        }

#line 1548
        float3 _S46 = (float4((&_S35)->color_1) ).xyz;

#line 1548
        float3 direct_1 = direct_0 + _S46 * float3((_S40 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S46 * (specular_0 * float3(reach_2) );

#line 1462
        slot_0 = slot_0 + 1U;

#line 1462
        direct_0 = direct_1;

#line 1462
        gloss_0 = gloss_1;

#line 1462
    }

#line 1462
    texture2d<float, access::sample> _S47 = (&kernelContext_6)->ambient_occlusion_0;

#line 1568
    thread uint occlusion_width_0;
    thread uint occlusion_height_0;
    (*((&occlusion_width_0)) = (_S47).get_width(0)),(*((&occlusion_height_0)) = (_S47).get_height(0));


    int3 _S48 = int3(min(int2(_S31), int2(int(occlusion_width_0), int(occlusion_height_0)) - int2(int(1)) ), int(0));

#line 1590
    thread FragmentOutput_0 output_0;



    (&output_0)->lit_0 = float4(diffuse_albedo_0 * ((&kernelContext_6)->frame_0->ambient_0.xyz * float3((((&kernelContext_6)->ambient_occlusion_0).read(vec<uint,2>(((_S48)).xy), uint(((_S48)).z)).x))  + direct_0) + gloss_0, albedo_0.w);

#line 1599
    (&output_0)->reflectivity_0 = float4(f0_1, roughness_1);
    return output_0;
}


#line 1600
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_6 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 729
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_7;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 729
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]])
{

#line 729
    thread KernelContext_0 kernelContext_7;

#line 729
    (&kernelContext_7)->draw_0 = draw_2;

#line 729
    (&kernelContext_7)->visible_instances_0 = visible_instances_2;

#line 729
    (&kernelContext_7)->instances_0 = instances_2;

#line 729
    (&kernelContext_7)->meshes_0 = meshes_2;

#line 729
    (&kernelContext_7)->vertices_0 = vertices_2;

#line 729
    (&kernelContext_7)->frame_0 = frame_2;

#line 729
    (&kernelContext_7)->materials_0 = materials_2;

#line 729
    (&kernelContext_7)->base_color_textures_0 = base_color_textures_2;

#line 729
    (&kernelContext_7)->base_color_sampler_0 = base_color_sampler_2;

#line 729
    (&kernelContext_7)->cluster_lights_0 = cluster_lights_2;

#line 729
    (&kernelContext_7)->lights_0 = lights_2;

#line 729
    (&kernelContext_7)->shadow_atlas_0 = shadow_atlas_2;

#line 729
    (&kernelContext_7)->shadow_sampler_0 = shadow_sampler_2;

#line 729
    (&kernelContext_7)->ambient_occlusion_0 = ambient_occlusion_2;

#line 769
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 776
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[draw_2->mesh_0].base_vertex_0];

#line 776
    matrix<float,int(4),int(4)>  _S49 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S49)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_7 = world_0.xyz;

#line 787
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S49[int(0)].xyz, _S49[int(1)].xyz, _S49[int(2)].xyz))));
    (&output_1)->color_4 = float4(vertex_0.color_0) ;

#line 793
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_3 = (float4(vertex_0.uv_0) ).xy;

#line 794
    thread vertexMain_Result_0 _S50;

#line 794
    (&_S50)->position_3 = output_1.position_4;

#line 794
    (&_S50)->world_position_6 = output_1.world_position_7;

#line 794
    (&_S50)->world_normal_1 = output_1.world_normal_2;

#line 794
    (&_S50)->color_3 = output_1.color_4;

#line 794
    (&_S50)->material_3 = output_1.material_4;

#line 794
    (&_S50)->uv_2 = output_1.uv_3;

#line 794
    return _S50;
}

