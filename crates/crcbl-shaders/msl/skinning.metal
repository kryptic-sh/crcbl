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


#line 78 "shaders/skinning.slang"
struct SkinParams_0
{
    uint vertex_count_0;
    uint input_base_0;
    uint output_base_0;
    uint binding_base_0;
    uint joint_base_0;
    uint joint_count_0;
    uint attribute_base_0;
};


#line 360
struct SkinBinding_natural_0
{
    packed_uint4 joints_0;
    packed_float4 weights_0;
};


#line 360
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 360
struct KernelContext_0
{
    SkinParams_0 constant* skin_0;
    uint device* vertices_0;
    SkinBinding_natural_0 device* bindings_0;
    _MatrixStorage_float4x4_ColMajornatural_0 device* joints_1;
};


#line 357
float3 load_position_0(uint at_0, KernelContext_0 thread* kernelContext_0)
{
    uint word_0 = at_0 * 3U;
    return float3((as_type<float>((*(kernelContext_0->vertices_0+word_0)))), (as_type<float>((*(kernelContext_0->vertices_0+(word_0 + 1U))))), (as_type<float>((*(kernelContext_0->vertices_0+(word_0 + 2U))))));
}


#line 143
float dequantise_snorm_0(int lane_0)
{
    return max(float(lane_0) / 32767.0f, -1.0f);
}


float4 unpack_snorm16x4_0(uint low_0, uint high_0)
{
    return float4(dequantise_snorm_0((as_type<int>((low_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((low_0))) >> 16U), dequantise_snorm_0((as_type<int>((high_0 << 16U))) >> 16U), dequantise_snorm_0((as_type<int>((high_0))) >> 16U));
}


#line 175
float3 rotate_by_0(float4 q_0, float3 v_0)
{
    float3 _S1 = q_0.xyz;

#line 177
    float3 t_0 = float3(2.0f)  * cross(_S1, v_0);
    return v_0 + float3(q_0.w)  * t_0 + cross(_S1, t_0);
}


#line 133
struct TangentFrame_0
{
    float3 tangent_0;
    float3 bitangent_0;
    float3 normal_0;
};


#line 189
TangentFrame_0 decode_qtangent_0(float4 lanes_0)
{
    float4 q_1 = normalize(lanes_0);
    thread TangentFrame_0 basis_0;
    float3 _S2 = rotate_by_0(q_1, float3(1.0f, 0.0f, 0.0f));

#line 193
    (&basis_0)->tangent_0 = _S2;
    float3 _S3 = rotate_by_0(q_1, float3(0.0f, 0.0f, 1.0f));

#line 194
    (&basis_0)->normal_0 = _S3;
    float3 _S4 = cross(_S3, _S2);

#line 195
    float _S5;

#line 195
    if((lanes_0.w) < 0.0f)
    {

#line 195
        _S5 = -1.0f;

#line 195
    }
    else
    {

#line 195
        _S5 = 1.0f;

#line 195
    }

#line 195
    (&basis_0)->bitangent_0 = _S4 * float3(_S5) ;
    return basis_0;
}


#line 371
void load_attributes_0(uint at_1, TangentFrame_0 thread* basis_1, uint3 thread* copied_0, KernelContext_0 thread* kernelContext_1)
{
    uint word_1 = kernelContext_1->skin_0->attribute_base_0 + at_1 * 5U;
    *basis_1 = decode_qtangent_0(unpack_snorm16x4_0(*(kernelContext_1->vertices_0+word_1), *(kernelContext_1->vertices_0+(word_1 + 1U))));
    *copied_0 = uint3(*(kernelContext_1->vertices_0+(word_1 + 2U)), *(kernelContext_1->vertices_0+(word_1 + 3U)), *(kernelContext_1->vertices_0+(word_1 + 4U)));
    return;
}


#line 410
matrix<float,int(3),int(3)>  normal_basis_0(matrix<float,int(3),int(3)>  basis_2)
{
    return matrix<float,int(3),int(3)> (cross(basis_2[int(1)], basis_2[int(2)]), cross(basis_2[int(2)], basis_2[int(0)]), cross(basis_2[int(0)], basis_2[int(1)]));
}


#line 233
float4 quaternion_from_columns_0(float3 x_0, float3 y_0, float3 z_0)
{
    float m00_0 = x_0.x;

#line 235
    float m01_0 = y_0.x;

#line 235
    float m02_0 = z_0.x;
    float m10_0 = x_0.y;

#line 236
    float m11_0 = y_0.y;

#line 236
    float m12_0 = z_0.y;
    float m20_0 = x_0.z;

#line 237
    float m21_0 = y_0.z;

#line 237
    float m22_0 = z_0.z;
    float trace_0 = m00_0 + m11_0 + m22_0;
    if(trace_0 > 0.0f)
    {
        float s_0 = sqrt(trace_0 + 1.0f) * 2.0f;
        return float4((m21_0 - m12_0) / s_0, (m02_0 - m20_0) / s_0, (m10_0 - m01_0) / s_0, 0.25f * s_0);
    }

#line 242
    bool _S6;

    if(m00_0 > m11_0)
    {

#line 244
        _S6 = m00_0 > m22_0;

#line 244
    }
    else
    {

#line 244
        _S6 = false;

#line 244
    }

#line 244
    if(_S6)
    {
        float s_1 = sqrt(1.0f + m00_0 - m11_0 - m22_0) * 2.0f;
        return float4(0.25f * s_1, (m01_0 + m10_0) / s_1, (m02_0 + m20_0) / s_1, (m21_0 - m12_0) / s_1);
    }
    if(m11_0 > m22_0)
    {
        float s_2 = sqrt(1.0f + m11_0 - m00_0 - m22_0) * 2.0f;
        return float4((m01_0 + m10_0) / s_2, 0.25f * s_2, (m12_0 + m21_0) / s_2, (m02_0 - m20_0) / s_2);
    }
    float s_3 = sqrt(1.0f + m22_0 - m00_0 - m11_0) * 2.0f;
    return float4((m02_0 + m20_0) / s_3, (m12_0 + m21_0) / s_3, 0.25f * s_3, (m10_0 - m01_0) / s_3);
}


#line 215
int quantise_snorm_0(float value_0)
{
    float scaled_0 = clamp(value_0, -1.0f, 1.0f) * 32767.0f;

#line 217
    float _S7;
    if(scaled_0 < 0.0f)
    {

#line 218
        _S7 = - floor(0.5f - scaled_0);

#line 218
    }
    else
    {

#line 218
        _S7 = floor(scaled_0 + 0.5f);

#line 218
    }

#line 218
    return int(_S7);
}


uint pack_snorm16x2_0(float low_1, float high_1)
{
    return (uint(quantise_snorm_0(low_1)) & 65535U) | (uint(quantise_snorm_0(high_1)) << 16U);
}


#line 265
uint2 encode_qtangent_0(const TangentFrame_0 thread* basis_3)
{

#line 265
    float3 _S8 = basis_3->normal_0;

#line 265
    float3 _S9 = basis_3->tangent_0;

#line 265
    float3 _S10 = basis_3->bitangent_0;

#line 265
    float handedness_0;


    if((dot(cross(basis_3->normal_0, basis_3->tangent_0), basis_3->bitangent_0)) < 0.0f)
    {

#line 268
        handedness_0 = -1.0f;

#line 268
    }
    else
    {

#line 268
        handedness_0 = 1.0f;

#line 268
    }

    float4 q_2 = normalize(quaternion_from_columns_0(_S9, _S10 * float3(handedness_0) , _S8));

#line 270
    float4 q_3;


    if((q_2.w) < 0.0f)
    {

#line 273
        q_3 = - q_2;

#line 273
    }
    else
    {

#line 273
        q_3 = q_2;

#line 273
    }



    float _S11 = q_3.w;

#line 277
    if(_S11 < 0.00003051850944757f)
    {

#line 277
        q_3 = float4(q_3.xyz * float3((sqrt(1.0f) / sqrt(1.0f - _S11 * _S11))) , 0.00003051850944757f);

#line 277
    }

#line 282
    if(handedness_0 < 0.0f)
    {

#line 282
        q_3 = - q_3;

#line 282
    }



    return uint2(pack_snorm16x2_0(q_3.x, q_3.y), pack_snorm16x2_0(q_3.z, q_3.w));
}


#line 379
void store_vertex_0(uint at_2, float3 position_0, const TangentFrame_0 thread* basis_4, uint3 copied_1, KernelContext_0 thread* kernelContext_2)
{
    uint word_2 = at_2 * 3U;
    *(kernelContext_2->vertices_0+word_2) = (as_type<uint>((position_0.x)));
    *(kernelContext_2->vertices_0+(word_2 + 1U)) = (as_type<uint>((position_0.y)));
    *(kernelContext_2->vertices_0+(word_2 + 2U)) = (as_type<uint>((position_0.z)));

#line 384
    uint2 _S12 = encode_qtangent_0(basis_4);

    uint word_3 = kernelContext_2->skin_0->attribute_base_0 + at_2 * 5U;
    *(kernelContext_2->vertices_0+word_3) = _S12.x;
    *(kernelContext_2->vertices_0+(word_3 + 1U)) = _S12.y;
    *(kernelContext_2->vertices_0+(word_3 + 2U)) = copied_1.x;
    *(kernelContext_2->vertices_0+(word_3 + 3U)) = copied_1.y;
    *(kernelContext_2->vertices_0+(word_3 + 4U)) = copied_1.z;
    return;
}


#line 462
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], SkinParams_0 constant* skin_1 [[buffer(0)]], uint device* vertices_1 [[buffer(3)]], SkinBinding_natural_0 device* bindings_1 [[buffer(2)]], _MatrixStorage_float4x4_ColMajornatural_0 device* joints_2 [[buffer(1)]])
{

#line 462
    thread KernelContext_0 kernelContext_3;

#line 462
    (&kernelContext_3)->skin_0 = skin_1;

#line 462
    (&kernelContext_3)->vertices_0 = vertices_1;

#line 462
    (&kernelContext_3)->bindings_0 = bindings_1;

#line 462
    (&kernelContext_3)->joints_1 = joints_2;

    uint index_0 = thread_0.x;
    if(index_0 >= (skin_1->vertex_count_0))
    {
        return;
    }

#line 467
    float3 _S13 = load_position_0((&kernelContext_3)->skin_0->input_base_0 + index_0, &kernelContext_3);



    thread TangentFrame_0 bind_basis_0;
    thread uint3 copied_2;

#line 472
    load_attributes_0((&kernelContext_3)->skin_0->input_base_0 + index_0, &bind_basis_0, &copied_2, &kernelContext_3);

    SkinBinding_natural_0 binding_0 = (&kernelContext_3)->bindings_0[(&kernelContext_3)->skin_0->binding_base_0 + index_0];



    uint last_0 = (&kernelContext_3)->skin_0->joint_count_0 - 1U;
    uint4 joint_0 = min(uint4(binding_0.joints_0) , uint4(last_0, last_0, last_0, last_0));

#line 479
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S14 = (&kernelContext_3)->joints_1+((&kernelContext_3)->skin_0->joint_base_0 + joint_0.x);

#line 479
    float4 _S15 = float4(binding_0.weights_0) ;

#line 479
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S16 = (&kernelContext_3)->joints_1+((&kernelContext_3)->skin_0->joint_base_0 + joint_0.y);

#line 479
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S17 = (&kernelContext_3)->joints_1+((&kernelContext_3)->skin_0->joint_base_0 + joint_0.z);

#line 479
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S18 = (&kernelContext_3)->joints_1+((&kernelContext_3)->skin_0->joint_base_0 + joint_0.w);

#line 484
    matrix<float,int(4),int(4)>  blended_0 = _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S14).data_0[int(0)][int(0)], (*_S14).data_0[int(1)][int(0)], (*_S14).data_0[int(2)][int(0)], (*_S14).data_0[int(3)][int(0)], (*_S14).data_0[int(0)][int(1)], (*_S14).data_0[int(1)][int(1)], (*_S14).data_0[int(2)][int(1)], (*_S14).data_0[int(3)][int(1)], (*_S14).data_0[int(0)][int(2)], (*_S14).data_0[int(1)][int(2)], (*_S14).data_0[int(2)][int(2)], (*_S14).data_0[int(3)][int(2)], (*_S14).data_0[int(0)][int(3)], (*_S14).data_0[int(1)][int(3)], (*_S14).data_0[int(2)][int(3)], (*_S14).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x, _S15.x)) + _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S16).data_0[int(0)][int(0)], (*_S16).data_0[int(1)][int(0)], (*_S16).data_0[int(2)][int(0)], (*_S16).data_0[int(3)][int(0)], (*_S16).data_0[int(0)][int(1)], (*_S16).data_0[int(1)][int(1)], (*_S16).data_0[int(2)][int(1)], (*_S16).data_0[int(3)][int(1)], (*_S16).data_0[int(0)][int(2)], (*_S16).data_0[int(1)][int(2)], (*_S16).data_0[int(2)][int(2)], (*_S16).data_0[int(3)][int(2)], (*_S16).data_0[int(0)][int(3)], (*_S16).data_0[int(1)][int(3)], (*_S16).data_0[int(2)][int(3)], (*_S16).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y, _S15.y)) + _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S17).data_0[int(0)][int(0)], (*_S17).data_0[int(1)][int(0)], (*_S17).data_0[int(2)][int(0)], (*_S17).data_0[int(3)][int(0)], (*_S17).data_0[int(0)][int(1)], (*_S17).data_0[int(1)][int(1)], (*_S17).data_0[int(2)][int(1)], (*_S17).data_0[int(3)][int(1)], (*_S17).data_0[int(0)][int(2)], (*_S17).data_0[int(1)][int(2)], (*_S17).data_0[int(2)][int(2)], (*_S17).data_0[int(3)][int(2)], (*_S17).data_0[int(0)][int(3)], (*_S17).data_0[int(1)][int(3)], (*_S17).data_0[int(2)][int(3)], (*_S17).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z, _S15.z)) + _slang_matrixCompMult(matrix<float,int(4),int(4)> ((*_S18).data_0[int(0)][int(0)], (*_S18).data_0[int(1)][int(0)], (*_S18).data_0[int(2)][int(0)], (*_S18).data_0[int(3)][int(0)], (*_S18).data_0[int(0)][int(1)], (*_S18).data_0[int(1)][int(1)], (*_S18).data_0[int(2)][int(1)], (*_S18).data_0[int(3)][int(1)], (*_S18).data_0[int(0)][int(2)], (*_S18).data_0[int(1)][int(2)], (*_S18).data_0[int(2)][int(2)], (*_S18).data_0[int(3)][int(2)], (*_S18).data_0[int(0)][int(3)], (*_S18).data_0[int(1)][int(3)], (*_S18).data_0[int(2)][int(3)], (*_S18).data_0[int(3)][int(3)]), matrix<float,int(4),int(4)> (_S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w, _S15.w));

    float3 position_1 = (((float4(_S13, 1.0f)) * (blended_0))).xyz;

#line 499
    matrix<float,int(3),int(3)>  _S19 = matrix<float,int(3),int(3)> (blended_0[int(0)].xyz, blended_0[int(1)].xyz, blended_0[int(2)].xyz);

#line 499
    float3 normal_1 = ((((&bind_basis_0)->normal_0) * (normal_basis_0(_S19))));

#line 505
    float3 tangent_1 = ((((&bind_basis_0)->tangent_0) * (_S19)));

#line 515
    float normal_length_0 = dot(normal_1, normal_1);
    thread TangentFrame_0 skinned_0;

#line 516
    float3 _S20;
    if(normal_length_0 > 0.0f)
    {

#line 517
        _S20 = normal_1 * float3(rsqrt(normal_length_0)) ;

#line 517
    }
    else
    {

#line 517
        _S20 = (&bind_basis_0)->normal_0;

#line 517
    }

#line 517
    (&skinned_0)->normal_0 = _S20;

#line 526
    float3 tangent_2 = tangent_1 - _S20 * float3(dot(_S20, tangent_1)) ;
    float tangent_length_0 = dot(tangent_2, tangent_2);
    if(tangent_length_0 > 0.0f)
    {

#line 528
        _S20 = tangent_2 * float3(rsqrt(tangent_length_0)) ;

#line 528
    }
    else
    {

#line 528
        _S20 = (&bind_basis_0)->tangent_0;

#line 528
    }

#line 528
    (&skinned_0)->tangent_0 = _S20;

#line 537
    float3 _S21 = cross((&skinned_0)->normal_0, _S20);

#line 537
    float _S22;

    if((dot(cross((&bind_basis_0)->normal_0, (&bind_basis_0)->tangent_0), (&bind_basis_0)->bitangent_0)) < 0.0f)
    {

#line 539
        _S22 = -1.0f;

#line 539
    }
    else
    {

#line 539
        _S22 = 1.0f;

#line 539
    }

#line 537
    (&skinned_0)->bitangent_0 = _S21 * float3(_S22) ;

#line 542
    uint _S23 = (&kernelContext_3)->skin_0->output_base_0 + index_0;

#line 542
    thread TangentFrame_0 _S24 = skinned_0;

#line 542
    store_vertex_0(_S23, position_1, &_S24, copied_2, &kernelContext_3);
    return;
}

