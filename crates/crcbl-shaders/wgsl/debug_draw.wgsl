struct DebugVertex_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
};

@binding(0) @group(0) var<storage, read> vertices_0 : array<DebugVertex_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct DebugConstants_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
};

@binding(1) @group(0) var<uniform> constants_0 : DebugConstants_std140_0;
struct DebugOutput_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) color_1 : vec4<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> DebugOutput_0
{
    var vertex_0 : DebugVertex_std430_0 = vertices_0[index_0];
    var output_0 : DebugOutput_0;
    output_0.position_1 = (((vec4<f32>(vertex_0.position_0.xyz, 1.0f)) * (mat4x4<f32>(constants_0.view_proj_0.data_0[i32(0)][i32(0)], constants_0.view_proj_0.data_0[i32(1)][i32(0)], constants_0.view_proj_0.data_0[i32(2)][i32(0)], constants_0.view_proj_0.data_0[i32(3)][i32(0)], constants_0.view_proj_0.data_0[i32(0)][i32(1)], constants_0.view_proj_0.data_0[i32(1)][i32(1)], constants_0.view_proj_0.data_0[i32(2)][i32(1)], constants_0.view_proj_0.data_0[i32(3)][i32(1)], constants_0.view_proj_0.data_0[i32(0)][i32(2)], constants_0.view_proj_0.data_0[i32(1)][i32(2)], constants_0.view_proj_0.data_0[i32(2)][i32(2)], constants_0.view_proj_0.data_0[i32(3)][i32(2)], constants_0.view_proj_0.data_0[i32(0)][i32(3)], constants_0.view_proj_0.data_0[i32(1)][i32(3)], constants_0.view_proj_0.data_0[i32(2)][i32(3)], constants_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    output_0.color_1 = vertex_0.color_0;
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) color_2 : vec4<f32>,
};

@fragment
fn fragmentMain( _S1 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var _S2 : pixelOutput_0 = pixelOutput_0( _S1.color_2 );
    return _S2;
}

