@binding(1) @group(0) var scene_depth_0 : texture_depth_2d;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_0 : array<vec4<f32>, i32(4)>,
};

struct SsaoParams_std140_0
{
    @align(16) inv_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) inv_view_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) params_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> camera_0 : SsaoParams_std140_0;
var<private> STEP_OFFSETS_0 : array<f32, i32(16)> = array<f32, i32(16)>( 0.0625f, 0.5625f, 0.1875f, 0.6875f, 0.8125f, 0.3125f, 0.9375f, 0.4375f, 0.25f, 0.75f, 0.125f, 0.625f, 1.0f, 0.5f, 0.875f, 0.375f );
var<private> SLICE_DIRECTIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(2.0f, 0.0f), vec2<f32>(-2.0f, 0.0f), vec2<f32>(1.0f, 1.0f), vec2<f32>(-1.0f, -1.0f), vec2<f32>(0.0f, -2.0f), vec2<f32>(0.0f, 2.0f), vec2<f32>(1.0f, -1.0f), vec2<f32>(-1.0f, 1.0f), vec2<f32>(1.0f, 2.0f), vec2<f32>(-1.0f, -2.0f), vec2<f32>(2.0f, 1.0f), vec2<f32>(-2.0f, -1.0f), vec2<f32>(2.0f, -1.0f), vec2<f32>(-2.0f, 1.0f), vec2<f32>(1.0f, -2.0f), vec2<f32>(-1.0f, 2.0f) );
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

fn full_res_pixel_0( pixel_0 : vec2<i32>) -> vec2<i32>
{
    return pixel_0 * vec2<i32>(i32(2));
}

fn depth_at_0( pixel_1 : vec2<i32>,  extent_0 : vec2<i32>) -> f32
{
    var _S2 : vec3<i32> = vec3<i32>(clamp(pixel_1, vec2<i32>(i32(0), i32(0)), extent_0 - vec2<i32>(i32(1), i32(1))), i32(0));
    return (textureLoad((scene_depth_0), ((_S2)).xy, ((_S2)).z));
}

fn encode_bent_0( direction_0 : vec3<f32>) -> vec3<f32>
{
    var _S3 : vec3<f32> = vec3<f32>(0.5f);
    return direction_0 * _S3 + _S3;
}

fn unproject_z_0( depth_0 : f32) -> vec2<f32>
{
    return vec2<f32>(camera_0.inv_proj_0.data_0[i32(2)][i32(2)] * depth_0 + camera_0.inv_proj_0.data_0[i32(3)][i32(2)], camera_0.inv_proj_0.data_0[i32(2)][i32(3)] * depth_0 + camera_0.inv_proj_0.data_0[i32(3)][i32(3)]);
}

fn unproject_0( ndc_0 : vec2<f32>,  depth_1 : f32) -> vec4<f32>
{
    var depth_row_0 : vec2<f32> = unproject_z_0(depth_1);
    return vec4<f32>(camera_0.inv_proj_0.data_0[i32(0)][i32(0)] * ndc_0.x + camera_0.inv_proj_0.data_0[i32(3)][i32(0)], camera_0.inv_proj_0.data_0[i32(1)][i32(1)] * ndc_0.y + camera_0.inv_proj_0.data_0[i32(3)][i32(1)], depth_row_0.x, depth_row_0.y);
}

fn view_position_0( pixel_2 : vec2<i32>,  depth_2 : f32,  extent_1 : vec2<f32>) -> vec3<f32>
{
    var view_0 : vec4<f32> = unproject_0(vec2<f32>((f32(pixel_2.x) + 0.5f) / extent_1.x * 2.0f - 1.0f, 1.0f - (f32(pixel_2.y) + 0.5f) / extent_1.y * 2.0f), depth_2);
    return view_0.xyz / vec3<f32>(view_0.w);
}

fn normal_at_0( pixel_3 : vec2<i32>,  centre_0 : vec3<f32>,  extent_2 : vec2<i32>,  size_0 : vec2<f32>) -> vec3<f32>
{
    var _S4 : vec2<i32> = pixel_3 + vec2<i32>(i32(-1), i32(0));
    var left_0 : vec3<f32> = view_position_0(_S4, depth_at_0(_S4, extent_2), size_0);
    var _S5 : vec2<i32> = pixel_3 + vec2<i32>(i32(1), i32(0));
    var right_0 : vec3<f32> = view_position_0(_S5, depth_at_0(_S5, extent_2), size_0);
    var _S6 : vec2<i32> = pixel_3 + vec2<i32>(i32(0), i32(-1));
    var up_0 : vec3<f32> = view_position_0(_S6, depth_at_0(_S6, extent_2), size_0);
    var _S7 : vec2<i32> = pixel_3 + vec2<i32>(i32(0), i32(1));
    var down_0 : vec3<f32> = view_position_0(_S7, depth_at_0(_S7, extent_2), size_0);
    var _S8 : f32 = centre_0.z;
    var horizontal_0 : vec3<f32>;
    if((abs(right_0.z - _S8)) < (abs(_S8 - left_0.z)))
    {
        horizontal_0 = right_0 - centre_0;
    }
    else
    {
        horizontal_0 = centre_0 - left_0;
    }
    var vertical_0 : vec3<f32>;
    if((abs(down_0.z - _S8)) < (abs(_S8 - up_0.z)))
    {
        vertical_0 = down_0 - centre_0;
    }
    else
    {
        vertical_0 = centre_0 - up_0;
    }
    return normalize(cross(vertical_0, horizontal_0));
}

fn slice_count_0() -> u32
{
    return clamp(u32(camera_0.params_0.y), u32(2), u32(4));
}

fn bent_normals_0() -> bool
{
    return (camera_0.params_0.w) != 0.0f;
}

fn turned_0( seed_0 : vec2<f32>,  slice_0 : u32) -> vec2<f32>
{
    var eighth_0 : vec2<f32>;
    if(((slice_0 & (u32(2)))) != u32(0))
    {
        var _S9 : f32 = seed_0.x;
        var _S10 : f32 = seed_0.y;
        eighth_0 = vec2<f32>(_S9 - _S10, _S9 + _S10);
    }
    else
    {
        eighth_0 = seed_0;
    }
    if(((slice_0 & (u32(1)))) != u32(0))
    {
        eighth_0 = vec2<f32>(- eighth_0.y, eighth_0.x);
    }
    return eighth_0;
}

fn acos_approx_0( x_0 : f32) -> f32
{
    var _S11 : f32 = min(abs(x_0), 1.0f);
    var positive_0 : f32 = (((-0.01872929930686951f * _S11 + 0.07426100224256516f) * _S11 + -0.21211439371109009f) * _S11 + 1.57072877883911133f) * sqrt(1.0f - _S11);
    var _S12 : f32;
    if(x_0 < 0.0f)
    {
        _S12 = 3.14159274101257324f - positive_0;
    }
    else
    {
        _S12 = positive_0;
    }
    return _S12;
}

fn horizon_cosine_0( pixel_4 : vec2<i32>,  step_0 : vec2<f32>,  offset_0 : f32,  reach_0 : f32,  centre_1 : vec3<f32>,  view_1 : vec3<f32>,  radius_0 : f32,  extent_3 : vec2<i32>,  size_1 : vec2<f32>) -> f32
{
    var cosine_0 : f32 = -1.0f;
    var index_1 : u32 = u32(0);
    for(;;)
    {
        if(index_1 < u32(4))
        {
        }
        else
        {
            break;
        }
        var tap_0 : vec2<i32> = pixel_4 + vec2<i32>(step_0 * vec2<f32>((reach_0 * (f32(index_1) + offset_0) / 4.0f)));
        var _S13 : i32 = tap_0.x;
        var _S14 : bool;
        if(_S13 < i32(0))
        {
            _S14 = true;
        }
        else
        {
            _S14 = (tap_0.y) < i32(0);
        }
        var _S15 : bool;
        if(_S14)
        {
            _S15 = true;
        }
        else
        {
            _S15 = _S13 >= (extent_3.x);
        }
        var _S16 : bool;
        if(_S15)
        {
            _S16 = true;
        }
        else
        {
            _S16 = (tap_0.y) >= (extent_3.y);
        }
        if(_S16)
        {
            break;
        }
        var depth_3 : f32 = depth_at_0(tap_0, extent_3);
        if(depth_3 <= 0.0f)
        {
            index_1 = index_1 + u32(1);
            continue;
        }
        var delta_0 : vec3<f32> = view_position_0(tap_0, depth_3, size_1) - centre_1;
        var length_squared_0 : f32 = dot(delta_0, delta_0);
        var _S17 : bool;
        if(length_squared_0 > (radius_0 * radius_0))
        {
            _S17 = true;
        }
        else
        {
            _S17 = length_squared_0 < 1.00000001335143196e-10f;
        }
        if(_S17)
        {
            index_1 = index_1 + u32(1);
            continue;
        }
        cosine_0 = max(cosine_0, dot(delta_0, view_1) / sqrt(length_squared_0));
        index_1 = index_1 + u32(1);
    }
    return cosine_0;
}

fn slice_visibility_0( h1_0 : f32,  cos_h1_0 : f32,  sin_h1_0 : f32,  h2_0 : f32,  cos_h2_0 : f32,  sin_h2_0 : f32,  cos_gamma_0 : f32,  sin_gamma_0 : f32) -> f32
{
    return 0.25f * (- ((2.0f * cos_h1_0 * cos_h1_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h1_0 * cos_h1_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h1_0 * sin_gamma_0 + (- ((2.0f * cos_h2_0 * cos_h2_0 - 1.0f) * cos_gamma_0 + 2.0f * sin_h2_0 * cos_h2_0 * sin_gamma_0) + cos_gamma_0 + 2.0f * h2_0 * sin_gamma_0));
}

fn occlusion_at_0( pixel_5 : vec2<i32>,  tile_0 : u32,  centre_2 : vec3<f32>,  normal_0 : vec3<f32>,  extent_4 : vec2<i32>,  size_2 : vec2<f32>) -> vec4<f32>
{
    const unoccluded_0 : vec4<f32> = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
    var radius_1 : f32 = camera_0.params_0.x;
    var near_clip_0 : vec4<f32> = (((vec4<f32>(centre_2, 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var far_clip_0 : vec4<f32> = (((vec4<f32>(centre_2 + vec3<f32>(radius_1, 0.0f, 0.0f), 1.0f)) * (mat4x4<f32>(camera_0.proj_0.data_0[i32(0)][i32(0)], camera_0.proj_0.data_0[i32(1)][i32(0)], camera_0.proj_0.data_0[i32(2)][i32(0)], camera_0.proj_0.data_0[i32(3)][i32(0)], camera_0.proj_0.data_0[i32(0)][i32(1)], camera_0.proj_0.data_0[i32(1)][i32(1)], camera_0.proj_0.data_0[i32(2)][i32(1)], camera_0.proj_0.data_0[i32(3)][i32(1)], camera_0.proj_0.data_0[i32(0)][i32(2)], camera_0.proj_0.data_0[i32(1)][i32(2)], camera_0.proj_0.data_0[i32(2)][i32(2)], camera_0.proj_0.data_0[i32(3)][i32(2)], camera_0.proj_0.data_0[i32(0)][i32(3)], camera_0.proj_0.data_0[i32(1)][i32(3)], camera_0.proj_0.data_0[i32(2)][i32(3)], camera_0.proj_0.data_0[i32(3)][i32(3)]))));
    var _S18 : f32 = near_clip_0.w;
    var _S19 : bool;
    if(_S18 <= 0.0f)
    {
        _S19 = true;
    }
    else
    {
        _S19 = (far_clip_0.w) <= 0.0f;
    }
    if(_S19)
    {
        return unoccluded_0;
    }
    var reach_1 : f32 = abs(far_clip_0.x / far_clip_0.w - near_clip_0.x / _S18) * 0.5f * size_2.x;
    if(reach_1 < 2.0f)
    {
        return unoccluded_0;
    }
    var _S20 : vec3<f32> = normalize((vec3<f32>(0) - centre_2));
    var _S21 : u32 = slice_count_0();
    var _S22 : bool = bent_normals_0();
    const _S23 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slice_1 : u32 = u32(0);
    var visibility_0 : f32 = 0.0f;
    var weight_0 : f32 = 0.0f;
    var bent_0 : vec3<f32> = _S23;
    var bent_weight_0 : f32 = 0.0f;
    for(;;)
    {
        if(slice_1 < u32(4))
        {
        }
        else
        {
            break;
        }
        if(slice_1 >= _S21)
        {
            break;
        }
        var direction_1 : vec2<f32> = normalize(turned_0(SLICE_DIRECTIONS_0[tile_0], slice_1));
        var axis_0 : vec3<f32> = normalize(cross(vec3<f32>(direction_1.x, - direction_1.y, 0.0f), _S20));
        var _S24 : f32 = dot(normal_0, axis_0);
        var projected_0 : vec3<f32> = normal_0 - axis_0 * vec3<f32>(_S24);
        var projected_length_0 : f32 = length(projected_0);
        if(projected_length_0 < 9.99999997475242708e-07f)
        {
            slice_1 = slice_1 + u32(1);
            continue;
        }
        var cos_gamma_1 : f32 = clamp(dot(projected_0, _S20) / projected_length_0, -1.0f, 1.0f);
        var sign_gamma_0 : f32;
        if((dot(cross(_S20, axis_0), projected_0)) < 0.0f)
        {
            sign_gamma_0 = -1.0f;
        }
        else
        {
            sign_gamma_0 = 1.0f;
        }
        var gamma_0 : f32 = sign_gamma_0 * acos_approx_0(cos_gamma_1);
        var sin_gamma_1 : f32 = sign_gamma_0 * sqrt(saturate(1.0f - cos_gamma_1 * cos_gamma_1));
        var cos_negative_0 : f32 = horizon_cosine_0(pixel_5, (vec2<f32>(0) - direction_1), STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S20, radius_1, extent_4, size_2);
        var cos_positive_0 : f32 = horizon_cosine_0(pixel_5, direction_1, STEP_OFFSETS_0[tile_0], reach_1, centre_2, _S20, radius_1, extent_4, size_2);
        var raw_low_0 : f32 = - acos_approx_0(cos_negative_0);
        var low_0 : f32 = gamma_0 - 1.57079637050628662f;
        var clamped_low_0 : bool = raw_low_0 < low_0;
        var h1_1 : f32;
        if(clamped_low_0)
        {
            h1_1 = low_0;
        }
        else
        {
            h1_1 = raw_low_0;
        }
        var cos_h1_1 : f32;
        if(clamped_low_0)
        {
            cos_h1_1 = sin_gamma_1;
        }
        else
        {
            cos_h1_1 = cos_negative_0;
        }
        var sin_h1_1 : f32;
        if(clamped_low_0)
        {
            sin_h1_1 = - cos_gamma_1;
        }
        else
        {
            sin_h1_1 = - sqrt(saturate(1.0f - cos_negative_0 * cos_negative_0));
        }
        var raw_high_0 : f32 = acos_approx_0(cos_positive_0);
        var high_0 : f32 = gamma_0 + 1.57079637050628662f;
        var clamped_high_0 : bool = raw_high_0 > high_0;
        var h2_1 : f32;
        if(clamped_high_0)
        {
            h2_1 = high_0;
        }
        else
        {
            h2_1 = raw_high_0;
        }
        var cos_h2_1 : f32;
        if(clamped_high_0)
        {
            cos_h2_1 = - sin_gamma_1;
        }
        else
        {
            cos_h2_1 = cos_positive_0;
        }
        var sin_h2_1 : f32;
        if(clamped_high_0)
        {
            sin_h2_1 = cos_gamma_1;
        }
        else
        {
            sin_h2_1 = sqrt(saturate(1.0f - cos_positive_0 * cos_positive_0));
        }
        var _S25 : f32 = projected_length_0 * slice_visibility_0(h1_1, cos_h1_1, sin_h1_1, h2_1, cos_h2_1, sin_h2_1, cos_gamma_1, sin_gamma_1);
        var visibility_1 : f32 = visibility_0 + _S25;
        var weight_1 : f32 = weight_0 + projected_length_0;
        var bent_weight_1 : f32;
        var bent_1 : vec3<f32>;
        if(_S22)
        {
            var cos_sum_0 : f32 = cos_h1_1 * cos_h2_1 - sin_h1_1 * sin_h2_1;
            var cos_half_0 : f32 = sqrt(saturate(0.5f * (1.0f + cos_sum_0)));
            var sin_half_0 : f32 = sqrt(saturate(0.5f * (1.0f - cos_sum_0)));
            if((h1_1 + h2_1) < 0.0f)
            {
                bent_weight_1 = - sin_half_0;
            }
            else
            {
                bent_weight_1 = sin_half_0;
            }
            var cos_turn_0 : f32 = cos_half_0 * cos_gamma_1 + bent_weight_1 * sin_gamma_1;
            var bent_weight_2 : f32 = bent_weight_0 + _S25;
            bent_1 = bent_0 + (normal_0 * vec3<f32>(cos_turn_0) - cross(axis_0, normal_0) * vec3<f32>((bent_weight_1 * cos_gamma_1 - cos_half_0 * sin_gamma_1)) + axis_0 * vec3<f32>((_S24 * (1.0f - cos_turn_0)))) * vec3<f32>(_S25);
            bent_weight_1 = bent_weight_2;
        }
        else
        {
            bent_1 = bent_0;
            bent_weight_1 = bent_weight_0;
        }
        visibility_0 = visibility_1;
        weight_0 = weight_1;
        bent_0 = bent_1;
        bent_weight_0 = bent_weight_1;
        slice_1 = slice_1 + u32(1);
    }
    if(weight_0 <= 0.0f)
    {
        return unoccluded_0;
    }
    var occlusion_0 : f32 = saturate(1.0f - visibility_0 / weight_0);
    if(bent_weight_0 <= 0.0f)
    {
        _S19 = true;
    }
    else
    {
        _S19 = (length(bent_0 / vec3<f32>(bent_weight_0))) < 0.5f;
    }
    if(_S19)
    {
        return vec4<f32>(occlusion_0, 0.0f, 0.0f, 0.0f);
    }
    return vec4<f32>(occlusion_0, normalize((((vec4<f32>(bent_0, 0.0f)) * (mat4x4<f32>(camera_0.inv_view_0.data_0[i32(0)][i32(0)], camera_0.inv_view_0.data_0[i32(1)][i32(0)], camera_0.inv_view_0.data_0[i32(2)][i32(0)], camera_0.inv_view_0.data_0[i32(3)][i32(0)], camera_0.inv_view_0.data_0[i32(0)][i32(1)], camera_0.inv_view_0.data_0[i32(1)][i32(1)], camera_0.inv_view_0.data_0[i32(2)][i32(1)], camera_0.inv_view_0.data_0[i32(3)][i32(1)], camera_0.inv_view_0.data_0[i32(0)][i32(2)], camera_0.inv_view_0.data_0[i32(1)][i32(2)], camera_0.inv_view_0.data_0[i32(2)][i32(2)], camera_0.inv_view_0.data_0[i32(3)][i32(2)], camera_0.inv_view_0.data_0[i32(0)][i32(3)], camera_0.inv_view_0.data_0[i32(1)][i32(3)], camera_0.inv_view_0.data_0[i32(2)][i32(3)], camera_0.inv_view_0.data_0[i32(3)][i32(3)])))).xyz));
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
fn fragmentMain( _S26 : pixelInput_0, @builtin(position) position_1 : vec4<f32>) -> pixelOutput_0
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((scene_depth_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var extent_5 : vec2<i32> = vec2<i32>(i32(width_0), i32(height_0));
    var size_3 : vec2<f32> = vec2<f32>(f32(width_0), f32(height_0));
    var _S27 : vec2<i32> = vec2<i32>(position_1.xy);
    var pixel_6 : vec2<i32> = full_res_pixel_0(_S27);
    var tile_1 : u32 = ((u32(_S27.y) & (u32(3)))) * u32(4) + ((u32(_S27.x) & (u32(3))));
    var depth_4 : f32 = depth_at_0(pixel_6, extent_5);
    if(depth_4 <= 0.0f)
    {
        var _S28 : pixelOutput_0 = pixelOutput_0( vec4<f32>(1.0f, encode_bent_0(vec3<f32>(0.0f, 0.0f, 0.0f))) );
        return _S28;
    }
    var centre_3 : vec3<f32> = view_position_0(pixel_6, depth_4, size_3);
    var gathered_0 : vec4<f32> = occlusion_at_0(pixel_6, tile_1, centre_3, normal_at_0(pixel_6, centre_3, extent_5, size_3), extent_5, size_3);
    var _S29 : pixelOutput_0 = pixelOutput_0( vec4<f32>(saturate(1.0f - gathered_0.x), encode_bent_0(gathered_0.yzw)) );
    return _S29;
}

