struct OctahedralParams_std140_0
{
    @align(16) probes_0 : u32,
    @align(4) probe_base_0 : u32,
    @align(8) extent_0 : u32,
    @align(4) face_texels_0 : u32,
    @align(16) atlas_columns_0 : u32,
    @align(4) row_floats_0 : u32,
    @align(8) layer_floats_0 : u32,
    @align(4) reserved_0 : u32,
};

@binding(0) @group(0) var<uniform> params_0 : OctahedralParams_std140_0;
@binding(1) @group(0) var<storage, read> directions_0 : array<vec4<f32>>;

struct _MatrixStorage_float4x4_ColMajorstd430_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

@binding(2) @group(0) var<storage, read> faces_0 : array<_MatrixStorage_float4x4_ColMajorstd430_0>;

@binding(3) @group(0) var distances_0 : texture_2d<f32>;

@binding(4) @group(0) var<storage, read_write> moments_0 : array<f32>;

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var texels_0 : u32 = params_0.extent_0 * params_0.extent_0;
    var index_0 : u32 = thread_0.x;
    if(index_0 >= (params_0.probes_0 * texels_0))
    {
        return;
    }
    var probe_0 : u32 = index_0 / texels_0;
    var texel_0 : u32 = index_0 - probe_0 * texels_0;
    var row_0 : u32 = texel_0 / params_0.extent_0;
    var column_0 : u32 = texel_0 - row_0 * params_0.extent_0;
    var entry_0 : vec4<f32> = directions_0[texel_0];
    var face_0 : u32 = u32(entry_0.w);
    var tile_0 : u32 = probe_0 * u32(6) + face_0;
    var clip_0 : vec4<f32> = (((vec4<f32>(entry_0.xyz, 0.0f)) * (mat4x4<f32>(faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(0)][i32(0)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(1)][i32(0)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(2)][i32(0)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(3)][i32(0)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(0)][i32(1)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(1)][i32(1)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(2)][i32(1)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(3)][i32(1)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(0)][i32(2)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(1)][i32(2)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(2)][i32(2)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(3)][i32(2)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(0)][i32(3)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(1)][i32(3)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(2)][i32(3)], faces_0[(params_0.probe_base_0 + probe_0) * u32(6) + face_0].data_0[i32(3)][i32(3)]))));
    var ndc_0 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var side_0 : f32 = f32(params_0.face_texels_0);
    var _S1 : f32 = side_0 - 1.0f;
    var inside_0 : vec2<f32> = clamp(vec2<f32>(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f) * vec2<f32>(side_0), vec2<f32>(0.0f, 0.0f), vec2<f32>(_S1, _S1));
    var _S2 : u32 = tile_0 % params_0.atlas_columns_0;
    var _S3 : u32 = _S2 * params_0.face_texels_0;
    var _S4 : u32 = tile_0 / params_0.atlas_columns_0;
    var _S5 : vec3<i32> = vec3<i32>(vec2<i32>(vec2<u32>(_S3, _S4 * params_0.face_texels_0) + vec2<u32>(inside_0)), i32(0));
    var reach_0 : f32 = (textureLoad((distances_0), ((_S5)).xy, ((_S5)).z).x);
    var at_0 : u32 = (params_0.probe_base_0 + probe_0) * params_0.layer_floats_0 + row_0 * params_0.row_floats_0 + column_0 * u32(2);
    moments_0[at_0] = reach_0;
    moments_0[at_0 + u32(1)] = reach_0 * reach_0;
    return;
}

