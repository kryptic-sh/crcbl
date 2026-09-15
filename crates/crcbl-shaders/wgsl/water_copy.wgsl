@binding(0) @group(0) var source_color_0 : texture_2d<f32>;

@binding(1) @group(0) var source_depth_0 : texture_depth_2d;

struct FullscreenOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> FullscreenOutput_0
{
    var output_0 : FullscreenOutput_0;
    output_0.position_0 = vec4<f32>(vec2<f32>(f32((((index_0 << (u32(1)))) & (u32(2)))), f32((index_0 & (u32(2))))) * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f), 0.0f, 1.0f);
    return output_0;
}

struct CopyOutput_0
{
    @location(0) color_0 : vec4<f32>,
    @builtin(frag_depth) depth_0 : f32,
};

@fragment
fn fragmentMain(@builtin(position) position_1 : vec4<f32>) -> CopyOutput_0
{
    var texel_0 : vec3<i32> = vec3<i32>(vec2<i32>(position_1.xy), i32(0));
    var output_1 : CopyOutput_0;
    output_1.color_0 = (textureLoad((source_color_0), ((texel_0)).xy, ((texel_0)).z));
    output_1.depth_0 = (textureLoad((source_depth_0), ((texel_0)).xy, ((texel_0)).z));
    return output_1;
}

