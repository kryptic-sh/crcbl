struct UpscaleParams_std140_0
{
    @align(16) source_extent_0 : vec2<f32>,
    @align(8) inv_source_0 : vec2<f32>,
};

@binding(2) @group(0) var<uniform> params_0 : UpscaleParams_std140_0;
@binding(0) @group(0) var source_0 : texture_2d<f32>;

@binding(1) @group(0) var sourceSampler_0 : sampler;

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

fn catmull_rom_weights_0( f_0 : f32) -> vec4<f32>
{
    var f2_0 : f32 = f_0 * f_0;
    var f3_0 : f32 = f2_0 * f_0;
    var _S2 : f32 = 0.5f * f_0;
    return vec4<f32>(-0.5f * f3_0 + f2_0 - _S2, 1.5f * f3_0 - 2.5f * f2_0 + 1.0f, -1.5f * f3_0 + 2.0f * f2_0 + _S2, 0.5f * f3_0 - 0.5f * f2_0);
}

fn merged_pair_offset_0( w_0 : vec4<f32>) -> f32
{
    var _S3 : f32 = w_0.z;
    return _S3 / (w_0.y + _S3);
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
fn fragmentMain( _S4 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var pos_0 : vec2<f32> = _S4.uv_1 * params_0.source_extent_0 - vec2<f32>(0.5f);
    var base_0 : vec2<f32> = floor(pos_0);
    var f_1 : vec2<f32> = pos_0 - base_0;
    var wx_0 : vec4<f32> = catmull_rom_weights_0(f_1.x);
    var wy_0 : vec4<f32> = catmull_rom_weights_0(f_1.y);
    var _S5 : vec3<f32> = vec3<f32>(wx_0.x, wx_0.y + wx_0.z, wx_0.w);
    var _S6 : vec3<f32> = vec3<f32>(wy_0.x, wy_0.y + wy_0.z, wy_0.w);
    var _S7 : vec3<f32> = vec3<f32>(0.5f);
    var _S8 : vec3<f32> = (vec3<f32>(base_0.x) + vec3<f32>(-1.0f, merged_pair_offset_0(wx_0), 2.0f) + _S7) * vec3<f32>(params_0.inv_source_0.x);
    var _S9 : vec3<f32> = (vec3<f32>(base_0.y) + vec3<f32>(-1.0f, merged_pair_offset_0(wy_0), 2.0f) + _S7) * vec3<f32>(params_0.inv_source_0.y);
    const _S10 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var j_0 : i32 = i32(0);
    var sum_0 : vec3<f32> = _S10;
    for(;;)
    {
        if(j_0 < i32(3))
        {
        }
        else
        {
            break;
        }
        var i_0 : i32 = i32(0);
        for(;;)
        {
            if(i_0 < i32(3))
            {
            }
            else
            {
                break;
            }
            var sum_1 : vec3<f32> = sum_0 + (textureSampleLevel((source_0), (sourceSampler_0), (vec2<f32>(_S8[i_0], _S9[j_0])), (0.0f))).xyz * vec3<f32>((_S5[i_0] * _S6[j_0]));
            i_0 = i_0 + i32(1);
            sum_0 = sum_1;
        }
        j_0 = j_0 + i32(1);
    }
    var _S11 : pixelOutput_0 = pixelOutput_0( vec4<f32>(saturate(sum_0), 1.0f) );
    return _S11;
}

