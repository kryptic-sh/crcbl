struct SkinParams_std140_0
{
    @align(16) vertex_count_0 : u32,
    @align(4) input_base_0 : u32,
    @align(8) output_base_0 : u32,
    @align(4) binding_base_0 : u32,
    @align(16) joint_base_0 : u32,
    @align(4) joint_count_0 : u32,
};

@binding(0) @group(0) var<uniform> skin_0 : SkinParams_std140_0;
struct MeshVertex_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) normal_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) uv_0 : vec4<f32>,
};

@binding(3) @group(0) var<storage, read_write> vertices_0 : array<MeshVertex_std430_0>;

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

fn normal_basis_0( basis_0 : mat3x3<f32>) -> mat3x3<f32>
{
    return mat3x3<f32>(cross(basis_0[i32(1)], basis_0[i32(2)]), cross(basis_0[i32(2)], basis_0[i32(0)]), cross(basis_0[i32(0)], basis_0[i32(1)]));
}

struct MeshVertex_0
{
     position_0 : vec4<f32>,
     normal_0 : vec4<f32>,
     color_0 : vec4<f32>,
     uv_0 : vec4<f32>,
};

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 >= (skin_0.vertex_count_0))
    {
        return;
    }
    var vertex_0 : MeshVertex_std430_0 = vertices_0[skin_0.input_base_0 + index_0];
    var binding_0 : SkinBinding_std430_0 = bindings_0[skin_0.binding_base_0 + index_0];
    var last_0 : u32 = skin_0.joint_count_0 - u32(1);
    var joint_0 : vec4<u32> = min(binding_0.joints_0, vec4<u32>(last_0, last_0, last_0, last_0));
    var _S1 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.x].data_0[i32(3)][i32(3)]);
    var _S2 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x, binding_0.weights_0.x);
    var _S3 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.y].data_0[i32(3)][i32(3)]);
    var _S4 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y, binding_0.weights_0.y);
    var _S5 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.z].data_0[i32(3)][i32(3)]);
    var _S6 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z, binding_0.weights_0.z);
    var _S7 : mat4x4<f32> = mat4x4<f32>(joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(0)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(1)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(2)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(0)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(1)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(2)][i32(3)], joints_1[skin_0.joint_base_0 + joint_0.w].data_0[i32(3)][i32(3)]);
    var _S8 : mat4x4<f32> = mat4x4<f32>(binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w, binding_0.weights_0.w);
    var blended_0 : mat4x4<f32> = mat4x4<f32>(_S1[0] * _S2[0], _S1[1] * _S2[1], _S1[2] * _S2[2], _S1[3] * _S2[3]) + mat4x4<f32>(_S3[0] * _S4[0], _S3[1] * _S4[1], _S3[2] * _S4[2], _S3[3] * _S4[3]) + mat4x4<f32>(_S5[0] * _S6[0], _S5[1] * _S6[1], _S5[2] * _S6[2], _S5[3] * _S6[3]) + mat4x4<f32>(_S7[0] * _S8[0], _S7[1] * _S8[1], _S7[2] * _S8[2], _S7[3] * _S8[3]);
    var _S9 : vec3<f32> = vertices_0[skin_0.input_base_0 + index_0].normal_0.xyz;
    var normal_1 : vec3<f32> = (((_S9) * (normal_basis_0(mat3x3<f32>(blended_0[i32(0)].xyz, blended_0[i32(1)].xyz, blended_0[i32(2)].xyz)))));
    var skinned_0 : MeshVertex_0;
    skinned_0.position_0 = vec4<f32>((((vec4<f32>(vertices_0[skin_0.input_base_0 + index_0].position_0.xyz, 1.0f)) * (blended_0))).xyz, 1.0f);
    var square_length_0 : f32 = dot(normal_1, normal_1);
    var _S10 : vec3<f32>;
    if(square_length_0 > 0.0f)
    {
        _S10 = normal_1 * vec3<f32>(rsqrt_0(square_length_0));
    }
    else
    {
        _S10 = _S9;
    }
    skinned_0.normal_0 = vec4<f32>(_S10, 0.0f);
    skinned_0.color_0 = vertex_0.color_0;
    skinned_0.uv_0 = vertex_0.uv_0;
    vertices_0[skin_0.output_base_0 + index_0].position_0 = skinned_0.position_0;
    vertices_0[skin_0.output_base_0 + index_0].normal_0 = skinned_0.normal_0;
    vertices_0[skin_0.output_base_0 + index_0].color_0 = skinned_0.color_0;
    vertices_0[skin_0.output_base_0 + index_0].uv_0 = skinned_0.uv_0;
    return;
}

