#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 59 "shaders/mesh_shader.slang"
struct Amplification_0
{
    float4 tint_0;
};


#line 75
constant array<float4, int(3)> COLORS_0 = { float4(1.0f, 0.0f, 0.0f, 1.0f), float4(0.0f, 1.0f, 0.0f, 1.0f), float4(0.0f, 0.0f, 1.0f, 1.0f) };

#line 65
constant array<float4, int(3)> POSITIONS_0 = { float4(0.0f, -0.69999998807907104f, 0.5f, 1.0f), float4(-0.60000002384185791f, 0.60000002384185791f, 0.5f, 1.0f), float4(0.60000002384185791f, 0.60000002384185791f, 0.5f, 1.0f) };

#line 48
struct MeshVertex_0
{
    float4 position_0 [[position]];
    float4 color_0;
};


#line 118
[[mesh]] void meshMain(uint3 index_0 [[thread_position_in_grid]], metal::mesh<MeshVertex_0, void, int(3), int(1), metal::topology::triangle> _slang_mesh)
{

#line 119
    uint index_1 = index_0.x;



    _slang_mesh.set_primitive_count((1U));

#line 123
    thread MeshVertex_0 vertex_0;

#line 123
    (&vertex_0)->position_0 = POSITIONS_0[index_1];

#line 123
    (&vertex_0)->color_0 = COLORS_0[index_1];

#line 123
    _slang_mesh.set_vertex(index_1,vertex_0);

#line 123
    if(index_1 == 0U)
    {

#line 123
        _slang_mesh.set_index(0U*3+0,(uint3(0U, 1U, 2U))[0]);
        _slang_mesh.set_index(0U*3+1,(uint3(0U, 1U, 2U))[1]);
        _slang_mesh.set_index(0U*3+2,(uint3(0U, 1U, 2U))[2]);
        ;

#line 123
    }
    return;
}


#line 136
[[object]] void taskMain(Amplification_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp)
{

#line 136
    threadgroup Amplification_0 amplification_0;

    (&amplification_0)->tint_0 = float4(0.0f, 1.0f, 1.0f, 1.0f);
    *_slang_mesh_payload = *(&amplification_0); _slang_mgp.set_threadgroups_per_grid(uint3((1U), (1U), (1U))); return;;
    return;
}


#line 148
[[mesh]] void amplifiedMeshMain(uint3 index_2 [[thread_position_in_grid]], const Amplification_0 object_data* amplification_1 [[payload]], metal::mesh<MeshVertex_0, void, int(3), int(1), metal::topology::triangle> _slang_mesh)
{
    uint index_3 = index_2.x;



    _slang_mesh.set_primitive_count((1U));

#line 154
    thread MeshVertex_0 vertex_1;

#line 154
    (&vertex_1)->position_0 = POSITIONS_0[index_3];

#line 154
    (&vertex_1)->color_0 = COLORS_0[index_3] * amplification_1->tint_0;

#line 154
    _slang_mesh.set_vertex(index_3,vertex_1);

#line 154
    if(index_3 == 0U)
    {

#line 154
        _slang_mesh.set_index(0U*3+0,(uint3(0U, 1U, 2U))[0]);
        _slang_mesh.set_index(0U*3+1,(uint3(0U, 1U, 2U))[1]);
        _slang_mesh.set_index(0U*3+2,(uint3(0U, 1U, 2U))[2]);
        ;

#line 154
    }
    return;
}


#line 155
struct pixelOutput_0
{
    float4 output_0 [[color(0)]];
};


#line 155
struct pixelInput_0
{
    float4 color_1 [[user(COLOR)]];
};


#line 159
[[fragment]] pixelOutput_0 fragmentMain(pixelInput_0 _S1 [[stage_in]], float4 position_1 [[position]])
{

#line 159
    pixelOutput_0 _S2 = { _S1.color_1 };

    return _S2;
}

