struct SkinParams_std140_0
{
    @align(16) vertex_count_0 : u32,
    @align(4) input_base_0 : u32,
    @align(8) output_base_0 : u32,
    @align(4) binding_base_0 : u32,
    @align(16) joint_base_0 : u32,
    @align(4) joint_count_0 : u32,
    @align(8) attribute_base_0 : u32,
};

@binding(0) @group(0) var<uniform> skin_0 : SkinParams_std140_0;
@binding(3) @group(0) var<storage, read_write> vertices_0 : array<u32>;

struct SkinBinding_std430_0
{
    @align(16) joints_0 : vec4<u32>,
    @align(16) weights_0 : vec4<f32>,
};

@binding(2) @group(0) var<storage, read> bindings_0 : array<SkinBinding_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd430_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

@binding(1) @group(0) var<storage, read> joints_1 : array<_MatrixStorage_float4x4_ColMajorstd430_0>;

fn rsqrt_0( x_0 : f32) -> f32
{
    return 1.0f / sqrt(x_0);
}

fn load_position_0( at_0 : u32) -> vec3<f32>
{
    var word_0 : u32 = at_0 * u32(3);
    return vec3<f32>((bitcast<f32>((vertices_0[word_0]))), (bitcast<f32>((vertices_0[word_0 + u32(1)]))), (bitcast<f32>((vertices_0[word_0 + u32(2)]))));
}

fn dequantise_snorm_0( lane_0 : i32) -> f32
{
    return max(f32(lane_0) / 32767.0f, -1.0f);
}

fn unpack_snorm16x4_0( low_0 : u32,  high_0 : u32) -> vec4<f32>
{
    return vec4<f32>(dequantise_snorm_0(((bitcast<i32>(((low_0 << (u32(16)))))) >> (u32(16)))), dequantise_snorm_0(((bitcast<i32>((low_0))) >> (u32(16)))), dequantise_snorm_0(((bitcast<i32>(((high_0 << (u32(16)))))) >> (u32(16)))), dequantise_snorm_0(((bitcast<i32>((high_0))) >> (u32(16)))));
}

fn rotate_by_0( q_0 : vec4<f32>,  v_0 : vec3<f32>) -> vec3<f32>
{
    var _S1 : vec3<f32> = q_0.xyz;
    var t_0 : vec3<f32> = vec3<f32>(2.0f) * cross(_S1, v_0);
    return v_0 + vec3<f32>(q_0.w) * t_0 + cross(_S1, t_0);
}

struct TangentFrame_0
{
     tangent_0 : vec3<f32>,
     bitangent_0 : vec3<f32>,
     normal_0 : vec3<f32>,
};

fn decode_qtangent_0( lanes_0 : vec4<f32>) -> TangentFrame_0
{
    var q_1 : vec4<f32> = normalize(lanes_0);
    var basis_0 : TangentFrame_0;
    var _S2 : vec3<f32> = rotate_by_0(q_1, vec3<f32>(1.0f, 0.0f, 0.0f));
    basis_0.tangent_0 = _S2;
    var _S3 : vec3<f32> = rotate_by_0(q_1, vec3<f32>(0.0f, 0.0f, 1.0f));
    basis_0.normal_0 = _S3;
    var _S4 : vec3<f32> = cross(_S3, _S2);
    var _S5 : f32;
    if((lanes_0.w) < 0.0f)
    {
        _S5 = -1.0f;
    }
    else
    {
        _S5 = 1.0f;
    }
    basis_0.bitangent_0 = _S4 * vec3<f32>(_S5);
    return basis_0;
}

fn load_attributes_0( at_1 : u32,  basis_1 : ptr<function, TangentFrame_0>,  copied_0 : ptr<function, vec3<u32>>)
{
    var word_1 : u32 = skin_0.attribute_base_0 + at_1 * u32(5);
    (*basis_1) = decode_qtangent_0(unpack_snorm16x4_0(vertices_0[word_1], vertices_0[word_1 + u32(1)]));
    (*copied_0) = vec3<u32>(vertices_0[word_1 + u32(2)], vertices_0[word_1 + u32(3)], vertices_0[word_1 + u32(4)]);
    return;
}

fn normal_basis_0( basis_2 : mat3x3<f32>) -> mat3x3<f32>
{
    return mat3x3<f32>(cross(basis_2[i32(1)], basis_2[i32(2)]), cross(basis_2[i32(2)], basis_2[i32(0)]), cross(basis_2[i32(0)], basis_2[i32(1)]));
}

fn quaternion_from_columns_0( x_1 : vec3<f32>,  y_0 : vec3<f32>,  z_0 : vec3<f32>) -> vec4<f32>
{
    var m00_0 : f32 = x_1.x;
    var m01_0 : f32 = y_0.x;
    var m02_0 : f32 = z_0.x;
    var m10_0 : f32 = x_1.y;
    var m11_0 : f32 = y_0.y;
    var m12_0 : f32 = z_0.y;
    var m20_0 : f32 = x_1.z;
    var m21_0 : f32 = y_0.z;
    var m22_0 : f32 = z_0.z;
    var trace_0 : f32 = m00_0 + m11_0 + m22_0;
    if(trace_0 > 0.0f)
    {
        var s_0 : f32 = sqrt(trace_0 + 1.0f) * 2.0f;
        return vec4<f32>((m21_0 - m12_0) / s_0, (m02_0 - m20_0) / s_0, (m10_0 - m01_0) / s_0, 0.25f * s_0);
    }
    var _S6 : bool;
    if(m00_0 > m11_0)
    {
        _S6 = m00_0 > m22_0;
    }
    else
    {
        _S6 = false;
    }
    if(_S6)
    {
        var s_1 : f32 = sqrt(1.0f + m00_0 - m11_0 - m22_0) * 2.0f;
        return vec4<f32>(0.25f * s_1, (m01_0 + m10_0) / s_1, (m02_0 + m20_0) / s_1, (m21_0 - m12_0) / s_1);
    }
    if(m11_0 > m22_0)
    {
        var s_2 : f32 = sqrt(1.0f + m11_0 - m00_0 - m22_0) * 2.0f;
        return vec4<f32>((m01_0 + m10_0) / s_2, 0.25f * s_2, (m12_0 + m21_0) / s_2, (m02_0 - m20_0) / s_2);
    }
    var s_3 : f32 = sqrt(1.0f + m22_0 - m00_0 - m11_0) * 2.0f;
    return vec4<f32>((m02_0 + m20_0) / s_3, (m12_0 + m21_0) / s_3, 0.25f * s_3, (m10_0 - m01_0) / s_3);
}

fn quantise_snorm_0( value_0 : f32) -> i32
{
    var scaled_0 : f32 = clamp(value_0, -1.0f, 1.0f) * 32767.0f;
    var _S7 : f32;
    if(scaled_0 < 0.0f)
    {
        _S7 = - floor(0.5f - scaled_0);
    }
    else
    {
        _S7 = floor(scaled_0 + 0.5f);
    }
    return i32(_S7);
}

fn pack_snorm16x2_0( low_1 : f32,  high_1 : f32) -> u32
{
    return (((u32(quantise_snorm_0(low_1)) & (u32(65535)))) | (((u32(quantise_snorm_0(high_1)) << (u32(16))))));
}

fn encode_qtangent_0( basis_3 : TangentFrame_0) -> vec2<u32>
{
    var handedness_0 : f32;
    if((dot(cross(basis_3.normal_0, basis_3.tangent_0), basis_3.bitangent_0)) < 0.0f)
    {
        handedness_0 = -1.0f;
    }
    else
    {
        handedness_0 = 1.0f;
    }
    var q_2 : vec4<f32> = normalize(quaternion_from_columns_0(basis_3.tangent_0, basis_3.bitangent_0 * vec3<f32>(handedness_0), basis_3.normal_0));
    var q_3 : vec4<f32>;
    if((q_2.w) < 0.0f)
    {
        q_3 = (vec4<f32>(0) - q_2);
    }
    else
    {
        q_3 = q_2;
    }
    var _S8 : f32 = q_3.w;
    if(_S8 < 0.00003051850944757f)
    {
        q_3 = vec4<f32>(q_3.xyz * vec3<f32>((sqrt(1.0f) / sqrt(1.0f - _S8 * _S8))), 0.00003051850944757f);
    }
    else
    {
    }
    if(handedness_0 < 0.0f)
    {
        q_3 = (vec4<f32>(0) - q_3);
    }
    else
    {
    }
    return vec2<u32>(pack_snorm16x2_0(q_3.x, q_3.y), pack_snorm16x2_0(q_3.z, q_3.w));
}

fn store_vertex_0( at_2 : u32,  position_0 : vec3<f32>,  basis_4 : TangentFrame_0,  copied_1 : vec3<u32>)
{
    var word_2 : u32 = at_2 * u32(3);
    vertices_0[word_2] = (bitcast<u32>((position_0.x)));
    vertices_0[word_2 + u32(1)] = (bitcast<u32>((position_0.y)));
    vertices_0[word_2 + u32(2)] = (bitcast<u32>((position_0.z)));
    var qtangent_0 : vec2<u32> = encode_qtangent_0(basis_4);
    var word_3 : u32 = skin_0.attribute_base_0 + at_2 * u32(5);
    vertices_0[word_3] = qtangent_0.x;
    vertices_0[word_3 + u32(1)] = qtangent_0.y;
    vertices_0[word_3 + u32(2)] = copied_1.x;
    vertices_0[word_3 + u32(3)] = copied_1.y;
    vertices_0[word_3 + u32(4)] = copied_1.z;
    return;
}

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 >= (skin_0.vertex_count_0))
    {
        return;
    }
    var bind_position_0 : vec3<f32> = load_position_0(skin_0.input_base_0 + index_0);
    var bind_basis_0 : TangentFrame_0;
    var copied_2 : vec3<u32>;
    load_attributes_0(skin_0.input_base_0 + index_0, &(bind_basis_0), &(copied_2));
    var binding_0 : SkinBinding_std430_0 = bindings_0[skin_0.binding_base_0 + index_0];
    var last_0 : u32 = skin_0.joint_count_0 - u32(1);
    var joint_0 : vec4<u32> = min(binding_0.joints_0, vec4<u32>(last_0, last_0, last_0, last_0));
    var _S9 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(3)]);
    var _S10 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x);
    var _S11 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(3)]);
    var _S12 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y);
    var _S13 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(3)]);
    var _S14 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z);
    var _S15 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(3)]);
    var _S16 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w);
    var blended_0 : mat4x4<f32> = mat4x4<f32>(_S9[0] * _S10[0], _S9[1] * _S10[1], _S9[2] * _S10[2], _S9[3] * _S10[3]) + mat4x4<f32>(_S11[0] * _S12[0], _S11[1] * _S12[1], _S11[2] * _S12[2], _S11[3] * _S12[3]) + mat4x4<f32>(_S13[0] * _S14[0], _S13[1] * _S14[1], _S13[2] * _S14[2], _S13[3] * _S14[3]) + mat4x4<f32>(_S15[0] * _S16[0], _S15[1] * _S16[1], _S15[2] * _S16[2], _S15[3] * _S16[3]);
    var position_1 : vec3<f32> = (((vec4<f32>(bind_position_0, 1.0f)) * (blended_0))).xyz;
    var _S17 : mat3x3<f32> = mat3x3<f32>(blended_0[i32(0)].xyz, blended_0[i32(1)].xyz, blended_0[i32(2)].xyz);
    var normal_1 : vec3<f32> = (((bind_basis_0.normal_0) * (normal_basis_0(_S17))));
    var tangent_1 : vec3<f32> = (((bind_basis_0.tangent_0) * (_S17)));
    var normal_length_0 : f32 = dot(normal_1, normal_1);
    var skinned_0 : TangentFrame_0;
    var _S18 : vec3<f32>;
    if(normal_length_0 > 0.0f)
    {
        _S18 = normal_1 * vec3<f32>(rsqrt_0(normal_length_0));
    }
    else
    {
        _S18 = bind_basis_0.normal_0;
    }
    skinned_0.normal_0 = _S18;
    var tangent_2 : vec3<f32> = tangent_1 - _S18 * vec3<f32>(dot(_S18, tangent_1));
    var tangent_length_0 : f32 = dot(tangent_2, tangent_2);
    if(tangent_length_0 > 0.0f)
    {
        _S18 = tangent_2 * vec3<f32>(rsqrt_0(tangent_length_0));
    }
    else
    {
        _S18 = bind_basis_0.tangent_0;
    }
    skinned_0.tangent_0 = _S18;
    var _S19 : vec3<f32> = cross(skinned_0.normal_0, _S18);
    var _S20 : f32;
    if((dot(cross(bind_basis_0.normal_0, bind_basis_0.tangent_0), bind_basis_0.bitangent_0)) < 0.0f)
    {
        _S20 = -1.0f;
    }
    else
    {
        _S20 = 1.0f;
    }
    skinned_0.bitangent_0 = _S19 * vec3<f32>(_S20);
    store_vertex_0(skin_0.output_base_0 + index_0, position_1, skinned_0, copied_2);
    return;
}

