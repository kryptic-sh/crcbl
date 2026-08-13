#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 527 "shaders/mesh.slang"
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 755
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 755
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 380
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


#line 761
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 761
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 761
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


#line 1193 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S1 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S2 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S3 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S4 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1203
    uint _S5 = uint(pixel_0.x) / _S4;

#line 1203
    uint _S6 = min(_S5, _S1 - 1U);
    uint _S7 = uint(pixel_0.y) / _S4;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1214
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S3 - 1U))) * _S2 + min(_S7, _S2 - 1U)) * _S1 + _S6;
}


#line 1158
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 1172
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 1179
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 833
float3 ggx_lobe_0(float alpha2_0, float3 f0_0, float n_dot_l_0, float n_dot_v_0, float n_dot_h_0, float v_dot_h_0)
{

#line 840
    float shape_0 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;

#line 847
    float _S8 = 1.0f - alpha2_0;

#line 852
    float grazing_0 = 1.0f - v_dot_h_0;
    float grazing2_0 = grazing_0 * grazing_0;


    return float3((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S8 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S8 + alpha2_0), 9.99999997475242708e-07f))))  * (f0_0 + (float3(1.0f, 1.0f, 1.0f) - f0_0) * float3((grazing2_0 * grazing2_0 * grazing_0)) );
}


#line 223
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 2.0f);
}


#line 875
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 882
    float2 grid_0 = float2(4.0f, 2.0f);
    float2 _S9 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 883
    int y_0 = int(-1);

#line 883
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 885
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 885
            break;
        }

#line 885
        int x_0 = int(-1);

        for(;;)
        {

#line 887
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 887
                break;
            }



            float _S10 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S9, float2(1.0f)  - _S9))), (reference_0), level((0.0f))));

#line 891
            float visibility_1 = visibility_0 + _S10;

#line 887
            x_0 = x_0 + int(1);

#line 887
            visibility_0 = visibility_1;

#line 887
        }

#line 885
        y_0 = y_0 + int(1);

#line 885
    }

#line 895
    return visibility_0 / 9.0f;
}


#line 906
float sun_visibility_0(float3 world_position_0, float n_dot_l_1, KernelContext_0 thread* kernelContext_2)
{

#line 906
    uint cascade_0;

    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }

#line 918
    float _S11 = length(world_position_0 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 918
    uint index_0 = 0U;

    for(;;)
    {

#line 920
        if(index_0 < 2U)
        {
        }
        else
        {

#line 920
            cascade_0 = 1U;

#line 920
            break;
        }
        if(_S11 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 922
            cascade_0 = index_0;


            break;
        }

#line 920
        index_0 = index_0 + 1U;

#line 920
    }

#line 929
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 933
    bool _S12;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 934
        _S12 = true;

#line 934
    }
    else
    {

#line 934
        _S12 = (ndc_0.z) <= 0.0f;

#line 934
    }

#line 934
    if(_S12)
    {



        return 1.0f;
    }

#line 948
    float cosine_0 = saturate(n_dot_l_1);

#line 948
    float _S13 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z + (kernelContext_2->frame_0->shadow_params_0.z + kernelContext_2->frame_0->shadow_params_0.w * min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f)), kernelContext_2);

#line 957
    return _S13;
}


#line 1109
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S14 = axis_1.x;

#line 1112
    float _S15 = axis_1.y;

#line 1112
    bool _S16;

#line 1112
    if(_S14 >= _S15)
    {

#line 1112
        _S16 = _S14 >= (axis_1.z);

#line 1112
    }
    else
    {

#line 1112
        _S16 = false;

#line 1112
    }

#line 1112
    uint _S17;

#line 1112
    if(_S16)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1114
            _S17 = 0U;

#line 1114
        }
        else
        {

#line 1114
            _S17 = 1U;

#line 1114
        }

#line 1114
        return _S17;
    }
    if(_S15 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1118
            _S17 = 2U;

#line 1118
        }
        else
        {

#line 1118
            _S17 = 3U;

#line 1118
        }

#line 1118
        return _S17;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1120
        _S17 = 4U;

#line 1120
    }
    else
    {

#line 1120
        _S17 = 5U;

#line 1120
    }

#line 1120
    return _S17;
}


#line 211
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 1029
float punctual_visibility_0(uint tile_3, float3 world_position_1, float3 to_light_1, float n_dot_l_2, float texel_world_0, KernelContext_0 thread* kernelContext_3)
{

    float cosine_1 = saturate(n_dot_l_2);

#line 1038
    float4 clip_1 = (((float4(world_position_1 + to_light_1 * float3((texel_world_0 * (2.0f + 4.0f * min(sqrt(saturate(1.0f - cosine_1 * cosine_1)) / max(cosine_1, 0.00009999999747379f), 5.0f)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 1045
    float _S18 = clip_1.w;

#line 1045
    if(_S18 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S18) ;

#line 1049
    bool _S19;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 1050
        _S19 = true;

#line 1050
    }
    else
    {

#line 1050
        _S19 = (ndc_1.z) <= 0.0f;

#line 1050
    }

#line 1050
    if(_S19)
    {

#line 1050
        _S19 = true;

#line 1050
    }
    else
    {

#line 1050
        _S19 = (ndc_1.z) > 1.0f;

#line 1050
    }

#line 1050
    if(_S19)
    {

#line 1057
        return 1.0f;
    }

#line 1057
    float _S20 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 1063
    return _S20;
}


#line 1128
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_2, float3 to_light_2, float n_dot_l_3, KernelContext_0 thread* kernelContext_4)
{

    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_2 - (float4(light_0->position_1) ).xyz;

#line 1136
    float _S21 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_2, to_light_2, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 1024.0f, kernelContext_4);

#line 1142
    return _S21;
}


#line 1070
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_3, float3 to_light_3, float n_dot_l_4, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_4 <= 0.0f)
    {


        return 1.0f;
    }

#line 1077
    float4 _S22 = float4(light_1->direction_0) ;

#line 1084
    float cos_outer_1 = _S22.w;

#line 1084
    float _S23 = punctual_visibility_0(tile_4, world_position_3, to_light_3, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_3 - (float4(light_1->position_1) ).xyz, normalize(_S22.xyz)), 0.0f) / 1024.0f, kernelContext_5);

#line 1091
    return _S23;
}


#line 1091
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1091
struct pixelInput_0
{
    float3 world_position_4 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 1218
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S24 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]])
{

#line 1218
    thread KernelContext_0 kernelContext_6;

#line 1218
    (&kernelContext_6)->draw_0 = draw_1;

#line 1218
    (&kernelContext_6)->visible_instances_0 = visible_instances_1;

#line 1218
    (&kernelContext_6)->instances_0 = instances_1;

#line 1218
    (&kernelContext_6)->meshes_0 = meshes_1;

#line 1218
    (&kernelContext_6)->vertices_0 = vertices_1;

#line 1218
    (&kernelContext_6)->frame_0 = frame_1;

#line 1218
    (&kernelContext_6)->materials_0 = materials_1;

#line 1218
    (&kernelContext_6)->base_color_textures_0 = base_color_textures_1;

#line 1218
    (&kernelContext_6)->base_color_sampler_0 = base_color_sampler_1;

#line 1218
    (&kernelContext_6)->cluster_lights_0 = cluster_lights_1;

#line 1218
    (&kernelContext_6)->lights_0 = lights_1;

#line 1218
    (&kernelContext_6)->shadow_atlas_0 = shadow_atlas_1;

#line 1218
    (&kernelContext_6)->shadow_sampler_0 = shadow_sampler_1;

#line 1218
    (&kernelContext_6)->ambient_occlusion_0 = ambient_occlusion_1;



    float3 normal_1 = normalize(_S24.world_normal_0);
    float3 to_eye_0 = normalize(frame_1->camera_position_0.xyz - _S24.world_position_4);



    GpuMaterial_natural_0 material_2 = materials_1[_S24.material_1];

#line 1237
    float3 _S25 = float3(_S24.uv_1, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S24.color_2 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S25)).xy, uint(((_S25)).z)));

#line 1244
    float metallic_1 = saturate(material_2.metallic_0);
    float roughness_1 = clamp(material_2.roughness_0, 0.04500000178813934f, 1.0f);
    float alpha_0 = roughness_1 * roughness_1;
    float _S26 = alpha_0 * alpha_0;

#line 1253
    float3 _S27 = albedo_0.xyz;

#line 1253
    float3 _S28 = mix(float3(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S27, float3(metallic_1) );
    float3 diffuse_albedo_0 = _S27 * float3((1.0f - metallic_1)) ;

#line 1260
    float _S29 = max(dot(normal_1, to_eye_0), 0.00009999999747379f);

#line 1270
    float2 _S30 = position_2.xy;

#line 1270
    uint _S31 = froxel_of_0(_S30, (((float4(_S24.world_position_4, 1.0f)) * (matrix<float,int(4),int(4)> (frame_1->view_proj_0.data_1[int(0)][int(0)], frame_1->view_proj_0.data_1[int(1)][int(0)], frame_1->view_proj_0.data_1[int(2)][int(0)], frame_1->view_proj_0.data_1[int(3)][int(0)], frame_1->view_proj_0.data_1[int(0)][int(1)], frame_1->view_proj_0.data_1[int(1)][int(1)], frame_1->view_proj_0.data_1[int(2)][int(1)], frame_1->view_proj_0.data_1[int(3)][int(1)], frame_1->view_proj_0.data_1[int(0)][int(2)], frame_1->view_proj_0.data_1[int(1)][int(2)], frame_1->view_proj_0.data_1[int(2)][int(2)], frame_1->view_proj_0.data_1[int(3)][int(2)], frame_1->view_proj_0.data_1[int(0)][int(3)], frame_1->view_proj_0.data_1[int(1)][int(3)], frame_1->view_proj_0.data_1[int(2)][int(3)], frame_1->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_6);

#line 1270
    uint base_2 = _S31 * 17U;

#line 1275
    uint _S32 = min((&kernelContext_6)->cluster_lights_0[base_2], 16U);

#line 1281
    float3 _S33 = float3(0.0f, 0.0f, 0.0f);

#line 1281
    uint slot_0 = 0U;

#line 1281
    float3 direct_0 = _S33;

#line 1281
    float3 gloss_0 = _S33;

    for(;;)
    {

#line 1283
        if(slot_0 < _S32)
        {
        }
        else
        {

#line 1283
            break;
        }

#line 1283
        thread GpuLight_natural_0 _S34 = (&kernelContext_6)->lights_0[(&kernelContext_6)->cluster_lights_0[base_2 + 1U + slot_0]];

#line 1283
        uint _S35 = (&_S34)->kind_0;

#line 1292
        bool _S36 = ((&_S34)->kind_0) == 0U;

#line 1292
        float3 to_light_4;

#line 1292
        float reach_0;

#line 1292
        if(_S36)
        {

#line 1292
            to_light_4 = normalize((float4((&_S34)->direction_0) ).xyz);

#line 1292
            reach_0 = 1.0f;

#line 1292
        }
        else
        {

#line 1292
            float4 _S37 = float4((&_S34)->position_1) ;

#line 1299
            float3 offset_0 = _S37.xyz - _S24.world_position_4;
            float distance_1 = length(offset_0);
            float3 to_light_5 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S37.w);
            if(_S35 == 2U)
            {

#line 1303
                float4 _S38 = float4((&_S34)->direction_0) ;

#line 1303
                reach_0 = reach_1 * spot_cone_0(to_light_5, _S38.xyz, _S38.w, (&_S34)->cos_inner_0);

#line 1303
            }
            else
            {

#line 1303
                reach_0 = reach_1;

#line 1303
            }

#line 1303
            to_light_4 = to_light_5;

#line 1292
        }

#line 1310
        float n_dot_l_5 = dot(normal_1, to_light_4);
        float _S39 = max(n_dot_l_5, 0.0f);

#line 1317
        float3 half_vector_0 = normalize(to_light_4 + to_eye_0);

#line 1324
        float3 specular_0 = ggx_lobe_0(_S26, _S28, _S39, _S29, max(dot(normal_1, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * float3(_S39) ;

#line 1324
        float reach_2;

#line 1339
        if(_S36)
        {

#line 1339
            float _S40 = sun_visibility_0(_S24.world_position_4, n_dot_l_5, &kernelContext_6);

#line 1339
            reach_2 = _S40;

#line 1339
        }
        else
        {

            if(_S35 == 1U)
            {

#line 1343
                uint _S41 = (&_S34)->shadow_tile_0;

#line 1355
                if(((&_S34)->shadow_tile_0) <= 0U)
                {

#line 1355
                    float _S42 = point_visibility_0(&_S34, _S41, _S24.world_position_4, to_light_4, n_dot_l_5, &kernelContext_6);

#line 1355
                    reach_2 = reach_0 * _S42;

#line 1355
                }
                else
                {

#line 1355
                    reach_2 = reach_0;

#line 1355
                }

#line 1343
            }
            else
            {

#line 1343
                uint _S43 = (&_S34)->shadow_tile_0;

#line 1361
                if(((&_S34)->shadow_tile_0) < 6U)
                {

#line 1361
                    float _S44 = spot_visibility_0(&_S34, _S43, _S24.world_position_4, to_light_4, n_dot_l_5, &kernelContext_6);

#line 1361
                    reach_2 = reach_0 * _S44;

#line 1361
                }
                else
                {

#line 1361
                    reach_2 = reach_0;

#line 1361
                }

#line 1343
            }

#line 1339
        }

#line 1369
        float3 _S45 = (float4((&_S34)->color_1) ).xyz;

#line 1369
        float3 direct_1 = direct_0 + _S45 * float3((_S39 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S45 * (specular_0 * float3(reach_2) );

#line 1283
        slot_0 = slot_0 + 1U;

#line 1283
        direct_0 = direct_1;

#line 1283
        gloss_0 = gloss_1;

#line 1283
    }

#line 1383
    int3 _S46 = int3(int2(_S30), int(0));

#line 1383
    pixelOutput_0 _S47 = { float4(diffuse_albedo_0 * ((&kernelContext_6)->frame_0->ambient_0.xyz * float3((((&kernelContext_6)->ambient_occlusion_0).read(vec<uint,2>(((_S46)).xy), uint(((_S46)).z)).x))  + direct_0) + gloss_0, albedo_0.w) };

#line 1403
    return _S47;
}


#line 1403
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_5 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 715
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_6;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 715
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]])
{

#line 715
    thread KernelContext_0 kernelContext_7;

#line 715
    (&kernelContext_7)->draw_0 = draw_2;

#line 715
    (&kernelContext_7)->visible_instances_0 = visible_instances_2;

#line 715
    (&kernelContext_7)->instances_0 = instances_2;

#line 715
    (&kernelContext_7)->meshes_0 = meshes_2;

#line 715
    (&kernelContext_7)->vertices_0 = vertices_2;

#line 715
    (&kernelContext_7)->frame_0 = frame_2;

#line 715
    (&kernelContext_7)->materials_0 = materials_2;

#line 715
    (&kernelContext_7)->base_color_textures_0 = base_color_textures_2;

#line 715
    (&kernelContext_7)->base_color_sampler_0 = base_color_sampler_2;

#line 715
    (&kernelContext_7)->cluster_lights_0 = cluster_lights_2;

#line 715
    (&kernelContext_7)->lights_0 = lights_2;

#line 715
    (&kernelContext_7)->shadow_atlas_0 = shadow_atlas_2;

#line 715
    (&kernelContext_7)->shadow_sampler_0 = shadow_sampler_2;

#line 715
    (&kernelContext_7)->ambient_occlusion_0 = ambient_occlusion_2;

#line 755
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 762
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[draw_2->mesh_0].base_vertex_0];

#line 762
    matrix<float,int(4),int(4)>  _S48 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S48)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_6 = world_0.xyz;

#line 773
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S48[int(0)].xyz, _S48[int(1)].xyz, _S48[int(2)].xyz))));
    (&output_1)->color_4 = float4(vertex_0.color_0) ;

#line 779
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_3 = (float4(vertex_0.uv_0) ).xy;

#line 780
    thread vertexMain_Result_0 _S49;

#line 780
    (&_S49)->position_3 = output_1.position_4;

#line 780
    (&_S49)->world_position_5 = output_1.world_position_6;

#line 780
    (&_S49)->world_normal_1 = output_1.world_normal_2;

#line 780
    (&_S49)->color_3 = output_1.color_4;

#line 780
    (&_S49)->material_3 = output_1.material_4;

#line 780
    (&_S49)->uv_2 = output_1.uv_3;

#line 780
    return _S49;
}

