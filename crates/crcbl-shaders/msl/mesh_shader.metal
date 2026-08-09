#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 83 "shaders/mesh_shader.slang"
struct Amplification_0
{
    float4 tint_0;
};


#line 72
struct MeshVertex_0
{
    float4 position_0 [[position]];
    float4 color_0;
};


#line 72
struct Vertex_natural_0
{
    packed_float4 position_1;
    packed_float4 color_1;
};


#line 72
struct KernelContext_0
{
    Vertex_natural_0 device* vertices_0;
    Amplification_0 threadgroup* amplification_0;
};


#line 127
[[mesh]] void meshMain(uint3 index_0 [[thread_position_in_grid]], metal::mesh<MeshVertex_0, void, int(3), int(1), metal::topology::triangle> _slang_mesh, Vertex_natural_0 device* vertices_1 [[buffer(0)]])
{

#line 128
    thread KernelContext_0 kernelContext_0;

#line 128
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 128
    threadgroup Amplification_0 amplification_1;

#line 128
    (&kernelContext_0)->amplification_0 = &amplification_1;

#line 128
    uint index_1 = index_0.x;



    _slang_mesh.set_primitive_count((1U));

#line 132
    Vertex_natural_0 pulled_0 = vertices_1[index_1];

#line 132
    thread MeshVertex_0 vertex_0;

#line 132
    (&vertex_0)->position_0 = float4(pulled_0.position_1) ;

#line 132
    (&vertex_0)->color_0 = float4(pulled_0.color_1) ;

#line 132
    _slang_mesh.set_vertex(index_1,vertex_0);

#line 132
    if(index_1 == 0U)
    {

#line 132
        _slang_mesh.set_index(0U*3+0,(uint3(0U, 1U, 2U))[0]);
        _slang_mesh.set_index(0U*3+1,(uint3(0U, 1U, 2U))[1]);
        _slang_mesh.set_index(0U*3+2,(uint3(0U, 1U, 2U))[2]);
        ;

#line 132
    }
    return;
}


#line 145
[[object]] void taskMain(Amplification_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, Vertex_natural_0 device* vertices_2 [[buffer(0)]])
{

#line 145
    thread KernelContext_0 kernelContext_1;

#line 145
    (&kernelContext_1)->vertices_0 = vertices_2;

#line 145
    threadgroup Amplification_0 amplification_2;

#line 145
    (&kernelContext_1)->amplification_0 = &amplification_2;

    (&amplification_2)->tint_0 = float4(0.0f, 1.0f, 1.0f, 1.0f);
    *_slang_mesh_payload = *(&amplification_2); _slang_mgp.set_threadgroups_per_grid(uint3((1U), (1U), (1U))); return;;
    return;
}


#line 157
[[mesh]] void amplifiedMeshMain(uint3 index_2 [[thread_position_in_grid]], const Amplification_0 object_data* amplification_3 [[payload]], metal::mesh<MeshVertex_0, void, int(3), int(1), metal::topology::triangle> _slang_mesh, Vertex_natural_0 device* vertices_3 [[buffer(0)]])
{
    thread KernelContext_0 kernelContext_2;

#line 159
    (&kernelContext_2)->vertices_0 = vertices_3;

#line 159
    threadgroup Amplification_0 amplification_4;

#line 159
    (&kernelContext_2)->amplification_0 = &amplification_4;

#line 159
    uint index_3 = index_2.x;



    _slang_mesh.set_primitive_count((1U));

#line 163
    Vertex_natural_0 pulled_1 = vertices_3[index_3];

#line 163
    thread MeshVertex_0 vertex_1;

#line 163
    (&vertex_1)->position_0 = float4(pulled_1.position_1) ;

#line 163
    (&vertex_1)->color_0 = float4(pulled_1.color_1)  * amplification_3->tint_0;

#line 163
    _slang_mesh.set_vertex(index_3,vertex_1);

#line 163
    if(index_3 == 0U)
    {

#line 163
        _slang_mesh.set_index(0U*3+0,(uint3(0U, 1U, 2U))[0]);
        _slang_mesh.set_index(0U*3+1,(uint3(0U, 1U, 2U))[1]);
        _slang_mesh.set_index(0U*3+2,(uint3(0U, 1U, 2U))[2]);
        ;

#line 163
    }
    return;
}


#line 164
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 164
struct pixelInput_0
{
    float4 color_2 [[user(COLOR)]];
};


#line 168
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_2 [[position]], Vertex_natural_0 device* vertices_4 [[buffer(0)]])
{

#line 168
    thread KernelContext_0 kernelContext_3;

#line 168
    (&kernelContext_3)->vertices_0 = vertices_4;

#line 168
    threadgroup Amplification_0 amplification_5;

#line 168
    (&kernelContext_3)->amplification_0 = &amplification_5;

#line 168
    pixelOutput_0 _S2 = { _S1.color_2 };

    return _S2;
}

