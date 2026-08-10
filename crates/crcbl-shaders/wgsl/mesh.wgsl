struct DrawConstants_std140_0
{
    @align(16) base_0 : u32,
    @align(4) pad0_0 : u32,
    @align(8) pad1_0 : u32,
    @align(4) pad2_0 : u32,
};

@binding(3) @group(0) var<uniform> draw_0 : DrawConstants_std140_0;
@binding(5) @group(0) var<storage, read> visible_instances_0 : array<u32>;

struct _MatrixStorage_float4x4_ColMajorstd430_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct GpuInstance_std430_0
{
    @align(16) transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) mesh_0 : u32,
    @align(4) material_0 : u32,
    @align(8) sector_0 : u32,
    @align(4) flags_0 : u32,
};

@binding(2) @group(0) var<storage, read> instances_0 : array<GpuInstance_std430_0>;

struct GpuMesh_std430_0
{
    @align(4) base_vertex_0 : u32,
    @align(4) base_index_0 : u32,
    @align(4) index_count_0 : u32,
    @align(4) min_x_0 : f32,
    @align(4) min_y_0 : f32,
    @align(4) min_z_0 : f32,
    @align(4) max_x_0 : f32,
    @align(4) max_y_0 : f32,
    @align(4) max_z_0 : f32,
};

@binding(4) @group(0) var<storage, read> meshes_0 : array<GpuMesh_std430_0>;

struct MeshVertex_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) normal_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
};

@binding(1) @group(0) var<storage, read> vertices_0 : array<MeshVertex_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_1 : array<vec4<f32>, i32(4)>,
};

struct FrameUniforms_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) light_direction_0 : vec4<f32>,
    @align(16) light_color_0 : vec4<f32>,
    @align(16) ambient_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
struct GpuMaterial_std430_0
{
    @align(16) base_color_0 : vec4<f32>,
};

@binding(6) @group(0) var<storage, read> materials_0 : array<GpuMaterial_std430_0>;

struct VertexOutput_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @location(1) world_normal_0 : vec3<f32>,
    @location(2) color_1 : vec4<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32, @builtin(instance_index) instance_id_0 : u32) -> VertexOutput_0
{
    var instance_0 : GpuInstance_std430_0 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]];
    var vertex_0 : MeshVertex_std430_0 = vertices_0[index_0 + meshes_0[instance_0.mesh_0].base_vertex_0];
    var _S1 : mat4x4<f32> = mat4x4<f32>(instance_0.transform_0.data_0[i32(0)][i32(0)], instance_0.transform_0.data_0[i32(1)][i32(0)], instance_0.transform_0.data_0[i32(2)][i32(0)], instance_0.transform_0.data_0[i32(3)][i32(0)], instance_0.transform_0.data_0[i32(0)][i32(1)], instance_0.transform_0.data_0[i32(1)][i32(1)], instance_0.transform_0.data_0[i32(2)][i32(1)], instance_0.transform_0.data_0[i32(3)][i32(1)], instance_0.transform_0.data_0[i32(0)][i32(2)], instance_0.transform_0.data_0[i32(1)][i32(2)], instance_0.transform_0.data_0[i32(2)][i32(2)], instance_0.transform_0.data_0[i32(3)][i32(2)], instance_0.transform_0.data_0[i32(0)][i32(3)], instance_0.transform_0.data_0[i32(1)][i32(3)], instance_0.transform_0.data_0[i32(2)][i32(3)], instance_0.transform_0.data_0[i32(3)][i32(3)]);
    var world_0 : vec4<f32> = (((vec4<f32>(vertex_0.position_0.xyz, 1.0f)) * (_S1)));
    var output_0 : VertexOutput_0;
    output_0.position_1 = (((world_0) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)]))));
    output_0.world_position_0 = world_0.xyz;
    output_0.world_normal_0 = (((vertex_0.normal_0.xyz) * (mat3x3<f32>(_S1[i32(0)].xyz, _S1[i32(1)].xyz, _S1[i32(2)].xyz))));
    output_0.color_1 = vertex_0.color_0 * materials_0[instance_0.material_0].base_color_0;
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_1 : vec3<f32>,
    @location(1) world_normal_1 : vec3<f32>,
    @location(2) color_2 : vec4<f32>,
};

@fragment
fn fragmentMain( _S2 : pixelInput_0, @builtin(position) position_2 : vec4<f32>) -> pixelOutput_0
{
    var normal_1 : vec3<f32> = normalize(_S2.world_normal_1);
    var to_light_0 : vec3<f32> = normalize(frame_0.light_direction_0.xyz);
    var _S3 : f32 = max(dot(normal_1, to_light_0), 0.0f);
    var _S4 : pixelOutput_0 = pixelOutput_0( vec4<f32>(_S2.color_2.xyz * (frame_0.ambient_0.xyz + frame_0.light_color_0.xyz * vec3<f32>(_S3)) + frame_0.light_color_0.xyz * vec3<f32>((pow(max(dot(normal_1, normalize(to_light_0 + normalize(frame_0.camera_position_0.xyz - _S2.world_position_1))), 0.0f), 32.0f) * (step(0.0f, _S3) * _S3) * 0.34999999403953552f)), _S2.color_2.w) );
    return _S4;
}

