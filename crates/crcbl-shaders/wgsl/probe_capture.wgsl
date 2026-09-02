@binding(1) @group(0) var<storage, read> positions_0 : array<f32>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct CaptureFace_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) origin_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> capture_0 : CaptureFace_std140_0;
struct CaptureVertex_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) world_0 : vec3<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) vertex_0 : u32) -> CaptureVertex_0
{
    var at_0 : u32 = vertex_0 * u32(3);
    var world_1 : vec3<f32> = vec3<f32>(positions_0[at_0], positions_0[at_0 + u32(1)], positions_0[at_0 + u32(2)]);
    var output_0 : CaptureVertex_0;
    output_0.world_0 = world_1;
    output_0.position_0 = (((vec4<f32>(world_1, 1.0f)) * (mat4x4<f32>(capture_0.view_proj_0.data_0[i32(0)][i32(0)], capture_0.view_proj_0.data_0[i32(1)][i32(0)], capture_0.view_proj_0.data_0[i32(2)][i32(0)], capture_0.view_proj_0.data_0[i32(3)][i32(0)], capture_0.view_proj_0.data_0[i32(0)][i32(1)], capture_0.view_proj_0.data_0[i32(1)][i32(1)], capture_0.view_proj_0.data_0[i32(2)][i32(1)], capture_0.view_proj_0.data_0[i32(3)][i32(1)], capture_0.view_proj_0.data_0[i32(0)][i32(2)], capture_0.view_proj_0.data_0[i32(1)][i32(2)], capture_0.view_proj_0.data_0[i32(2)][i32(2)], capture_0.view_proj_0.data_0[i32(3)][i32(2)], capture_0.view_proj_0.data_0[i32(0)][i32(3)], capture_0.view_proj_0.data_0[i32(1)][i32(3)], capture_0.view_proj_0.data_0[i32(2)][i32(3)], capture_0.view_proj_0.data_0[i32(3)][i32(3)]))));
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) world_2 : vec3<f32>,
};

@fragment
fn fragmentMain( _S1 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var _S2 : pixelOutput_0 = pixelOutput_0( vec4<f32>(length(_S1.world_2 - capture_0.origin_0.xyz), 0.0f, 0.0f, 0.0f) );
    return _S2;
}

