@binding(2) @group(0) var<storage, read_write> histogram_0 : array<atomic<u32>>;

@binding(4) @group(0) var<storage, read> previous_0 : array<f32>;

struct ExposureParams_std140_0
{
    @align(16) viewport_x_0 : u32,
    @align(4) viewport_y_0 : u32,
    @align(8) brighten_blend_0 : f32,
    @align(4) darken_blend_0 : f32,
};

@binding(0) @group(0) var<uniform> params_0 : ExposureParams_std140_0;
@binding(3) @group(0) var<storage, read_write> measured_0 : array<f32>;

@binding(1) @group(0) var scene_0 : texture_2d<f32>;

@compute
@workgroup_size(64, 1, 1)
fn clearMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var _S1 : u32 = thread_0.x;
    if(_S1 >= u32(96))
    {
        return;
    }
    atomicStore(&(histogram_0[_S1]), u32(0));
    return;
}

fn bin_luminance_0( bin_0 : u32) -> f32
{
    return (bitcast<f32>(((((u32(i32(-12) + i32(bin_0 / u32(4)) + i32(127)) << (u32(23)))) | ((((bin_0 % u32(4)) << (u32(21)))))))));
}

@compute
@workgroup_size(1, 1, 1)
fn reduceMain()
{
    var bin_1 : u32 = u32(1);
    var total_0 : u32 = u32(0);
    for(;;)
    {
        if(bin_1 < u32(96))
        {
        }
        else
        {
            break;
        }
        var _S2 : u32 = atomicLoad(&(histogram_0[bin_1]));
        var total_1 : u32 = total_0 + _S2;
        bin_1 = bin_1 + u32(1);
        total_0 = total_1;
    }
    var rate_0 : f32;
    var target_0 : f32;
    if(total_0 > u32(0))
    {
        var _S3 : f32 = f32(total_0);
        var _S4 : u32 = u32(_S3 * 0.5f);
        var _S5 : u32 = u32(_S3 * 0.94999998807907104f);
        bin_1 = u32(1);
        var seen_0 : u32 = u32(0);
        rate_0 = 0.0f;
        var population_0 : f32 = 0.0f;
        for(;;)
        {
            if(bin_1 < u32(96))
            {
            }
            else
            {
                break;
            }
            var _S6 : u32 = atomicLoad(&(histogram_0[bin_1]));
            var seen_1 : u32 = seen_0 + _S6;
            var _S7 : u32 = max(seen_0, _S4);
            var _S8 : u32 = min(seen_1, _S5);
            if(_S8 > _S7)
            {
                var part_0 : f32 = f32(_S8 - _S7);
                var population_1 : f32 = population_0 + part_0;
                rate_0 = rate_0 + part_0 * bin_luminance_0(bin_1) * 1.09050774574279785f;
                population_0 = population_1;
            }
            bin_1 = bin_1 + u32(1);
            seen_0 = seen_1;
        }
        if(population_0 > 0.0f)
        {
            target_0 = clamp(0.18000000715255737f / (rate_0 / population_0), 0.03125f, 32.0f);
        }
        else
        {
            target_0 = 1.0f;
        }
    }
    else
    {
        target_0 = 1.0f;
    }
    var prior_0 : f32 = previous_0[i32(0)];
    if(target_0 > prior_0)
    {
        rate_0 = params_0.brighten_blend_0;
    }
    else
    {
        rate_0 = params_0.darken_blend_0;
    }
    var blend_0 : f32 = clamp(rate_0, 0.0f, 1.0f);
    if(blend_0 >= 1.0f)
    {
        measured_0[i32(0)] = target_0;
    }
    else
    {
        if(blend_0 <= 0.0f)
        {
            measured_0[i32(0)] = prior_0;
        }
        else
        {
            measured_0[i32(0)] = clamp(prior_0 + (target_0 - prior_0) * blend_0, 0.03125f, 32.0f);
        }
    }
    return;
}

fn luma_0( color_0 : vec3<f32>) -> f32
{
    return dot(color_0, vec3<f32>(0.2125999927520752f, 0.71520000696182251f, 0.07220000028610229f));
}

fn bin_of_0( luminance_0 : f32) -> u32
{
    var bits_0 : u32 = (bitcast<u32>((luminance_0)));
    return u32(clamp((i32((bits_0 >> (u32(23)))) - i32(127) - i32(-12)) * i32(4) + i32((((bits_0 >> (u32(21)))) & (u32(3)))), i32(0), i32(95)));
}

@compute
@workgroup_size(64, 1, 1)
fn histogramMain(@builtin(global_invocation_id) thread_1 : vec3<u32>)
{
    var index_0 : u32 = thread_1.x;
    if(index_0 >= (params_0.viewport_x_0 * params_0.viewport_y_0))
    {
        return;
    }
    var _S9 : u32 = index_0 % params_0.viewport_x_0;
    var _S10 : u32 = index_0 / params_0.viewport_x_0;
    var _S11 : vec3<i32> = vec3<i32>(vec2<i32>(vec2<u32>(_S9, _S10)), i32(0));
    var _S12 : u32 = atomicAdd(&(histogram_0[bin_of_0(luma_0((textureLoad((scene_0), ((_S11)).xy, ((_S11)).z)).xyz))]), u32(1));
    return;
}

