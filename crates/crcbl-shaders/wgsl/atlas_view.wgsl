struct AtlasViewParams_std140_0
{
    @align(16) view_0 : vec4<f32>,
    @align(16) atlas_0 : vec4<f32>,
    @align(16) rect_0 : array<vec4<f32>, i32(16)>,
};

@binding(0) @group(0) var<uniform> params_0 : AtlasViewParams_std140_0;
@binding(1) @group(0) var shadow_atlas_0 : texture_depth_2d;

struct FullscreenOutput_0
{
    @builtin(position) position_0 : vec4<f32>,
    @location(0) uv_0 : vec2<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32) -> FullscreenOutput_0
{
    var output_0 : FullscreenOutput_0;
    var _S1 : vec2<f32> = vec2<f32>(f32((((index_0 << (u32(1)))) & (u32(2)))), f32((index_0 & (u32(2)))));
    output_0.uv_0 = _S1;
    output_0.position_0 = vec4<f32>(_S1 * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f), 0.0f, 1.0f);
    return output_0;
}

struct pixelOutput_0
{
    @location(0) output_1 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) uv_1 : vec2<f32>,
};

@fragment
fn fragmentMain( _S2 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var inside_0 : vec2<f32> = position_1.xy - params_0.view_0.xy;
    var _S3 : bool;
    if((any((inside_0 < vec2<f32>(0.0f, 0.0f)))))
    {
        _S3 = true;
    }
    else
    {
        _S3 = (any((inside_0 >= (params_0.view_0.zw))));
    }
    if(_S3)
    {
        var _S4 : pixelOutput_0 = pixelOutput_0( vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f) );
        return _S4;
    }
    var slot_0 : u32 = u32(0);
    for(;;)
    {
        if(slot_0 < u32(16))
        {
        }
        else
        {
            break;
        }
        var rect_1 : vec4<f32> = params_0.rect_0[slot_0];
        if((params_0.rect_0[slot_0].x) <= 0.0f)
        {
            _S3 = true;
        }
        else
        {
            _S3 = (rect_1.y) <= 0.0f;
        }
        if(_S3)
        {
            slot_0 = slot_0 + u32(1);
            continue;
        }
        var tile_min_0 : vec2<f32> = rect_1.zw * params_0.view_0.zw;
        var tile_max_0 : vec2<f32> = tile_min_0 + rect_1.xy * params_0.view_0.zw;
        var _S5 : bool;
        if((any((inside_0 < tile_min_0))))
        {
            _S5 = true;
        }
        else
        {
            _S5 = (any((inside_0 >= tile_max_0)));
        }
        if(_S5)
        {
            slot_0 = slot_0 + u32(1);
            continue;
        }
        var edge_0 : vec2<f32> = min(inside_0 - tile_min_0, tile_max_0 - inside_0);
        if((min(edge_0.x, edge_0.y)) < 2.0f)
        {
            var _S6 : vec3<f32> = vec3<f32>(1.0f, 0.55000001192092896f, 0.15000000596046448f);
            var _S7 : pixelOutput_0 = pixelOutput_0( vec4<f32>(_S6, 1.0f) );
            return _S7;
        }
        slot_0 = slot_0 + u32(1);
    }
    var extent_0 : vec2<f32> = params_0.atlas_0.xy;
    var _S8 : vec3<i32> = vec3<i32>(vec2<i32>(min(inside_0 / params_0.view_0.zw * extent_0, extent_0 - vec2<f32>(1.0f, 1.0f))), i32(0));
    var depth_0 : f32 = (textureLoad((shadow_atlas_0), ((_S8)).xy, ((_S8)).z));
    var grey_0 : f32;
    if(depth_0 > 0.0f)
    {
        grey_0 = mix(0.30000001192092896f, 1.0f, depth_0);
    }
    else
    {
        grey_0 = 0.05999999865889549f;
    }
    var _S9 : pixelOutput_0 = pixelOutput_0( vec4<f32>(grey_0, grey_0, grey_0, 1.0f) );
    return _S9;
}

