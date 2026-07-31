struct Vertex_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
};

@binding(0) @group(0) var<storage, read> vertices_0 : array<Vertex_std430_0>;

struct VertexOutput_0
{
    @builtin(position) position_1 : vec4<f32>,
    @location(0) color_1 : vec4<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> VertexOutput_0
{
    var vertex_0 : Vertex_std430_0 = vertices_0[index_0];
    var output_0 : VertexOutput_0;
    output_0.position_1 = vertex_0.position_0;
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

