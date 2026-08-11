#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 341 "shaders/mesh.slang"
struct DrawConstants_0
{
    uint base_0;
    uint pad0_0;
    uint pad1_0;
    uint pad2_0;
};


#line 512
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 512
struct GpuInstance_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_0 transform_0;
    uint mesh_0;
    uint material_0;
    uint sector_0;
    uint flags_0;
};


#line 282
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


#line 516
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 516
struct _MatrixStorage_float4x4_ColMajornatural_1
{
    array<float4, int(4)> data_1;
};


#line 516
struct _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0
{
    array<_MatrixStorage_float4x4_ColMajornatural_1, int(2)> data_2;
};


#line 516
struct FrameUniforms_natural_0
{
    _MatrixStorage_float4x4_ColMajornatural_1 view_proj_0;
    float4 camera_position_0;
    float4 light_direction_0;
    float4 light_color_0;
    float4 ambient_0;
    _Array_natural_matrixx3Cfloatx2C4x2C4x3E2_0 shadow_view_proj_0;
    float4 cascade_far_0;
    float4 shadow_params_0;
};


#line 516
struct GpuMaterial_natural_0
{
    packed_float4 base_color_0;
    uint base_color_texture_0;
    uint pad0_1;
    uint pad1_1;
    uint pad2_1;
};


#line 516
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
    depth2d<float, access::sample> shadow_atlas_0;
    sampler shadow_sampler_0;
};


#line 569
float sun_visibility_0(float3 world_position_0, float n_dot_l_0, KernelContext_0 thread* kernelContext_0)
{

#line 569
    uint cascade_0;

    if(n_dot_l_0 <= 0.0f)
    {
        return 1.0f;
    }

#line 581
    float _S1 = length(world_position_0 - kernelContext_0->frame_0->camera_position_0.xyz);

#line 581
    uint index_0 = 0U;

    for(;;)
    {

#line 583
        if(index_0 < 2U)
        {
        }
        else
        {

#line 583
            cascade_0 = 1U;

#line 583
            break;
        }
        if(_S1 < kernelContext_0->frame_0->cascade_far_0[index_0])
        {

#line 585
            cascade_0 = index_0;


            break;
        }

#line 583
        index_0 = index_0 + 1U;

#line 583
    }

#line 592
    float4 clip_0 = (((float4(world_position_0, 1.0f)) * (matrix<float,int(4),int(4)> ((&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(0)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(0)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(0)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(0)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(1)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(1)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(1)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(1)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(2)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(2)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(2)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(2)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(0)][int(3)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(1)][int(3)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(2)][int(3)], (&kernelContext_0->frame_0->shadow_view_proj_0)->data_2[cascade_0].data_1[int(3)][int(3)]))));



    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 596
    float2 _S2 = float2(1.0f) ;

#line 596
    bool _S3;
    if(any((abs(ndc_0.xy)) > _S2))
    {

#line 597
        _S3 = true;

#line 597
    }
    else
    {

#line 597
        _S3 = (ndc_0.z) <= 0.0f;

#line 597
    }

#line 597
    if(_S3)
    {



        return 1.0f;
    }



    float2 _S4 = float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);
    float2 texel_0 = kernelContext_0->frame_0->shadow_params_0.xy;



    float cosine_0 = saturate(n_dot_l_0);

#line 619
    float _S5 = ndc_0.z + (kernelContext_0->frame_0->shadow_params_0.z + kernelContext_0->frame_0->shadow_params_0.w * min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f));

#line 625
    float2 _S6 = float2(2.0f, 1.0f);

#line 625
    float2 _S7 = float2(0.5f, 0.5f) * texel_0 * _S6;

#line 625
    int y_0 = int(-1);

#line 625
    float visibility_0 = 0.0f;

    for(;;)
    {

#line 627
        if(y_0 <= int(1))
        {
        }
        else
        {

#line 627
            break;
        }

#line 627
        int x_0 = int(-1);

        for(;;)
        {

#line 629
            if(x_0 <= int(1))
            {
            }
            else
            {

#line 629
                break;
            }

#line 636
            float2 tap_0 = clamp(_S4 + float2(float(x_0), float(y_0)) * texel_0 * _S6, _S7, _S2 - _S7);


            float _S8 = ((kernelContext_0->shadow_atlas_0).sample_compare((kernelContext_0->shadow_sampler_0), (float2((float(cascade_0) + tap_0.x) / 2.0f, tap_0.y)), (_S5), level((0.0f))));

#line 639
            float visibility_1 = visibility_0 + _S8;

#line 629
            x_0 = x_0 + int(1);

#line 629
            visibility_0 = visibility_1;

#line 629
        }

#line 627
        y_0 = y_0 + int(1);

#line 627
    }

#line 642
    return visibility_0 / 9.0f;
}


#line 642
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 642
struct pixelInput_0
{
    float3 world_position_1 [[user(POSITION)]];
    float3 world_normal_0 [[user(NORMAL)]];
    float4 color_1 [[user(COLOR)]];
    [[flat]] uint material_1 [[user(TEXCOORD)]];
    float2 uv_1 [[user(TEXCOORD_1)]];
};


#line 646
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S9 [[stage_in]], float4 position_1 [[position]], DrawConstants_0 constant* draw_1 [[buffer(3)]], uint device* visible_instances_1 [[buffer(5)]], GpuInstance_natural_0 device* instances_1 [[buffer(2)]], GpuMesh_0 device* meshes_1 [[buffer(4)]], MeshVertex_natural_0 device* vertices_1 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_1 [[buffer(0)]], GpuMaterial_natural_0 device* materials_1 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_1 [[texture(0)]], sampler base_color_sampler_1 [[sampler(0)]], depth2d<float, access::sample> shadow_atlas_1 [[texture(1)]], sampler shadow_sampler_1 [[sampler(1)]])
{

#line 646
    thread KernelContext_0 kernelContext_1;

#line 646
    (&kernelContext_1)->draw_0 = draw_1;

#line 646
    (&kernelContext_1)->visible_instances_0 = visible_instances_1;

#line 646
    (&kernelContext_1)->instances_0 = instances_1;

#line 646
    (&kernelContext_1)->meshes_0 = meshes_1;

#line 646
    (&kernelContext_1)->vertices_0 = vertices_1;

#line 646
    (&kernelContext_1)->frame_0 = frame_1;

#line 646
    (&kernelContext_1)->materials_0 = materials_1;

#line 646
    (&kernelContext_1)->base_color_textures_0 = base_color_textures_1;

#line 646
    (&kernelContext_1)->base_color_sampler_0 = base_color_sampler_1;

#line 646
    (&kernelContext_1)->shadow_atlas_0 = shadow_atlas_1;

#line 646
    (&kernelContext_1)->shadow_sampler_0 = shadow_sampler_1;



    float3 normal_1 = normalize(_S9.world_normal_0);
    float3 to_light_0 = normalize(frame_1->light_direction_0.xyz);

#line 656
    GpuMaterial_natural_0 material_2 = materials_1[_S9.material_1];

#line 666
    float3 _S10 = float3(_S9.uv_1, float(material_2.base_color_texture_0));
    float4 albedo_0 = _S9.color_1 * float4(material_2.base_color_0)  * ((base_color_textures_1).sample((base_color_sampler_1), ((_S10)).xy, uint(((_S10)).z)));


    float n_dot_l_1 = dot(normal_1, to_light_0);
    float _S11 = max(n_dot_l_1, 0.0f);

#line 678
    float specular_0 = pow(max(dot(normal_1, normalize(to_light_0 + normalize(frame_1->camera_position_0.xyz - _S9.world_position_1))), 0.0f), 32.0f) * (step(0.0f, _S11) * _S11);

#line 678
    float _S12 = sun_visibility_0(_S9.world_position_1, n_dot_l_1, &kernelContext_1);

#line 678
    pixelOutput_0 _S13 = { float4(albedo_0.xyz * ((&kernelContext_1)->frame_0->ambient_0.xyz + (&kernelContext_1)->frame_0->light_color_0.xyz * float3((_S11 * _S12)) ) + (&kernelContext_1)->frame_0->light_color_0.xyz * float3((specular_0 * _S12 * 0.34999999403953552f)) , albedo_0.w) };

#line 695
    return _S13;
}


#line 695
struct vertexMain_Result_0
{
    float4 position_2 [[position]];
    float3 world_position_2 [[user(POSITION)]];
    float3 world_normal_1 [[user(NORMAL)]];
    float4 color_2 [[user(COLOR)]];
    uint material_3 [[user(TEXCOORD)]];
    float2 uv_2 [[user(TEXCOORD_1)]];
};


#line 472
struct VertexOutput_0
{
    float4 position_3;
    float3 world_position_3;
    float3 world_normal_2;
    float4 color_3;
    [[flat]] uint material_4;
    float2 uv_3;
};


#line 472
[[vertex]] vertexMain_Result_0 vertexMain(uint index_1 [[vertex_id]], uint instance_id_0 [[instance_id]], DrawConstants_0 constant* draw_2 [[buffer(3)]], uint device* visible_instances_2 [[buffer(5)]], GpuInstance_natural_0 device* instances_2 [[buffer(2)]], GpuMesh_0 device* meshes_2 [[buffer(4)]], MeshVertex_natural_0 device* vertices_2 [[buffer(1)]], FrameUniforms_natural_0 constant* frame_2 [[buffer(0)]], GpuMaterial_natural_0 device* materials_2 [[buffer(6)]], texture2d_array<float, access::sample> base_color_textures_2 [[texture(0)]], sampler base_color_sampler_2 [[sampler(0)]], depth2d<float, access::sample> shadow_atlas_2 [[texture(1)]], sampler shadow_sampler_2 [[sampler(1)]])
{

#line 472
    thread KernelContext_0 kernelContext_2;

#line 472
    (&kernelContext_2)->draw_0 = draw_2;

#line 472
    (&kernelContext_2)->visible_instances_0 = visible_instances_2;

#line 472
    (&kernelContext_2)->instances_0 = instances_2;

#line 472
    (&kernelContext_2)->meshes_0 = meshes_2;

#line 472
    (&kernelContext_2)->vertices_0 = vertices_2;

#line 472
    (&kernelContext_2)->frame_0 = frame_2;

#line 472
    (&kernelContext_2)->materials_0 = materials_2;

#line 472
    (&kernelContext_2)->base_color_textures_0 = base_color_textures_2;

#line 472
    (&kernelContext_2)->base_color_sampler_0 = base_color_sampler_2;

#line 472
    (&kernelContext_2)->shadow_atlas_0 = shadow_atlas_2;

#line 472
    (&kernelContext_2)->shadow_sampler_0 = shadow_sampler_2;

#line 512
    GpuInstance_natural_0 instance_0 = instances_2[visible_instances_2[draw_2->base_0 + instance_id_0]];

#line 517
    MeshVertex_natural_0 vertex_0 = vertices_2[index_1 + meshes_2[instance_0.mesh_0].base_vertex_0];

#line 517
    matrix<float,int(4),int(4)>  _S14 = matrix<float,int(4),int(4)> (instance_0.transform_0.data_0[int(0)][int(0)], instance_0.transform_0.data_0[int(1)][int(0)], instance_0.transform_0.data_0[int(2)][int(0)], instance_0.transform_0.data_0[int(3)][int(0)], instance_0.transform_0.data_0[int(0)][int(1)], instance_0.transform_0.data_0[int(1)][int(1)], instance_0.transform_0.data_0[int(2)][int(1)], instance_0.transform_0.data_0[int(3)][int(1)], instance_0.transform_0.data_0[int(0)][int(2)], instance_0.transform_0.data_0[int(1)][int(2)], instance_0.transform_0.data_0[int(2)][int(2)], instance_0.transform_0.data_0[int(3)][int(2)], instance_0.transform_0.data_0[int(0)][int(3)], instance_0.transform_0.data_0[int(1)][int(3)], instance_0.transform_0.data_0[int(2)][int(3)], instance_0.transform_0.data_0[int(3)][int(3)]);

    float4 world_0 = (((float4((float4(vertex_0.position_0) ).xyz, 1.0f)) * (_S14)));

    thread VertexOutput_0 output_1;
    (&output_1)->position_3 = (((world_0) * (matrix<float,int(4),int(4)> (frame_2->view_proj_0.data_1[int(0)][int(0)], frame_2->view_proj_0.data_1[int(1)][int(0)], frame_2->view_proj_0.data_1[int(2)][int(0)], frame_2->view_proj_0.data_1[int(3)][int(0)], frame_2->view_proj_0.data_1[int(0)][int(1)], frame_2->view_proj_0.data_1[int(1)][int(1)], frame_2->view_proj_0.data_1[int(2)][int(1)], frame_2->view_proj_0.data_1[int(3)][int(1)], frame_2->view_proj_0.data_1[int(0)][int(2)], frame_2->view_proj_0.data_1[int(1)][int(2)], frame_2->view_proj_0.data_1[int(2)][int(2)], frame_2->view_proj_0.data_1[int(3)][int(2)], frame_2->view_proj_0.data_1[int(0)][int(3)], frame_2->view_proj_0.data_1[int(1)][int(3)], frame_2->view_proj_0.data_1[int(2)][int(3)], frame_2->view_proj_0.data_1[int(3)][int(3)]))));
    (&output_1)->world_position_3 = world_0.xyz;

#line 528
    (&output_1)->world_normal_2 = ((((float4(vertex_0.normal_0) ).xyz) * (matrix<float,int(3),int(3)> (_S14[int(0)].xyz, _S14[int(1)].xyz, _S14[int(2)].xyz))));
    (&output_1)->color_3 = float4(vertex_0.color_0) ;

#line 534
    (&output_1)->material_4 = instance_0.material_0;
    (&output_1)->uv_3 = (float4(vertex_0.uv_0) ).xy;

#line 535
    thread vertexMain_Result_0 _S15;

#line 535
    (&_S15)->position_2 = output_1.position_3;

#line 535
    (&_S15)->world_position_2 = output_1.world_position_3;

#line 535
    (&_S15)->world_normal_1 = output_1.world_normal_2;

#line 535
    (&_S15)->color_2 = output_1.color_3;

#line 535
    (&_S15)->material_3 = output_1.material_4;

#line 535
    (&_S15)->uv_2 = output_1.uv_3;

#line 535
    return _S15;
}

