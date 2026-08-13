#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 511 "shaders/mesh.slang"
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 739
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 739
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


#line 745
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 745
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 745
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


#line 1115 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S1 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S2 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S3 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S4 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1125
    uint _S5 = uint(pixel_0.x) / _S4;

#line 1125
    uint _S6 = min(_S5, _S1 - 1U);
    uint _S7 = uint(pixel_0.y) / _S4;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1136
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S3 - 1U))) * _S2 + min(_S7, _S2 - 1U)) * _S1 + _S6;
}


#line 1080
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 1094
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 1101
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 223
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 2.0f);
}


#line 797
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 804
    float2 grid_0 = float2(4.0f, 2.0f);
    float2 _S8 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 805
    int y_0 = int(-1);

#line 805
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 807
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 807
            break;
        }

#line 807
        int x_0 = int(-1);

        for(;;)
        {

#line 809
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 809
                break;
            }



            float _S9 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S8, float2(1.0f)  - _S8))), (reference_0), level((0.0f))));

#line 813
            float visibility_1 = visibility_0 + _S9;

#line 809
            x_0 = x_0 + int(1);

#line 809
            visibility_0 = visibility_1;

#line 809
        }

#line 807
        y_0 = y_0 + int(1);

#line 807
    }

#line 817
    return visibility_0 / 9.0f;
}


#line 828
float sun_visibility_0(float3 world_position_0, float n_dot_l_0, KernelContext_0 thread* kernelContext_2)
{

#line 828
    uint cascade_0;

    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 840
    float _S10 = length(world_position_0 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 840
    uint index_0 = 0U;

    for(;;)
    {

#line 842
        if(index_0 < 2U)
        {
        }
        else
        {

#line 842
            cascade_0 = 1U;

#line 842
            break;
        }
        if(_S10 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 844
            cascade_0 = index_0;


            break;
        }

#line 842
        index_0 = index_0 + 1U;

#line 842
    }

#line 851
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 855
    bool _S11;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 856
        _S11 = true;

#line 856
    }
    else
    {

#line 856
        _S11 = (ndc_0.z) <= 0.0f;

#line 856
    }

#line 856
    if(_S11)
    {



        return 1.0f;
    }

#line 870
    float cosine_0 = saturate(n_dot_l_0);

#line 870
    float _S12 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z + (kernelContext_2->frame_0->shadow_params_0.z + kernelContext_2->frame_0->shadow_params_0.w * min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f)), kernelContext_2);

#line 879
    return _S12;
}


#line 1031
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S13 = axis_1.x;

#line 1034
    float _S14 = axis_1.y;

#line 1034
    bool _S15;

#line 1034
    if(_S13 >= _S14)
    {

#line 1034
        _S15 = _S13 >= (axis_1.z);

#line 1034
    }
    else
    {

#line 1034
        _S15 = false;

#line 1034
    }

#line 1034
    uint _S16;

#line 1034
    if(_S15)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1036
            _S16 = 0U;

#line 1036
        }
        else
        {

#line 1036
            _S16 = 1U;

#line 1036
        }

#line 1036
        return _S16;
    }
    if(_S14 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1040
            _S16 = 2U;

#line 1040
        }
        else
        {

#line 1040
            _S16 = 3U;

#line 1040
        }

#line 1040
        return _S16;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1042
        _S16 = 4U;

#line 1042
    }
    else
    {

#line 1042
        _S16 = 5U;

#line 1042
    }

#line 1042
    return _S16;
}


#line 211
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 951
float punctual_visibility_0(uint tile_3, float3 world_position_1, float3 to_light_1, float n_dot_l_1, float texel_world_0, KernelContext_0 thread* kernelContext_3)
{

    float cosine_1 = saturate(n_dot_l_1);

#line 960
    float4 clip_1 = (((float4(world_position_1 + to_light_1 * float3((texel_world_0 * (2.0f + 4.0f * min(sqrt(saturate(1.0f - cosine_1 * cosine_1)) / max(cosine_1, 0.00009999999747379f), 5.0f)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 967
    float _S17 = clip_1.w;

#line 967
    if(_S17 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S17) ;

#line 971
    bool _S18;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 972
        _S18 = true;

#line 972
    }
    else
    {

#line 972
        _S18 = (ndc_1.z) <= 0.0f;

#line 972
    }

#line 972
    if(_S18)
    {

#line 972
        _S18 = true;

#line 972
    }
    else
    {

#line 972
        _S18 = (ndc_1.z) > 1.0f;

#line 972
    }

#line 972
    if(_S18)
    {

#line 979
        return 1.0f;
    }

#line 979
    float _S19 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 985
    return _S19;
}


#line 1050
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_2, float3 to_light_2, float n_dot_l_2, KernelContext_0 thread* kernelContext_4)
{

    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_2 - (float4(light_0->position_1) ).xyz;

#line 1058
    float _S20 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_2, to_light_2, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 1024.0f, kernelContext_4);

#line 1064
    return _S20;
}


#line 992
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_3, float3 to_light_3, float n_dot_l_3, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_3 <= 0.0f)
    {


        return 1.0f;
    }

#line 999
    float4 _S21 = float4(light_1->direction_0) ;

#line 1006
    float cos_outer_1 = _S21.w;

#line 1006
    float _S22 = punctual_visibility_0(tile_4, world_position_3, to_light_3, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_3 - (float4(light_1->position_1) ).xyz, normalize(_S21.xyz)), 0.0f) / 1024.0f, kernelContext_5);

#line 1013
    return _S22;
}


#line 1013
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 1013
struct pixelInput_0
{
    float3 world_position_4 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 1140
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S23 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_1 [[texture(2)]])
{

#line 1140
    thread KernelContext_0 kernelContext_6;

#line 1140
    (&kernelContext_6)->draw_0 = draw_1;

#line 1140
    (&kernelContext_6)->visible_instances_0 = visible_instances_1;

#line 1140
    (&kernelContext_6)->instances_0 = instances_1;

#line 1140
    (&kernelContext_6)->meshes_0 = meshes_1;

#line 1140
    (&kernelContext_6)->vertices_0 = vertices_1;

#line 1140
    (&kernelContext_6)->frame_0 = frame_1;

#line 1140
    (&kernelContext_6)->materials_0 = materials_1;

#line 1140
    (&kernelContext_6)->base_color_textures_0 = base_color_textures_1;

#line 1140
    (&kernelContext_6)->base_color_sampler_0 = base_color_sampler_1;

#line 1140
    (&kernelContext_6)->cluster_lights_0 = cluster_lights_1;

#line 1140
    (&kernelContext_6)->lights_0 = lights_1;

#line 1140
    (&kernelContext_6)->shadow_atlas_0 = shadow_atlas_1;

#line 1140
    (&kernelContext_6)->shadow_sampler_0 = shadow_sampler_1;

#line 1140
    (&kernelContext_6)->ambient_occlusion_0 = ambient_occlusion_1;



    float3 _S24 = normalize(_S23.world_normal_0);
    float3 _S25 = normalize(frame_1->camera_position_0.xyz - _S23.world_position_4);



    GpuMaterial_natural_0 material_2 = materials_1[_S23.material_1];

#line 1159
    float3 _S26 = float3(_S23.uv_1, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S23.color_2 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S26)).xy, uint(((_S26)).z)));

#line 1170
    float2 _S27 = position_2.xy;

#line 1170
    uint _S28 = froxel_of_0(_S27, (((float4(_S23.world_position_4, 1.0f)) * (matrix<float,int(4),int(4)> (frame_1->view_proj_0.data_1[int(0)][int(0)], frame_1->view_proj_0.data_1[int(1)][int(0)], frame_1->view_proj_0.data_1[int(2)][int(0)], frame_1->view_proj_0.data_1[int(3)][int(0)], frame_1->view_proj_0.data_1[int(0)][int(1)], frame_1->view_proj_0.data_1[int(1)][int(1)], frame_1->view_proj_0.data_1[int(2)][int(1)], frame_1->view_proj_0.data_1[int(3)][int(1)], frame_1->view_proj_0.data_1[int(0)][int(2)], frame_1->view_proj_0.data_1[int(1)][int(2)], frame_1->view_proj_0.data_1[int(2)][int(2)], frame_1->view_proj_0.data_1[int(3)][int(2)], frame_1->view_proj_0.data_1[int(0)][int(3)], frame_1->view_proj_0.data_1[int(1)][int(3)], frame_1->view_proj_0.data_1[int(2)][int(3)], frame_1->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_6);

#line 1170
    uint base_2 = _S28 * 17U;

#line 1175
    uint _S29 = min((&kernelContext_6)->cluster_lights_0[base_2], 16U);

#line 1181
    float3 _S30 = float3(0.0f, 0.0f, 0.0f);

#line 1181
    uint slot_0 = 0U;

#line 1181
    float3 direct_0 = _S30;

#line 1181
    float3 gloss_0 = _S30;

    for(;;)
    {

#line 1183
        if(slot_0 < _S29)
        {
        }
        else
        {

#line 1183
            break;
        }

#line 1183
        thread GpuLight_natural_0 _S31 = (&kernelContext_6)->lights_0[(&kernelContext_6)->cluster_lights_0[base_2 + 1U + slot_0]];

#line 1183
        uint _S32 = (&_S31)->kind_0;

#line 1192
        bool _S33 = ((&_S31)->kind_0) == 0U;

#line 1192
        float3 to_light_4;

#line 1192
        float reach_0;

#line 1192
        if(_S33)
        {

#line 1192
            to_light_4 = normalize((float4((&_S31)->direction_0) ).xyz);

#line 1192
            reach_0 = 1.0f;

#line 1192
        }
        else
        {

#line 1192
            float4 _S34 = float4((&_S31)->position_1) ;

#line 1199
            float3 offset_0 = _S34.xyz - _S23.world_position_4;
            float distance_1 = length(offset_0);
            float3 to_light_5 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S34.w);
            if(_S32 == 2U)
            {

#line 1203
                float4 _S35 = float4((&_S31)->direction_0) ;

#line 1203
                reach_0 = reach_1 * spot_cone_0(to_light_5, _S35.xyz, _S35.w, (&_S31)->cos_inner_0);

#line 1203
            }
            else
            {

#line 1203
                reach_0 = reach_1;

#line 1203
            }

#line 1203
            to_light_4 = to_light_5;

#line 1192
        }

#line 1210
        float n_dot_l_4 = dot(_S24, to_light_4);
        float _S36 = max(n_dot_l_4, 0.0f);

#line 1219
        float specular_0 = pow(max(dot(_S24, normalize(to_light_4 + _S25)), 0.0f), 32.0f) * (step(0.0f, _S36) * _S36);

#line 1219
        float reach_2;

#line 1234
        if(_S33)
        {

#line 1234
            float _S37 = sun_visibility_0(_S23.world_position_4, n_dot_l_4, &kernelContext_6);

#line 1234
            reach_2 = _S37;

#line 1234
        }
        else
        {

            if(_S32 == 1U)
            {

#line 1238
                uint _S38 = (&_S31)->shadow_tile_0;

#line 1250
                if(((&_S31)->shadow_tile_0) <= 0U)
                {

#line 1250
                    float _S39 = point_visibility_0(&_S31, _S38, _S23.world_position_4, to_light_4, n_dot_l_4, &kernelContext_6);

#line 1250
                    reach_2 = reach_0 * _S39;

#line 1250
                }
                else
                {

#line 1250
                    reach_2 = reach_0;

#line 1250
                }

#line 1238
            }
            else
            {

#line 1238
                uint _S40 = (&_S31)->shadow_tile_0;

#line 1256
                if(((&_S31)->shadow_tile_0) < 6U)
                {

#line 1256
                    float _S41 = spot_visibility_0(&_S31, _S40, _S23.world_position_4, to_light_4, n_dot_l_4, &kernelContext_6);

#line 1256
                    reach_2 = reach_0 * _S41;

#line 1256
                }
                else
                {

#line 1256
                    reach_2 = reach_0;

#line 1256
                }

#line 1238
            }

#line 1234
        }

#line 1264
        float3 _S42 = (float4((&_S31)->color_1) ).xyz;

#line 1264
        float3 direct_1 = direct_0 + _S42 * float3((_S36 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S42 * float3((specular_0 * reach_2 * 0.34999999403953552f)) ;

#line 1183
        slot_0 = slot_0 + 1U;

#line 1183
        direct_0 = direct_1;

#line 1183
        gloss_0 = gloss_1;

#line 1183
    }

#line 1278
    int3 _S43 = int3(int2(_S27), int(0));

#line 1278
    pixelOutput_0 _S44 = { float4(albedo_0.xyz * ((&kernelContext_6)->frame_0->ambient_0.xyz * float3((((&kernelContext_6)->ambient_occlusion_0).read(vec<uint,2>(((_S43)).xy), uint(((_S43)).z)).x))  + direct_0) + gloss_0, albedo_0.w) };

#line 1290
    return _S44;
}


#line 1290
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_5 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 699
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_6;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 699
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]], texture2d<float, access::sample> ambient_occlusion_2 [[texture(2)]])
{

#line 699
    thread KernelContext_0 kernelContext_7;

#line 699
    (&kernelContext_7)->draw_0 = draw_2;

#line 699
    (&kernelContext_7)->visible_instances_0 = visible_instances_2;

#line 699
    (&kernelContext_7)->instances_0 = instances_2;

#line 699
    (&kernelContext_7)->meshes_0 = meshes_2;

#line 699
    (&kernelContext_7)->vertices_0 = vertices_2;

#line 699
    (&kernelContext_7)->frame_0 = frame_2;

#line 699
    (&kernelContext_7)->materials_0 = materials_2;

#line 699
    (&kernelContext_7)->base_color_textures_0 = base_color_textures_2;

#line 699
    (&kernelContext_7)->base_color_sampler_0 = base_color_sampler_2;

#line 699
    (&kernelContext_7)->cluster_lights_0 = cluster_lights_2;

#line 699
    (&kernelContext_7)->lights_0 = lights_2;

#line 699
    (&kernelContext_7)->shadow_atlas_0 = shadow_atlas_2;

#line 699
    (&kernelContext_7)->shadow_sampler_0 = shadow_sampler_2;

#line 699
    (&kernelContext_7)->ambient_occlusion_0 = ambient_occlusion_2;

#line 739
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 746
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[draw_2->mesh_0].base_vertex_0];

#line 746
    matrix<float,int(4),int(4)>  _S45 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S45)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_6 = world_0.xyz;

#line 757
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S45[int(0)].xyz, _S45[int(1)].xyz, _S45[int(2)].xyz))));
    (&output_1)->color_4 = float4(vertex_0.color_0) ;

#line 763
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_3 = (float4(vertex_0.uv_0) ).xy;

#line 764
    thread vertexMain_Result_0 _S46;

#line 764
    (&_S46)->position_3 = output_1.position_4;

#line 764
    (&_S46)->world_position_5 = output_1.world_position_6;

#line 764
    (&_S46)->world_normal_1 = output_1.world_normal_2;

#line 764
    (&_S46)->color_3 = output_1.color_4;

#line 764
    (&_S46)->material_3 = output_1.material_4;

#line 764
    (&_S46)->uv_2 = output_1.uv_3;

#line 764
    return _S46;
}

