#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

template<typename T, int A, int B>
matrix<T,A,B> _slang_matrixCompMult(matrix<T,A,B> m1, matrix<T,A,B> m2)
{
    matrix<T,A,B> result;
    for (int i = 0; i < A; i++)
        result[i] = m1[i] * m2[i];
    return result;
}


#line 209 "shaders/skinning.slang"
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_0)
{
    return matrix<float,int(3),int(3)> (cross(basis_0[int(1)], basis_0[int(2)]), cross(basis_0[int(2)], basis_0[int(0)]), cross(basis_0[int(0)], basis_0[int(1)]));
}


#line 78
struct SkinParams_0
{
    uint vertex_count_0;
    uint input_base_0;
    uint output_base_0;
    uint binding_base_0;
    uint joint_base_0;
    uint joint_count_0;
};


#line 78
struct MeshVertex_natural_0
{
    packed_float4 position_0;
    packed_float4 normal_0;
    packed_float4 color_0;
    packed_float4 uv_0;
};


#line 78
struct SkinBinding_natural_0
{
    packed_uint4 joints_0;
    packed_float4 weights_0;
};


#line 78
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 78
struct KernelContext_0
{
    SkinParams_0 constant* skin_0;
    MeshVertex_natural_0 device* vertices_0;
    SkinBinding_natural_0 device* bindings_0;
    _MatrixStorage_float4x4_ColMajornatural_0 device* joints_1;
};


#line 117
struct MeshVertex_0
{
    float4 position_0;
    float4 normal_0;
    float4 color_0;
    float4 uv_0;
};


#line 261
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], SkinParams_0 constant* skin_1 [[buffer(0)]], MeshVertex_natural_0 device* vertices_1 [[buffer(3)]], SkinBinding_natural_0 device* bindings_1 [[buffer(2)]], _MatrixStorage_float4x4_ColMajornatural_0 device* joints_2 [[buffer(1)]])
{

#line 261
    thread KernelContext_0 kernelContext_0;

#line 261
    (&kernelContext_0)->skin_0 = skin_1;

#line 261
    (&kernelContext_0)->vertices_0 = vertices_1;

#line 261
    (&kernelContext_0)->bindings_0 = bindings_1;

#line 261
    (&kernelContext_0)->joints_1 = joints_2;

    uint index_0 = thread_0.x;
    if(index_0 >= (skin_1->vertex_count_0))
    {
        return;
    }

#line 266
    MeshVertex_natural_0 device* _S1 = (&kernelContext_0)->vertices_0+((&kernelContext_0)->skin_0->input_base_0 + index_0);


    MeshVertex_natural_0 vertex_0 = *_S1;
    SkinBinding_natural_0 binding_0 = (&kernelContext_0)->bindings_0[(&kernelContext_0)->skin_0->binding_base_0 + index_0];



    uint last_0 = (&kernelContext_0)->skin_0->joint_count_0 - 1U;
    uint4 joint_0 = min(uint4(binding_0.joints_0) , uint4(last_0, last_0, last_0, last_0));

#line 275
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S2 = (&kernelContext_0)->joints_1+((&kernelContext_0)->skin_0->joint_base_0 + joint_0.x);

#line 275
    float4 _S3 = float4(binding_0.weights_0) ;

#line 275
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S4 = (&kernelContext_0)->joints_1+((&kernelContext_0)->skin_0->joint_base_0 + joint_0.y);

#line 275
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S5 = (&kernelContext_0)->joints_1+((&kernelContext_0)->skin_0->joint_base_0 + joint_0.z);

#line 275
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S6 = (&kernelContext_0)->joints_1+((&kernelContext_0)->skin_0->joint_base_0 + joint_0.w);

#line 280
    matrix<float,int(4),int(4)>  blended_0 = _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S2).data_0[int(0)][int(0)], (*_S2).data_0[int(1)][int(0)], (*_S2).data_0[int(2)][int(0)], (*_S2).data_0[int(3)][int(0)], (*_S2).data_0[int(0)][int(1)], (*_S2).data_0[int(1)][int(1)], (*_S2).data_0[int(2)][int(1)], (*_S2).data_0[int(3)][int(1)], (*_S2).data_0[int(0)][int(2)], (*_S2).data_0[int(1)][int(2)], (*_S2).data_0[int(2)][int(2)], (*_S2).data_0[int(3)][int(2)], (*_S2).data_0[int(0)][int(3)], (*_S2).data_0[int(1)][int(3)], (*_S2).data_0[int(2)][int(3)], (*_S2).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x, _S3.x)) + _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S4).data_0[int(0)][int(0)], (*_S4).data_0[int(1)][int(0)], (*_S4).data_0[int(2)][int(0)], (*_S4).data_0[int(3)][int(0)], (*_S4).data_0[int(0)][int(1)], (*_S4).data_0[int(1)][int(1)], (*_S4).data_0[int(2)][int(1)], (*_S4).data_0[int(3)][int(1)], (*_S4).data_0[int(0)][int(2)], (*_S4).data_0[int(1)][int(2)], (*_S4).data_0[int(2)][int(2)], (*_S4).data_0[int(3)][int(2)], (*_S4).data_0[int(0)][int(3)], (*_S4).data_0[int(1)][int(3)], (*_S4).data_0[int(2)][int(3)], (*_S4).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y, _S3.y)) + _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S5).data_0[int(0)][int(0)], (*_S5).data_0[int(1)][int(0)], (*_S5).data_0[int(2)][int(0)], (*_S5).data_0[int(3)][int(0)], (*_S5).data_0[int(0)][int(1)], (*_S5).data_0[int(1)][int(1)], (*_S5).data_0[int(2)][int(1)], (*_S5).data_0[int(3)][int(1)], (*_S5).data_0[int(0)][int(2)], (*_S5).data_0[int(1)][int(2)], (*_S5).data_0[int(2)][int(2)], (*_S5).data_0[int(3)][int(2)], (*_S5).data_0[int(0)][int(3)], (*_S5).data_0[int(1)][int(3)], (*_S5).data_0[int(2)][int(3)], (*_S5).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z, _S3.z)) + _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S6).data_0[int(0)][int(0)], (*_S6).data_0[int(1)][int(0)], (*_S6).data_0[int(2)][int(0)], (*_S6).data_0[int(3)][int(0)], (*_S6).data_0[int(0)][int(1)], (*_S6).data_0[int(1)][int(1)], (*_S6).data_0[int(2)][int(1)], (*_S6).data_0[int(3)][int(1)], (*_S6).data_0[int(0)][int(2)], (*_S6).data_0[int(1)][int(2)], (*_S6).data_0[int(2)][int(2)], (*_S6).data_0[int(3)][int(2)], (*_S6).data_0[int(0)][int(3)], (*_S6).data_0[int(1)][int(3)], (*_S6).data_0[int(2)][int(3)], (*_S6).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w, _S3.w));

#line 295
    float3 _S7 = (float4((*_S1).normal_0) ).xyz;

#line 295
    float3 normal_1 = (((_S7) * (normal_basis_0(matrix<float,int(3),int(3)> (blended_0[int(0)].xyz, blended_0[int(1)].xyz, blended_0[int(2)].xyz)))));

    thread MeshVertex_0 skinned_0;
    (&skinned_0)->position_0 = float4((((float4((float4((*_S1).position_0) ).xyz, 1.0f)) * (blended_0))).xyz, 1.0f);

#line 307
    float square_length_0 = dot(normal_1, normal_1);

#line 307
    float3 _S8;
    if(square_length_0 > 0.0f)
    {

#line 308
        _S8 = normal_1 * float3(rsqrt(square_length_0)) ;

#line 308
    }
    else
    {

#line 308
        _S8 = _S7;

#line 308
    }

#line 308
    (&skinned_0)->normal_0 = float4(_S8, 0.0f);


    (&skinned_0)->color_0 = float4(vertex_0.color_0) ;
    (&skinned_0)->uv_0 = float4(vertex_0.uv_0) ;

#line 312
    MeshVertex_natural_0 device* _S9 = (&kernelContext_0)->vertices_0+((&kernelContext_0)->skin_0->output_base_0 + index_0);

#line 312
    _S9->position_0 = packed_float4(skinned_0.position_0) ;

#line 312
    _S9->normal_0 = packed_float4(skinned_0.normal_0) ;

#line 312
    _S9->color_0 = packed_float4(skinned_0.color_0) ;

#line 312
    _S9->uv_0 = packed_float4(skinned_0.uv_0) ;


    return;
}

