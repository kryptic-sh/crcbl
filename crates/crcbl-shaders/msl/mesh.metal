#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 491 "shaders/mesh.slang"
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 692
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 692
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 364
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


#line 698
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 698
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 698
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 3332 "core.meta.slang"
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
    uint4 cluster_grid_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 light_view_proj_0;
};


#line 3332
struct GpuMaterial_natural_0
{
    packed_float4 base_color_0;
    uint base_color_texture_0;
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
    uint shadow_slot_0;
    uint pad1_2;
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
};


#line 996 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S1 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S2 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S3 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S4 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1006
    uint _S5 = uint(pixel_0.x) / _S4;

#line 1006
    uint _S6 = min(_S5, _S1 - 1U);
    uint _S7 = uint(pixel_0.y) / _S4;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1017
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S3 - 1U))) * _S2 + min(_S7, _S2 - 1U)) * _S1 + _S6;
}


#line 961
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 975
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 982
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 210
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 2U), float(tile_0 / 2U)) + tile_uv_0) / float2(2.0f, 2.0f);
}


#line 750
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 757
    float2 grid_0 = float2(2.0f, 2.0f);
    float2 _S8 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 758
    int y_0 = int(-1);

#line 758
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 760
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 760
            break;
        }

#line 760
        int x_0 = int(-1);

        for(;;)
        {

#line 762
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 762
                break;
            }



            float _S9 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S8, float2(1.0f)  - _S8))), (reference_0), level((0.0f))));

#line 766
            float visibility_1 = visibility_0 + _S9;

#line 762
            x_0 = x_0 + int(1);

#line 762
            visibility_0 = visibility_1;

#line 762
        }

#line 760
        y_0 = y_0 + int(1);

#line 760
    }

#line 770
    return visibility_0 / 9.0f;
}


#line 781
float sun_visibility_0(float3 world_position_0, float n_dot_l_0, KernelContext_0 thread* kernelContext_2)
{

#line 781
    uint cascade_0;

    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 793
    float _S10 = length(world_position_0 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 793
    uint index_0 = 0U;

    for(;;)
    {

#line 795
        if(index_0 < 2U)
        {
        }
        else
        {

#line 795
            cascade_0 = 1U;

#line 795
            break;
        }
        if(_S10 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 797
            cascade_0 = index_0;


            break;
        }

#line 795
        index_0 = index_0 + 1U;

#line 795
    }

#line 804
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 808
    bool _S11;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 809
        _S11 = true;

#line 809
    }
    else
    {

#line 809
        _S11 = (ndc_0.z) <= 0.0f;

#line 809
    }

#line 809
    if(_S11)
    {



        return 1.0f;
    }

#line 823
    float cosine_0 = saturate(n_dot_l_0);

#line 823
    float _S12 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z + (kernelContext_2->frame_0->shadow_params_0.z + kernelContext_2->frame_0->shadow_params_0.w * min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f)), kernelContext_2);

#line 832
    return _S12;
}


#line 198
uint light_tile_0(uint slot_0)
{
    return 2U + slot_0;
}


#line 893
float spot_visibility_0(const GpuLight_natural_0 thread* light_0, uint slot_1, float3 world_position_1, float3 to_light_1, float n_dot_l_1, KernelContext_0 thread* kernelContext_3)
{

    if(n_dot_l_1 <= 0.0f)
    {


        return 1.0f;
    }

#line 900
    float4 _S13 = float4(light_0->direction_0) ;

#line 907
    float cos_outer_1 = _S13.w;

#line 916
    float cosine_1 = saturate(n_dot_l_1);

#line 922
    float4 clip_1 = (((float4(world_position_1 + to_light_1 * float3((2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_1 - (float4(light_0->position_1) ).xyz, normalize(_S13.xyz)), 0.0f) / 1024.0f * (2.0f + 4.0f * min(sqrt(saturate(1.0f - cosine_1 * cosine_1)) / max(cosine_1, 0.00009999999747379f), 5.0f)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_2[slot_1].data_1[int(3)][int(3)]))));

#line 929
    float _S14 = clip_1.w;

#line 929
    if(_S14 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S14) ;

#line 933
    bool _S15;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 934
        _S15 = true;

#line 934
    }
    else
    {

#line 934
        _S15 = (ndc_1.z) <= 0.0f;

#line 934
    }

#line 934
    if(_S15)
    {

#line 934
        _S15 = true;

#line 934
    }
    else
    {

#line 934
        _S15 = (ndc_1.z) > 1.0f;

#line 934
    }

#line 934
    if(_S15)
    {

#line 940
        return 1.0f;
    }

#line 940
    float _S16 = tile_pcf_0(light_tile_0(slot_1), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 946
    return _S16;
}


#line 946
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 946
struct pixelInput_0
{
    float3 world_position_2 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 1021
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S17 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]])
{

#line 1021
    thread KernelContext_0 kernelContext_4;

#line 1021
    (&kernelContext_4)->draw_0 = draw_1;

#line 1021
    (&kernelContext_4)->visible_instances_0 = visible_instances_1;

#line 1021
    (&kernelContext_4)->instances_0 = instances_1;

#line 1021
    (&kernelContext_4)->meshes_0 = meshes_1;

#line 1021
    (&kernelContext_4)->vertices_0 = vertices_1;

#line 1021
    (&kernelContext_4)->frame_0 = frame_1;

#line 1021
    (&kernelContext_4)->materials_0 = materials_1;

#line 1021
    (&kernelContext_4)->base_color_textures_0 = base_color_textures_1;

#line 1021
    (&kernelContext_4)->base_color_sampler_0 = base_color_sampler_1;

#line 1021
    (&kernelContext_4)->cluster_lights_0 = cluster_lights_1;

#line 1021
    (&kernelContext_4)->lights_0 = lights_1;

#line 1021
    (&kernelContext_4)->shadow_atlas_0 = shadow_atlas_1;

#line 1021
    (&kernelContext_4)->shadow_sampler_0 = shadow_sampler_1;



    float3 _S18 = normalize(_S17.world_normal_0);
    float3 _S19 = normalize(frame_1->camera_position_0.xyz - _S17.world_position_2);



    GpuMaterial_natural_0 material_2 = materials_1[_S17.material_1];

#line 1040
    float3 _S20 = float3(_S17.uv_1, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S17.color_2 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S20)).xy, uint(((_S20)).z)));

#line 1041
    uint _S21 = froxel_of_0(position_2.xy, (((float4(_S17.world_position_2, 1.0f)) * (matrix<float,int(4),int(4)> (frame_1->view_proj_0.data_1[int(0)][int(0)], frame_1->view_proj_0.data_1[int(1)][int(0)], frame_1->view_proj_0.data_1[int(2)][int(0)], frame_1->view_proj_0.data_1[int(3)][int(0)], frame_1->view_proj_0.data_1[int(0)][int(1)], frame_1->view_proj_0.data_1[int(1)][int(1)], frame_1->view_proj_0.data_1[int(2)][int(1)], frame_1->view_proj_0.data_1[int(3)][int(1)], frame_1->view_proj_0.data_1[int(0)][int(2)], frame_1->view_proj_0.data_1[int(1)][int(2)], frame_1->view_proj_0.data_1[int(2)][int(2)], frame_1->view_proj_0.data_1[int(3)][int(2)], frame_1->view_proj_0.data_1[int(0)][int(3)], frame_1->view_proj_0.data_1[int(1)][int(3)], frame_1->view_proj_0.data_1[int(2)][int(3)], frame_1->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_4);

#line 1051
    uint base_1 = _S21 * 17U;

#line 1056
    uint _S22 = min((&kernelContext_4)->cluster_lights_0[base_1], 16U);

#line 1062
    float3 _S23 = float3(0.0f, 0.0f, 0.0f);

#line 1062
    uint slot_2 = 0U;

#line 1062
    float3 direct_0 = _S23;

#line 1062
    float3 gloss_0 = _S23;

    for(;;)
    {

#line 1064
        if(slot_2 < _S22)
        {
        }
        else
        {

#line 1064
            break;
        }

#line 1064
        thread GpuLight_natural_0 _S24 = (&kernelContext_4)->lights_0[(&kernelContext_4)->cluster_lights_0[base_1 + 1U + slot_2]];

#line 1064
        uint _S25 = (&_S24)->kind_0;

#line 1073
        bool _S26 = ((&_S24)->kind_0) == 0U;

#line 1073
        float3 to_light_2;

#line 1073
        float reach_0;

#line 1073
        if(_S26)
        {

#line 1073
            to_light_2 = normalize((float4((&_S24)->direction_0) ).xyz);

#line 1073
            reach_0 = 1.0f;

#line 1073
        }
        else
        {

#line 1073
            float4 _S27 = float4((&_S24)->position_1) ;

#line 1080
            float3 offset_0 = _S27.xyz - _S17.world_position_2;
            float distance_1 = length(offset_0);
            float3 to_light_3 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S27.w);
            if(_S25 == 2U)
            {

#line 1084
                float4 _S28 = float4((&_S24)->direction_0) ;

#line 1084
                reach_0 = reach_1 * spot_cone_0(to_light_3, _S28.xyz, _S28.w, (&_S24)->cos_inner_0);

#line 1084
            }
            else
            {

#line 1084
                reach_0 = reach_1;

#line 1084
            }

#line 1084
            to_light_2 = to_light_3;

#line 1073
        }

#line 1091
        float n_dot_l_2 = dot(_S18, to_light_2);
        float _S29 = max(n_dot_l_2, 0.0f);

#line 1100
        float specular_0 = pow(max(dot(_S18, normalize(to_light_2 + _S19)), 0.0f), 32.0f) * (step(0.0f, _S29) * _S29);

#line 1100
        float reach_2;

#line 1115
        if(_S26)
        {

#line 1115
            float _S30 = sun_visibility_0(_S17.world_position_2, n_dot_l_2, &kernelContext_4);

#line 1115
            reach_2 = _S30;

#line 1115
        }
        else
        {

#line 1115
            uint _S31 = (&_S24)->shadow_slot_0;



            if(((&_S24)->shadow_slot_0) < 2U)
            {

#line 1119
                float _S32 = spot_visibility_0(&_S24, _S31, _S17.world_position_2, to_light_2, n_dot_l_2, &kernelContext_4);

#line 1119
                reach_2 = reach_0 * _S32;

#line 1119
            }
            else
            {

#line 1119
                reach_2 = reach_0;

#line 1119
            }

#line 1115
        }

#line 1127
        float3 _S33 = (float4((&_S24)->color_1) ).xyz;

#line 1127
        float3 direct_1 = direct_0 + _S33 * float3((_S29 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S33 * float3((specular_0 * reach_2 * 0.34999999403953552f)) ;

#line 1064
        slot_2 = slot_2 + 1U;

#line 1064
        direct_0 = direct_1;

#line 1064
        gloss_0 = gloss_1;

#line 1064
    }

#line 1064
    pixelOutput_0 _S34 = { float4(albedo_0.xyz * ((&kernelContext_4)->frame_0->ambient_0.xyz + direct_0) + gloss_0, albedo_0.w) };

#line 1141
    return _S34;
}


#line 1141
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_3 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 652
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_4;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 652
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]])
{

#line 652
    thread KernelContext_0 kernelContext_5;

#line 652
    (&kernelContext_5)->draw_0 = draw_2;

#line 652
    (&kernelContext_5)->visible_instances_0 = visible_instances_2;

#line 652
    (&kernelContext_5)->instances_0 = instances_2;

#line 652
    (&kernelContext_5)->meshes_0 = meshes_2;

#line 652
    (&kernelContext_5)->vertices_0 = vertices_2;

#line 652
    (&kernelContext_5)->frame_0 = frame_2;

#line 652
    (&kernelContext_5)->materials_0 = materials_2;

#line 652
    (&kernelContext_5)->base_color_textures_0 = base_color_textures_2;

#line 652
    (&kernelContext_5)->base_color_sampler_0 = base_color_sampler_2;

#line 652
    (&kernelContext_5)->cluster_lights_0 = cluster_lights_2;

#line 652
    (&kernelContext_5)->lights_0 = lights_2;

#line 652
    (&kernelContext_5)->shadow_atlas_0 = shadow_atlas_2;

#line 652
    (&kernelContext_5)->shadow_sampler_0 = shadow_sampler_2;

#line 692
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 699
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[draw_2->mesh_0].base_vertex_0];

#line 699
    matrix<float,int(4),int(4)>  _S35 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S35)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_4 = world_0.xyz;

#line 710
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S35[int(0)].xyz, _S35[int(1)].xyz, _S35[int(2)].xyz))));
    (&output_1)->color_4 = float4(vertex_0.color_0) ;

#line 716
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_3 = (float4(vertex_0.uv_0) ).xy;

#line 717
    thread vertexMain_Result_0 _S36;

#line 717
    (&_S36)->position_3 = output_1.position_4;

#line 717
    (&_S36)->world_position_3 = output_1.world_position_4;

#line 717
    (&_S36)->world_normal_1 = output_1.world_normal_2;

#line 717
    (&_S36)->color_3 = output_1.color_4;

#line 717
    (&_S36)->material_3 = output_1.material_4;

#line 717
    (&_S36)->uv_2 = output_1.uv_3;

#line 717
    return _S36;
}

