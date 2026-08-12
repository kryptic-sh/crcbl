#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 412 "shaders/mesh.slang"
struct DrawConstants_0
{
    uint base_0;
    uint mesh_0;
    uint pad0_0;
    uint pad1_0;
};


#line 613
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 613
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_1;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 299
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


#line 619
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 619
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 619
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
    uint pad0_2;
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


#line 795 "shaders/mesh.slang"
uint froxel_of_0(float2 pixel_0, float depth_0, KernelContext_0 thread* kernelContext_0)
{
    uint _S1 = max(kernelContext_0->frame_0->cluster_grid_0.x, 1U);
    uint _S2 = max(kernelContext_0->frame_0->cluster_grid_0.y, 1U);
    uint _S3 = max(kernelContext_0->frame_0->cluster_grid_0.z, 1U);
    uint _S4 = max(kernelContext_0->frame_0->cluster_grid_0.w, 1U);

#line 805
    uint _S5 = uint(pixel_0.x) / _S4;

#line 805
    uint _S6 = min(_S5, _S1 - 1U);
    uint _S7 = uint(pixel_0.y) / _S4;

    float scale_0 = 24.0f / log2(10000.0f);

#line 816
    return (uint(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, float(_S3 - 1U))) * _S2 + min(_S7, _S2 - 1U)) * _S1 + _S6;
}


#line 760
float punctual_falloff_0(float distance_0, float radius_0)
{
    float ratio_0 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    float window_0 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}


#line 774
float spot_cone_0(float3 to_light_0, float3 axis_0, float cos_outer_0, float cos_inner_1)
{

#line 781
    return saturate((dot(- to_light_0, normalize(axis_0)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}


#line 672
float sun_visibility_0(float3 world_position_0, float n_dot_l_0, KernelContext_0 thread* kernelContext_1)
{

#line 672
    uint cascade_0;

    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 684
    float _S8 = length(world_position_0 - kernelContext_1->frame_0->camera_position_0.xyz);

#line 684
    uint index_0 = 0U;

    for(;;)
    {

#line 686
        if(index_0 < 2U)
        {
        }
        else
        {

#line 686
            cascade_0 = 1U;

#line 686
            break;
        }
        if(_S8 < kernelContext_1->frame_0->cascade_far_0[index_0])
        {

#line 688
            cascade_0 = index_0;


            break;
        }

#line 686
        index_0 = index_0 + 1U;

#line 686
    }

#line 695
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_1->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 699
    float2 _S9 = float2(1.0f) ;

#line 699
    bool _S10;
    if(any((abs(ndc_0.xy)) > _S9))
    {

#line 700
        _S10 = true;

#line 700
    }
    else
    {

#line 700
        _S10 = (ndc_0.z) <= 0.0f;

#line 700
    }

#line 700
    if(_S10)
    {



        return 1.0f;
    }



    float2 _S11 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);
    float2 texel_0 = kernelContext_1->frame_0->shadow_params_0.xy;



    float cosine_0 = saturate(n_dot_l_0);

#line 722
    float _S12 = ndc_0.z + (kernelContext_1->frame_0->shadow_params_0.z + kernelContext_1->frame_0->shadow_params_0.w * min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f));

#line 728
    float2 _S13 = float2(2.0f, 1.0f);

#line 728
    float2 _S14 = float2(0.5f, 0.5f) * texel_0 * _S13;

#line 728
    int y_0 = int(-1);

#line 728
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 730
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 730
            break;
        }

#line 730
        int x_0 = int(-1);

        for(;;)
        {

#line 732
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 732
                break;
            }

#line 739
            float2 tap_0 = clamp(_S11 + float2(float(x_0), float(y_0)) * texel_0 * _S13, _S14, _S9 - _S14);


            float _S15 = ((kernelContext_1->shadow_atlas_0).sample_compare((kernelContext_1->shadow_sampler_0), (float2((float(cascade_0) + tap_0.x) / 2.0f, tap_0.y)), (_S12), level((0.0f))));

#line 742
            float visibility_1 = visibility_0 + _S15;

#line 732
            x_0 = x_0 + int(1);

#line 732
            visibility_0 = visibility_1;

#line 732
        }

#line 730
        y_0 = y_0 + int(1);

#line 730
    }

#line 745
    return visibility_0 / 9.0f;
}


#line 745
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 745
struct pixelInput_0
{
    float3 world_position_1 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 820
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S16 [[stage_in]], float4 position_2 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], uint device* cluster_lights_1 [[buffer(8)]], GpuLight_natural_0 device* lights_1 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]])
{

#line 820
    thread KernelContext_0 kernelContext_2;

#line 820
    (&kernelContext_2)->draw_0 = draw_1;

#line 820
    (&kernelContext_2)->visible_instances_0 = visible_instances_1;

#line 820
    (&kernelContext_2)->instances_0 = instances_1;

#line 820
    (&kernelContext_2)->meshes_0 = meshes_1;

#line 820
    (&kernelContext_2)->vertices_0 = vertices_1;

#line 820
    (&kernelContext_2)->frame_0 = frame_1;

#line 820
    (&kernelContext_2)->materials_0 = materials_1;

#line 820
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_1;

#line 820
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_1;

#line 820
    (&kernelContext_2)->cluster_lights_0 = cluster_lights_1;

#line 820
    (&kernelContext_2)->lights_0 = lights_1;

#line 820
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_1;

#line 820
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_1;



    float3 _S17 = normalize(_S16.world_normal_0);
    float3 _S18 = normalize(frame_1->camera_position_0.xyz - _S16.world_position_1);



    GpuMaterial_natural_0 material_2 = materials_1[_S16.material_1];

#line 839
    float3 _S19 = float3(_S16.uv_1, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S16.color_2 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S19)).xy, uint(((_S19)).z)));

#line 840
    uint _S20 = froxel_of_0(position_2.xy, (((float4(_S16.world_position_1, 1.0f)) * (matrix<float,int(4),int(4)> (frame_1->view_proj_0.data_1[int(0)][int(0)], frame_1->view_proj_0.data_1[int(1)][int(0)], frame_1->view_proj_0.data_1[int(2)][int(0)], frame_1->view_proj_0.data_1[int(3)][int(0)], frame_1->view_proj_0.data_1[int(0)][int(1)], frame_1->view_proj_0.data_1[int(1)][int(1)], frame_1->view_proj_0.data_1[int(2)][int(1)], frame_1->view_proj_0.data_1[int(3)][int(1)], frame_1->view_proj_0.data_1[int(0)][int(2)], frame_1->view_proj_0.data_1[int(1)][int(2)], frame_1->view_proj_0.data_1[int(2)][int(2)], frame_1->view_proj_0.data_1[int(3)][int(2)], frame_1->view_proj_0.data_1[int(0)][int(3)], frame_1->view_proj_0.data_1[int(1)][int(3)], frame_1->view_proj_0.data_1[int(2)][int(3)], frame_1->view_proj_0.data_1[int(3)][int(3)])))).w, &kernelContext_2);

#line 850
    uint base_1 = _S20 * 17U;

#line 855
    uint _S21 = min((&kernelContext_2)->cluster_lights_0[base_1], 16U);

#line 861
    float3 _S22 = float3(0.0f, 0.0f, 0.0f);

#line 861
    uint slot_0 = 0U;

#line 861
    float3 direct_0 = _S22;

#line 861
    float3 gloss_0 = _S22;

    for(;;)
    {

#line 863
        if(slot_0 < _S21)
        {
        }
        else
        {

#line 863
            break;
        }
        GpuLight_natural_0 light_0 = (&kernelContext_2)->lights_0[(&kernelContext_2)->cluster_lights_0[base_1 + 1U + slot_0]];

#line 872
        bool _S23 = (light_0.kind_0) == 0U;

#line 872
        float3 to_light_1;

#line 872
        float reach_0;

#line 872
        if(_S23)
        {

#line 872
            to_light_1 = normalize((float4(light_0.direction_0) ).xyz);

#line 872
            reach_0 = 1.0f;

#line 872
        }
        else
        {

#line 872
            float4 _S24 = float4(light_0.position_1) ;

#line 879
            float3 offset_0 = _S24.xyz - _S16.world_position_1;
            float distance_1 = length(offset_0);
            float3 to_light_2 = offset_0 / float3(max(distance_1, 9.99999997475242708e-07f)) ;
            float reach_1 = punctual_falloff_0(distance_1, _S24.w);
            if((light_0.kind_0) == 2U)
            {

#line 883
                float4 _S25 = float4(light_0.direction_0) ;

#line 883
                reach_0 = reach_1 * spot_cone_0(to_light_2, _S25.xyz, _S25.w, light_0.cos_inner_0);

#line 883
            }
            else
            {

#line 883
                reach_0 = reach_1;

#line 883
            }

#line 883
            to_light_1 = to_light_2;

#line 872
        }

#line 890
        float n_dot_l_1 = dot(_S17, to_light_1);
        float _S26 = max(n_dot_l_1, 0.0f);

#line 899
        float specular_0 = pow(max(dot(_S17, normalize(to_light_1 + _S18)), 0.0f), 32.0f) * (step(0.0f, _S26) * _S26);

#line 899
        float reach_2;

#line 905
        if(_S23)
        {

#line 905
            float _S27 = sun_visibility_0(_S16.world_position_1, n_dot_l_1, &kernelContext_2);

#line 905
            reach_2 = _S27;

#line 905
        }
        else
        {

#line 905
            reach_2 = reach_0;

#line 905
        }

#line 912
        float3 _S28 = (float4(light_0.color_1) ).xyz;

#line 912
        float3 direct_1 = direct_0 + _S28 * float3((_S26 * reach_2)) ;
        float3 gloss_1 = gloss_0 + _S28 * float3((specular_0 * reach_2 * 0.34999999403953552f)) ;

#line 863
        slot_0 = slot_0 + 1U;

#line 863
        direct_0 = direct_1;

#line 863
        gloss_0 = gloss_1;

#line 863
    }

#line 863
    pixelOutput_0 _S29 = { float4(albedo_0.xyz * ((&kernelContext_2)->frame_0->ambient_0.xyz + direct_0) + gloss_0, albedo_0.w) };

#line 926
    return _S29;
}


#line 926
struct vertexMain_Result_0
{
    float4 position_3 [[position]];
    float3 world_position_2 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_3 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 573
struct VertexOutput_0
{
    float4 position_4;
    float3 world_position_3;
    float3 world_normal_2;
    float4 color_4;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 573
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], uint device* cluster_lights_2 [[buffer(8)]], GpuLight_natural_0 device* lights_2 [[buffer(7)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]])
{

#line 573
    thread KernelContext_0 kernelContext_3;

#line 573
    (&kernelContext_3)->draw_0 = draw_2;

#line 573
    (&kernelContext_3)->visible_instances_0 = visible_instances_2;

#line 573
    (&kernelContext_3)->instances_0 = instances_2;

#line 573
    (&kernelContext_3)->meshes_0 = meshes_2;

#line 573
    (&kernelContext_3)->vertices_0 = vertices_2;

#line 573
    (&kernelContext_3)->frame_0 = frame_2;

#line 573
    (&kernelContext_3)->materials_0 = materials_2;

#line 573
    (&kernelContext_3)->base_color_textures_0 = base_color_textures_2;

#line 573
    (&kernelContext_3)->base_color_sampler_0 = base_color_sampler_2;

#line 573
    (&kernelContext_3)->cluster_lights_0 = cluster_lights_2;

#line 573
    (&kernelContext_3)->lights_0 = lights_2;

#line 573
    (&kernelContext_3)->shadow_atlas_0 = shadow_atlas_2;

#line 573
    (&kernelContext_3)->shadow_sampler_0 = shadow_sampler_2;

#line 613
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 620
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[draw_2->mesh_0].base_vertex_0];

#line 620
    matrix<float,int(4),int(4)>  _S30 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S30)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_4 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_3 = world_0.xyz;

#line 631
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S30[int(0)].xyz, _S30[int(1)].xyz, _S30[int(2)].xyz))));
    (&output_1)->color_4 = float4(vertex_0.color_0) ;

#line 637
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_3 = (float4(vertex_0.uv_0) ).xy;

#line 638
    thread vertexMain_Result_0 _S31;

#line 638
    (&_S31)->position_3 = output_1.position_4;

#line 638
    (&_S31)->world_position_2 = output_1.world_position_3;

#line 638
    (&_S31)->world_normal_1 = output_1.world_normal_2;

#line 638
    (&_S31)->color_3 = output_1.color_4;

#line 638
    (&_S31)->material_3 = output_1.material_4;

#line 638
    (&_S31)->uv_2 = output_1.uv_3;

#line 638
    return _S31;
}

