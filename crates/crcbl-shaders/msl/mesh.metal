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


#line 712
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 712
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


#line 718
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 718
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 718
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
};


#line 1088 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S1 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S2 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S3 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S4 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 1098
    uint _S5 = uint(pixel_0.x) / _S4;

#line 1098
    uint _S6 = min(_S5, _S1 - 1U);
    uint _S7 = uint(pixel_0.y) / _S4;

    float scale_0 = 24.0f / log2(10000.0f);

#line 1109
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S3 - 1U))) * _S2 + min(_S7, _S2 - 1U)) * _S1 + _S6;
}


#line 1053
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 1067
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 1074
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 223
float2 atlas_uv_0(uint tile_0, float2 tile_uv_0)
{

    return (float2(float(tile_0 % 4U), float(tile_0 / 4U)) + tile_uv_0) / float2(4.0f, 2.0f);
}


#line 770
float tile_pcf_0(uint tile_1, float2 tile_uv_1, float reference_0, KernelContext_0 thread* kernelContext_1)
{
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;

#line 777
    float2 grid_0 = float2(4.0f, 2.0f);
    float2 _S8 = float2(0.5f, 0.5f) * texel_0 * grid_0;

#line 778
    int y_0 = int(-1);

#line 778
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 780
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 780
            break;
        }

#line 780
        int x_0 = int(-1);

        for(;;)
        {

#line 782
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 782
                break;
            }



            float _S9 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + float2(float(x_0), float(y_0)) * texel_0 * grid_0, _S8, float2(1.0f)  - _S8))), (reference_0), level((0.0f))));

#line 786
            float visibility_1 = visibility_0 + _S9;

#line 782
            x_0 = x_0 + int(1);

#line 782
            visibility_0 = visibility_1;

#line 782
        }

#line 780
        y_0 = y_0 + int(1);

#line 780
    }

#line 790
    return visibility_0 / 9.0f;
}


#line 801
float sun_visibility_0(float3 world_position_0, float n_dot_l_0, KernelContext_0 thread* kernelContext_2)
{

#line 801
    uint cascade_0;

    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 813
    float _S10 = length(world_position_0 - kernelContext_2->frame_0->camera_position_0.xyz);

#line 813
    uint index_0 = 0U;

    for(;;)
    {

#line 815
        if(index_0 < 2U)
        {
        }
        else
        {

#line 815
            cascade_0 = 1U;

#line 815
            break;
        }
        if(_S10 < kernelContext_2->frame_0->cascade_far_0[index_0])
        {

#line 817
            cascade_0 = index_0;


            break;
        }

#line 815
        index_0 = index_0 + 1U;

#line 815
    }

#line 824
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_2->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 828
    bool _S11;
    if(any((abs(ndc_0.xy)) > (float2(1.0f) )))
    {

#line 829
        _S11 = true;

#line 829
    }
    else
    {

#line 829
        _S11 = (ndc_0.z) <= 0.0f;

#line 829
    }

#line 829
    if(_S11)
    {



        return 1.0f;
    }

#line 843
    float cosine_0 = saturate(n_dot_l_0);

#line 843
    float _S12 = tile_pcf_0(cascade_0, float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z + (kernelContext_2->frame_0->shadow_params_0.z + kernelContext_2->frame_0->shadow_params_0.w * min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f)), kernelContext_2);

#line 852
    return _S12;
}


#line 1004
uint point_face_0(float3 from_light_0)
{
    float3 axis_1 = abs(from_light_0);
    float _S13 = axis_1.x;

#line 1007
    float _S14 = axis_1.y;

#line 1007
    bool _S15;

#line 1007
    if(_S13 >= _S14)
    {

#line 1007
        _S15 = _S13 >= (axis_1.z);

#line 1007
    }
    else
    {

#line 1007
        _S15 = false;

#line 1007
    }

#line 1007
    uint _S16;

#line 1007
    if(_S15)
    {
        if((from_light_0.x) >= 0.0f)
        {

#line 1009
            _S16 = 0U;

#line 1009
        }
        else
        {

#line 1009
            _S16 = 1U;

#line 1009
        }

#line 1009
        return _S16;
    }
    if(_S14 >= (axis_1.z))
    {
        if((from_light_0.y) >= 0.0f)
        {

#line 1013
            _S16 = 2U;

#line 1013
        }
        else
        {

#line 1013
            _S16 = 3U;

#line 1013
        }

#line 1013
        return _S16;
    }
    if((from_light_0.z) >= 0.0f)
    {

#line 1015
        _S16 = 4U;

#line 1015
    }
    else
    {

#line 1015
        _S16 = 5U;

#line 1015
    }

#line 1015
    return _S16;
}


#line 211
uint light_tile_0(uint tile_2)
{
    return 2U + tile_2;
}


#line 924
float punctual_visibility_0(uint tile_3, float3 world_position_1, float3 to_light_1, float n_dot_l_1, float texel_world_0, KernelContext_0 thread* kernelContext_3)
{

    float cosine_1 = saturate(n_dot_l_1);

#line 933
    float4 clip_1 = (((float4(world_position_1 + to_light_1 * float3((texel_world_0 * (2.0f + 4.0f * min(sqrt(saturate(1.0f - cosine_1 * cosine_1)) / max(cosine_1, 0.00009999999747379f), 5.0f)))) , 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(0)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(1)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(2)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(0)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(1)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(2)][int(3)], (&kernelContext_3->frame_0->light_view_proj_0)->data_3[tile_3].data_1[int(3)][int(3)]))));

#line 940
    float _S17 = clip_1.w;

#line 940
    if(_S17 <= 0.0f)
    {
        return 1.0f;
    }
    float3 ndc_1 = clip_1.xyz / float3(_S17) ;

#line 944
    bool _S18;
    if(any((abs(ndc_1.xy)) > (float2(1.0f) )))
    {

#line 945
        _S18 = true;

#line 945
    }
    else
    {

#line 945
        _S18 = (ndc_1.z) <= 0.0f;

#line 945
    }

#line 945
    if(_S18)
    {

#line 945
        _S18 = true;

#line 945
    }
    else
    {

#line 945
        _S18 = (ndc_1.z) > 1.0f;

#line 945
    }

#line 945
    if(_S18)
    {

#line 952
        return 1.0f;
    }

#line 952
    float _S19 = tile_pcf_0(light_tile_0(tile_3), float2(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, kernelContext_3);

#line 958
    return _S19;
}


#line 1023
float point_visibility_0(const GpuLight_natural_0 thread* light_0, uint base_1, float3 world_position_2, float3 to_light_2, float n_dot_l_2, KernelContext_0 thread* kernelContext_4)
{

    if(n_dot_l_2 <= 0.0f)
    {
        return 1.0f;
    }

    float3 from_light_1 = world_position_2 - (float4(light_0->position_1) ).xyz;

#line 1031
    float _S20 = punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_2, to_light_2, n_dot_l_2, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 1024.0f, kernelContext_4);

#line 1037
    return _S20;
}


#line 965
float spot_visibility_0(const GpuLight_natural_0 thread* light_1, uint tile_4, float3 world_position_3, float3 to_light_3, float n_dot_l_3, KernelContext_0 thread* kernelContext_5)
{

    if(n_dot_l_3 <= 0.0f)
    {


        return 1.0f;
    }

#line 972
    float4 _S21 = float4(light_1->direction_0) ;

#line 979
    float cos_outer_1 = _S21.w;

#line 979
    float _S22 = punctual_visibility_0(tile_4, world_position_3, to_light_3, n_dot_l_3, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_3 - (float4(light_1->position_1) ).xyz, normalize(_S21.xyz)), 0.0f) / 1024.0f, kernelContext_5);

#line 986
    return _S22;
}


#line 986
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 986
struct pixelInput_0
{
    float3 world_position_4 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 1113
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S23 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]])
{

#line 1113
    thread KernelContext_0 kernelContext_6;

#line 1113
    (&kernelContext_6)->draw_0 = draw_1;

#line 1113
    (&kernelContext_6)->visible_instances_0 = visible_instances_1;

#line 1113
    (&kernelContext_6)->instances_0 = instances_1;

#line 1113
    (&kernelContext_6)->meshes_0 = meshes_1;

#line 1113
    (&kernelContext_6)->vertices_0 = vertices_1;

#line 1113
    (&kernelContext_6)->frame_0 = frame_1;

#line 1113
    (&kernelContext_6)->materials_0 = materials_1;

#line 1113
    (&kernelContext_6)->base_color_textures_0 = base_color_textures_1;

#line 1113
    (&kernelContext_6)->base_color_sampler_0 = base_color_sampler_1;

#line 1113
    (&kernelContext_6)->cluster_lights_0 = cluster_lights_1;

#line 1113
    (&kernelContext_6)->lights_0 = lights_1;

#line 1113
    (&kernelContext_6)->shadow_atlas_0 = shadow_atlas_1;

#line 1113
    (&kernelContext_6)->shadow_sampler_0 = shadow_sampler_1;



    float3 _S24 = normalize(_S23.world_normal_0);
    float3 _S25 = normalize(frame_1->camera_position_0.xyz - _S23.world_position_4);



    GpuMaterial_natural_0 material_2 = materials_1[_S23.material_1];

#line 1132
    float3 _S26 = float3(_S23.uv_1, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S23.color_2 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S26)).xy, uint(((_S26)).z)));

#line 1133
    uint _S27 = froxel_of_0(position_2.xy, (((float4(_S23.world_position_4, 1.0f)) * (matrix<float,int(4),int(4)> (frame_1->view_proj_0.data_1[int(0)][int(0)], frame_1->view_proj_0.data_1[int(1)][int(0)], frame_1->view_proj_0.data_1[int(2)][int(0)], frame_1->view_proj_0.data_1[int(3)][int(0)], frame_1->view_proj_0.data_1[int(0)][int(1)], frame_1->view_proj_0.data_1[int(1)][int(1)], frame_1->view_proj_0.data_1[int(2)][int(1)], frame_1->view_proj_0.data_1[int(3)][int(1)], frame_1->view_proj_0.data_1[int(0)][int(2)], frame_1->view_proj_0.data_1[int(1)][int(2)], frame_1->view_proj_0.data_1[int(2)][int(2)], frame_1->view_proj_0.data_1[int(3)][int(2)], frame_1->view_proj_0.data_1[int(0)][int(3)], frame_1->view_proj_0.data_1[int(1)][int(3)], frame_1->view_proj_0.data_1[int(2)][int(3)], frame_1->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_6);

#line 1143
    uint base_2 = _S27 * 17U;

#line 1148
    uint _S28 = min((&kernelContext_6)->cluster_lights_0[base_2], 16U);

#line 1154
    float3 _S29 = float3(0.0f, 0.0f, 0.0f);

#line 1154
    uint slot_0 = 0U;

#line 1154
    float3 direct_0 = _S29;

#line 1154
    float3 gloss_0 = _S29;

    for(;;)
    {

#line 1156
        if(slot_0 < _S28)
        {
        }
        else
        {

#line 1156
            break;
        }

#line 1156
        thread GpuLight_natural_0 _S30 = (&kernelContext_6)->lights_0[(&kernelContext_6)->cluster_lights_0[base_2 + 1U + slot_0]];

#line 1156
        uint _S31 = (&_S30)->kind_0;

#line 1165
        bool _S32 = ((&_S30)->kind_0) == 0U;

#line 1165
        float3 to_light_4;

#line 1165
        float reach_0;

#line 1165
        if(_S32)
        {

#line 1165
            to_light_4 = normalize((float4((&_S30)->direction_0) ).xyz);

#line 1165
            reach_0 = 1.0f;

#line 1165
        }
        else
        {

#line 1165
            float4 _S33 = float4((&_S30)->position_1) ;

#line 1172
            float3 offset_0 = _S33.xyz - _S23.world_position_4;
            float distance_1 = length(offset_0);
            float3 to_light_5 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S33.w);
            if(_S31 == 2U)
            {

#line 1176
                float4 _S34 = float4((&_S30)->direction_0) ;

#line 1176
                reach_0 = reach_1 * spot_cone_0(to_light_5, _S34.xyz, _S34.w, (&_S30)->cos_inner_0);

#line 1176
            }
            else
            {

#line 1176
                reach_0 = reach_1;

#line 1176
            }

#line 1176
            to_light_4 = to_light_5;

#line 1165
        }

#line 1183
        float n_dot_l_4 = dot(_S24, to_light_4);
        float _S35 = max(n_dot_l_4, 0.0f);

#line 1192
        float specular_0 = pow(max(dot(_S24, normalize(to_light_4 + _S25)), 0.0f), 32.0f) * (step(0.0f, _S35) * _S35);

#line 1192
        float reach_2;

#line 1207
        if(_S32)
        {

#line 1207
            float _S36 = sun_visibility_0(_S23.world_position_4, n_dot_l_4, &kernelContext_6);

#line 1207
            reach_2 = _S36;

#line 1207
        }
        else
        {

            if(_S31 == 1U)
            {

#line 1211
                uint _S37 = (&_S30)->shadow_tile_0;

#line 1223
                if(((&_S30)->shadow_tile_0) <= 0U)
                {

#line 1223
                    float _S38 = point_visibility_0(&_S30, _S37, _S23.world_position_4, to_light_4, n_dot_l_4, &kernelContext_6);

#line 1223
                    reach_2 = reach_0 * _S38;

#line 1223
                }
                else
                {

#line 1223
                    reach_2 = reach_0;

#line 1223
                }

#line 1211
            }
            else
            {

#line 1211
                uint _S39 = (&_S30)->shadow_tile_0;

#line 1229
                if(((&_S30)->shadow_tile_0) < 6U)
                {

#line 1229
                    float _S40 = spot_visibility_0(&_S30, _S39, _S23.world_position_4, to_light_4, n_dot_l_4, &kernelContext_6);

#line 1229
                    reach_2 = reach_0 * _S40;

#line 1229
                }
                else
                {

#line 1229
                    reach_2 = reach_0;

#line 1229
                }

#line 1211
            }

#line 1207
        }

#line 1237
        float3 _S41 = (float4((&_S30)->color_1) ).xyz;

#line 1237
        float3 direct_1 = direct_0 + _S41 * float3((_S35 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S41 * float3((specular_0 * reach_2 * 0.34999999403953552f)) ;

#line 1156
        slot_0 = slot_0 + 1U;

#line 1156
        direct_0 = direct_1;

#line 1156
        gloss_0 = gloss_1;

#line 1156
    }

#line 1156
    pixelOutput_0 _S42 = { float4(albedo_0.xyz * ((&kernelContext_6)->frame_0->ambient_0.xyz + direct_0) + gloss_0, albedo_0.w) };

#line 1251
    return _S42;
}


#line 1251
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_5 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 672
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_6;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 672
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]])
{

#line 672
    thread KernelContext_0 kernelContext_7;

#line 672
    (&kernelContext_7)->draw_0 = draw_2;

#line 672
    (&kernelContext_7)->visible_instances_0 = visible_instances_2;

#line 672
    (&kernelContext_7)->instances_0 = instances_2;

#line 672
    (&kernelContext_7)->meshes_0 = meshes_2;

#line 672
    (&kernelContext_7)->vertices_0 = vertices_2;

#line 672
    (&kernelContext_7)->frame_0 = frame_2;

#line 672
    (&kernelContext_7)->materials_0 = materials_2;

#line 672
    (&kernelContext_7)->base_color_textures_0 = base_color_textures_2;

#line 672
    (&kernelContext_7)->base_color_sampler_0 = base_color_sampler_2;

#line 672
    (&kernelContext_7)->cluster_lights_0 = cluster_lights_2;

#line 672
    (&kernelContext_7)->lights_0 = lights_2;

#line 672
    (&kernelContext_7)->shadow_atlas_0 = shadow_atlas_2;

#line 672
    (&kernelContext_7)->shadow_sampler_0 = shadow_sampler_2;

#line 712
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 719
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[draw_2->mesh_0].base_vertex_0];

#line 719
    matrix<float,int(4),int(4)>  _S43 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S43)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_6 = world_0.xyz;

#line 730
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S43[int(0)].xyz, _S43[int(1)].xyz, _S43[int(2)].xyz))));
    (&output_1)->color_4 = float4(vertex_0.color_0) ;

#line 736
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_3 = (float4(vertex_0.uv_0) ).xy;

#line 737
    thread vertexMain_Result_0 _S44;

#line 737
    (&_S44)->position_3 = output_1.position_4;

#line 737
    (&_S44)->world_position_5 = output_1.world_position_6;

#line 737
    (&_S44)->world_normal_1 = output_1.world_normal_2;

#line 737
    (&_S44)->color_3 = output_1.color_4;

#line 737
    (&_S44)->material_3 = output_1.material_4;

#line 737
    (&_S44)->uv_2 = output_1.uv_3;

#line 737
    return _S44;
}

