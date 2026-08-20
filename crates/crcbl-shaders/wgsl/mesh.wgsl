struct DrawConstants_std140_0
{
    @align(16) base_0 : u32,
    @align(4) mesh_0 : u32,
    @align(8) pad0_0 : u32,
    @align(4) pad1_0 : u32,
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
    @align(16) mesh_1 : u32,
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
    @align(16) uv_0 : vec4<f32>,
};

@binding(1) @group(0) var<storage, read> vertices_0 : array<MeshVertex_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_1 : array<vec4<f32>, i32(4)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0
{
    @align(16) data_2 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(2)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E6_0
{
    @align(16) data_3 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(6)>,
};

struct FrameUniforms_std140_0
{
    @align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) camera_position_0 : vec4<f32>,
    @align(16) ambient_0 : vec4<f32>,
    @align(16) shadow_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0,
    @align(16) cascade_far_0 : vec4<f32>,
    @align(16) shadow_params_0 : vec4<f32>,
    @align(16) cluster_grid_0 : vec4<u32>,
    @align(16) light_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E6_0,
    @align(16) probe_origin_0 : vec4<f32>,
    @align(16) probe_inv_spacing_0 : vec4<f32>,
    @align(16) probe_counts_0 : vec4<u32>,
    @align(16) lod_params_0 : vec4<f32>,
};

@binding(0) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
struct GpuMaterial_std430_0
{
    @align(16) base_color_0 : vec4<f32>,
    @align(16) base_color_texture_0 : u32,
    @align(4) metallic_0 : f32,
    @align(8) roughness_0 : f32,
    @align(4) tiling_0 : u32,
    @align(16) tile_metres_0 : f32,
    @align(4) pad0_1 : u32,
    @align(8) pad1_1 : u32,
    @align(4) pad2_0 : u32,
};

@binding(6) @group(0) var<storage, read> materials_0 : array<GpuMaterial_std430_0>;

@binding(7) @group(0) var base_color_textures_0 : texture_2d_array<f32>;

@binding(8) @group(0) var base_color_sampler_0 : sampler;

@binding(21) @group(0) var<storage, read> cluster_lights_0 : array<u32>;

struct GpuLight_std430_0
{
    @align(16) position_1 : vec4<f32>,
    @align(16) color_1 : vec4<f32>,
    @align(16) direction_0 : vec4<f32>,
    @align(16) kind_0 : u32,
    @align(4) cos_inner_0 : f32,
    @align(8) shadow_tile_0 : u32,
    @align(4) pad1_2 : u32,
};

@binding(20) @group(0) var<storage, read> lights_0 : array<GpuLight_std430_0>;

@binding(15) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(16) @group(0) var shadow_sampler_0 : sampler_comparison;

@binding(22) @group(0) var ambient_occlusion_0 : texture_2d<f32>;

struct GpuProbe_std430_0
{
    @align(16) sh_r_0 : vec4<f32>,
    @align(16) sh_g_0 : vec4<f32>,
    @align(16) sh_b_0 : vec4<f32>,
};

@binding(23) @group(0) var<storage, read> probes_0 : array<GpuProbe_std430_0>;

struct VertexOutput_0
{
    @builtin(position) position_2 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @location(2) world_normal_0 : vec3<f32>,
    @location(3) color_2 : vec4<f32>,
    @interpolate(flat) @location(4) material_1 : u32,
    @location(1) uv_1 : vec2<f32>,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32, @builtin(instance_index) instance_id_0 : u32) -> VertexOutput_0
{
    var instance_0 : GpuInstance_std430_0 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]];
    var vertex_0 : MeshVertex_std430_0 = vertices_0[index_0 + meshes_0[draw_0.mesh_0].base_vertex_0];
    var _S1 : mat4x4<f32> = mat4x4<f32>(instance_0.transform_0.data_0[i32(0)][i32(0)], instance_0.transform_0.data_0[i32(1)][i32(0)], instance_0.transform_0.data_0[i32(2)][i32(0)], instance_0.transform_0.data_0[i32(3)][i32(0)], instance_0.transform_0.data_0[i32(0)][i32(1)], instance_0.transform_0.data_0[i32(1)][i32(1)], instance_0.transform_0.data_0[i32(2)][i32(1)], instance_0.transform_0.data_0[i32(3)][i32(1)], instance_0.transform_0.data_0[i32(0)][i32(2)], instance_0.transform_0.data_0[i32(1)][i32(2)], instance_0.transform_0.data_0[i32(2)][i32(2)], instance_0.transform_0.data_0[i32(3)][i32(2)], instance_0.transform_0.data_0[i32(0)][i32(3)], instance_0.transform_0.data_0[i32(1)][i32(3)], instance_0.transform_0.data_0[i32(2)][i32(3)], instance_0.transform_0.data_0[i32(3)][i32(3)]);
    var world_0 : vec4<f32> = (((vec4<f32>(vertex_0.position_0.xyz, 1.0f)) * (_S1)));
    var output_0 : VertexOutput_0;
    output_0.position_2 = (((world_0) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)]))));
    output_0.world_position_0 = world_0.xyz;
    output_0.world_normal_0 = (((vertex_0.normal_0.xyz) * (mat3x3<f32>(_S1[i32(0)].xyz, _S1[i32(1)].xyz, _S1[i32(2)].xyz))));
    var _S2 : vec4<f32>;
    if((frame_0.ambient_0.w) >= 1.5f)
    {
        _S2 = vec4<f32>(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);
    }
    else
    {
        _S2 = vertex_0.color_0;
    }
    output_0.color_2 = _S2;
    output_0.material_1 = instance_0.material_0;
    output_0.uv_1 = vertex_0.uv_0.xy;
    return output_0;
}

fn geometric_normal_of_0( world_position_1 : vec3<f32>,  shading_normal_0 : vec3<f32>) -> vec3<f32>
{
    var facet_0 : vec3<f32> = cross(dpdx(world_position_1), dpdy(world_position_1));
    var extent_0 : f32 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {
        return shading_normal_0;
    }
    var facet_1 : vec3<f32> = facet_0 / vec3<f32>(extent_0);
    var _S3 : vec3<f32>;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {
        _S3 = (vec3<f32>(0) - facet_1);
    }
    else
    {
        _S3 = facet_1;
    }
    return _S3;
}

fn physical_tile_uv_0( world_position_2 : vec3<f32>,  normal_1 : vec3<f32>,  tile_metres_1 : f32) -> vec2<f32>
{
    var axis_0 : vec3<f32> = abs(normal_1);
    var _S4 : f32 = axis_0.x;
    var _S5 : f32 = axis_0.y;
    var _S6 : bool;
    if(_S4 >= _S5)
    {
        _S6 = _S4 >= (axis_0.z);
    }
    else
    {
        _S6 = false;
    }
    var planar_0 : vec2<f32>;
    if(_S6)
    {
        planar_0 = world_position_2.zy;
    }
    else
    {
        if(_S5 >= (axis_0.z))
        {
            planar_0 = world_position_2.xz;
        }
        else
        {
            planar_0 = world_position_2.xy;
        }
    }
    return planar_0 / vec2<f32>(max(tile_metres_1, 0.00009999999747379f));
}

fn froxel_of_0( pixel_0 : vec2<f32>,  depth_0 : f32) -> u32
{
    var _S7 : u32 = max(frame_0.cluster_grid_0.x, u32(1));
    var _S8 : u32 = max(frame_0.cluster_grid_0.y, u32(1));
    var _S9 : u32 = max(frame_0.cluster_grid_0.z, u32(1));
    var _S10 : u32 = max(frame_0.cluster_grid_0.w, u32(1));
    var _S11 : u32 = u32(pixel_0.x) / _S10;
    var _S12 : u32 = min(_S11, _S7 - u32(1));
    var _S13 : u32 = u32(pixel_0.y) / _S10;
    var scale_0 : f32 = 24.0f / log2(10000.0f);
    return (u32(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, f32(_S9 - u32(1)))) * _S8 + min(_S13, _S8 - u32(1))) * _S7 + _S12;
}

fn punctual_falloff_0( distance_0 : f32,  radius_0 : f32) -> f32
{
    var ratio_0 : f32 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    var window_0 : f32 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0 / (distance_0 * distance_0 + 1.0f);
}

fn spot_cone_0( to_light_0 : vec3<f32>,  axis_1 : vec3<f32>,  cos_outer_0 : f32,  cos_inner_1 : f32) -> f32
{
    return saturate((dot((vec3<f32>(0) - to_light_0), normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}

fn ggx_lobe_0( alpha2_0 : f32,  f0_0 : vec3<f32>,  n_dot_l_0 : f32,  n_dot_v_0 : f32,  n_dot_h_0 : f32,  v_dot_h_0 : f32) -> vec3<f32>
{
    var shape_0 : f32 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;
    var _S14 : f32 = 1.0f - alpha2_0;
    var grazing_0 : f32 = 1.0f - v_dot_h_0;
    var grazing2_0 : f32 = grazing_0 * grazing_0;
    return vec3<f32>((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_0 * n_dot_v_0 * _S14 + alpha2_0) + n_dot_v_0 * sqrt(n_dot_l_0 * n_dot_l_0 * _S14 + alpha2_0), 9.99999997475242708e-07f)))) * (f0_0 + (vec3<f32>(1.0f, 1.0f, 1.0f) - f0_0) * vec3<f32>((grazing2_0 * grazing2_0 * grazing_0)));
}

fn shadow_slope_0( geometric_normal_0 : vec3<f32>,  to_light_1 : vec3<f32>) -> f32
{
    var cosine_0 : f32 = saturate(dot(geometric_normal_0, to_light_1));
    return min(sqrt(saturate(1.0f - cosine_0 * cosine_0)) / max(cosine_0, 0.00009999999747379f), 5.0f);
}

fn atlas_uv_0( tile_0 : u32,  tile_uv_0 : vec2<f32>) -> vec2<f32>
{
    return (vec2<f32>(f32(tile_0 % u32(4)), f32(tile_0 / u32(4))) + tile_uv_0) / vec2<f32>(4.0f, 2.0f);
}

fn tile_pcf_0( tile_1 : u32,  tile_uv_1 : vec2<f32>,  reference_0 : f32) -> f32
{
    var texel_0 : vec2<f32> = frame_0.shadow_params_0.xy;
    const grid_0 : vec2<f32> = vec2<f32>(4.0f, 2.0f);
    var _S15 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_0 * grid_0;
    var y_0 : i32 = i32(-1);
    var visibility_0 : f32 = 0.0f;
    for(;;)
    {
        if(y_0 <= i32(1))
        {
        }
        else
        {
            break;
        }
        var x_0 : i32 = i32(-1);
        for(;;)
        {
            if(x_0 <= i32(1))
            {
            }
            else
            {
                break;
            }
            var visibility_1 : f32 = visibility_0 + (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (atlas_uv_0(tile_1, clamp(tile_uv_1 + vec2<f32>(f32(x_0), f32(y_0)) * texel_0 * grid_0, _S15, vec2<f32>(1.0f) - _S15))), (reference_0)));
            x_0 = x_0 + i32(1);
            visibility_0 = visibility_1;
        }
        y_0 = y_0 + i32(1);
    }
    return visibility_0 / 9.0f;
}

fn sun_visibility_0( world_position_3 : vec3<f32>,  to_light_2 : vec3<f32>,  n_dot_l_1 : f32,  geometric_normal_1 : vec3<f32>) -> f32
{
    var cascade_0 : u32;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }
    var _S16 : f32 = length(world_position_3 - frame_0.camera_position_0.xyz);
    var index_1 : u32 = u32(0);
    for(;;)
    {
        if(index_1 < u32(2))
        {
        }
        else
        {
            cascade_0 = u32(1);
            break;
        }
        if(_S16 < (frame_0.cascade_far_0[index_1]))
        {
            cascade_0 = index_1;
            break;
        }
        index_1 = index_1 + u32(1);
    }
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_3 + to_light_2 * vec3<f32>((2.0f * frame_0.cascade_far_0[cascade_0] / 1024.0f * (frame_0.shadow_params_0.z + frame_0.shadow_params_0.w * shadow_slope_0(geometric_normal_1, to_light_2)))), 1.0f)) * (mat4x4<f32>(frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(0)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(1)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(2)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_0].data_1[i32(3)][i32(3)]))));
    var ndc_0 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S17 : bool;
    if((any(((abs(ndc_0.xy)) > vec2<f32>(1.0f)))))
    {
        _S17 = true;
    }
    else
    {
        _S17 = (ndc_0.z) <= 0.0f;
    }
    if(_S17)
    {
        return 1.0f;
    }
    return tile_pcf_0(cascade_0, vec2<f32>(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f), ndc_0.z);
}

fn point_face_0( from_light_0 : vec3<f32>) -> u32
{
    var axis_2 : vec3<f32> = abs(from_light_0);
    var _S18 : f32 = axis_2.x;
    var _S19 : f32 = axis_2.y;
    var _S20 : bool;
    if(_S18 >= _S19)
    {
        _S20 = _S18 >= (axis_2.z);
    }
    else
    {
        _S20 = false;
    }
    var _S21 : u32;
    if(_S20)
    {
        if((from_light_0.x) >= 0.0f)
        {
            _S21 = u32(0);
        }
        else
        {
            _S21 = u32(1);
        }
        return _S21;
    }
    if(_S19 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {
            _S21 = u32(2);
        }
        else
        {
            _S21 = u32(3);
        }
        return _S21;
    }
    if((from_light_0.z) >= 0.0f)
    {
        _S21 = u32(4);
    }
    else
    {
        _S21 = u32(5);
    }
    return _S21;
}

fn light_tile_0( tile_2 : u32) -> u32
{
    return u32(2) + tile_2;
}

fn punctual_visibility_0( tile_3 : u32,  world_position_4 : vec3<f32>,  to_light_3 : vec3<f32>,  n_dot_l_2 : f32,  texel_world_0 : f32,  geometric_normal_2 : vec3<f32>) -> f32
{
    var clip_1 : vec4<f32> = (((vec4<f32>(world_position_4 + to_light_3 * vec3<f32>((texel_world_0 * (2.0f + 4.0f * shadow_slope_0(geometric_normal_2, to_light_3)))), 1.0f)) * (mat4x4<f32>(frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(3)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(3)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(3)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(3)]))));
    var _S22 : f32 = clip_1.w;
    if(_S22 <= 0.0f)
    {
        return 1.0f;
    }
    var ndc_1 : vec3<f32> = clip_1.xyz / vec3<f32>(_S22);
    var _S23 : bool;
    if((any(((abs(ndc_1.xy)) > vec2<f32>(1.0f)))))
    {
        _S23 = true;
    }
    else
    {
        _S23 = (ndc_1.z) <= 0.0f;
    }
    if(_S23)
    {
        _S23 = true;
    }
    else
    {
        _S23 = (ndc_1.z) > 1.0f;
    }
    if(_S23)
    {
        return 1.0f;
    }
    return tile_pcf_0(light_tile_0(tile_3), vec2<f32>(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z);
}

fn point_visibility_0( light_0 : ptr<function, GpuLight_std430_0>,  base_1 : u32,  world_position_5 : vec3<f32>,  to_light_4 : vec3<f32>,  n_dot_l_3 : f32,  geometric_normal_3 : vec3<f32>) -> f32
{
    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }
    var from_light_1 : vec3<f32> = world_position_5 - (*light_0).position_1.xyz;
    return punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_5, to_light_4, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)) / 1024.0f, geometric_normal_3);
}

fn spot_visibility_0( light_1 : ptr<function, GpuLight_std430_0>,  tile_4 : u32,  world_position_6 : vec3<f32>,  to_light_5 : vec3<f32>,  n_dot_l_4 : f32,  geometric_normal_4 : vec3<f32>) -> f32
{
    if(n_dot_l_4 <= 0.0f)
    {
        return 1.0f;
    }
    var cos_outer_1 : f32 = (*light_1).direction_0.w;
    return punctual_visibility_0(tile_4, world_position_6, to_light_5, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_6 - (*light_1).position_1.xyz, normalize((*light_1).direction_0.xyz)), 0.0f) / 1024.0f, geometric_normal_4);
}

struct GpuProbe_0
{
     sh_r_0 : vec4<f32>,
     sh_g_0 : vec4<f32>,
     sh_b_0 : vec4<f32>,
};

fn probe_at_0( cell_0 : vec3<u32>) -> GpuProbe_0
{
    var _S24 : GpuProbe_std430_0 = probes_0[min((cell_0.z * frame_0.probe_counts_0.y + cell_0.y) * frame_0.probe_counts_0.x + cell_0.x, max(frame_0.probe_counts_0.w, u32(1)) - u32(1))];
    var _S25 : GpuProbe_0 = GpuProbe_0( _S24.sh_r_0, _S24.sh_g_0, _S24.sh_b_0 );
    return _S25;
}

fn lerp_probe_0( a_0 : GpuProbe_0,  b_0 : GpuProbe_0,  t_0 : f32) -> GpuProbe_0
{
    var blended_0 : GpuProbe_0;
    var _S26 : vec4<f32> = vec4<f32>(t_0);
    blended_0.sh_r_0 = mix(a_0.sh_r_0, b_0.sh_r_0, _S26);
    blended_0.sh_g_0 = mix(a_0.sh_g_0, b_0.sh_g_0, _S26);
    blended_0.sh_b_0 = mix(a_0.sh_b_0, b_0.sh_b_0, _S26);
    return blended_0;
}

fn probe_irradiance_0( world_position_7 : vec3<f32>,  normal_2 : vec3<f32>) -> vec3<f32>
{
    var _S27 : vec3<f32> = vec3<f32>(1.0f);
    const _S28 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var last_0 : vec3<f32> = max(vec3<f32>(frame_0.probe_counts_0.xyz) - _S27, _S28);
    var grid_1 : vec3<f32> = clamp((world_position_7 - frame_0.probe_origin_0.xyz) * frame_0.probe_inv_spacing_0.xyz, _S28, last_0);
    var base_2 : vec3<f32> = floor(grid_1);
    var f_0 : vec3<f32> = grid_1 - base_2;
    var _S29 : vec3<u32> = vec3<u32>(base_2);
    var _S30 : vec3<u32> = vec3<u32>(min(base_2 + _S27, last_0));
    var _S31 : u32 = _S29.x;
    var _S32 : u32 = _S29.y;
    var _S33 : u32 = _S29.z;
    var _S34 : u32 = _S30.x;
    var _S35 : f32 = f_0.x;
    var _S36 : u32 = _S30.y;
    var _S37 : u32 = _S30.z;
    var _S38 : f32 = f_0.y;
    var cell_1 : GpuProbe_0 = lerp_probe_0(lerp_probe_0(lerp_probe_0(probe_at_0(vec3<u32>(_S31, _S32, _S33)), probe_at_0(vec3<u32>(_S34, _S32, _S33)), _S35), lerp_probe_0(probe_at_0(vec3<u32>(_S31, _S36, _S33)), probe_at_0(vec3<u32>(_S34, _S36, _S33)), _S35), _S38), lerp_probe_0(lerp_probe_0(probe_at_0(vec3<u32>(_S31, _S32, _S37)), probe_at_0(vec3<u32>(_S34, _S32, _S37)), _S35), lerp_probe_0(probe_at_0(vec3<u32>(_S31, _S36, _S37)), probe_at_0(vec3<u32>(_S34, _S36, _S37)), _S35), _S38), f_0.z);
    var basis_0 : vec4<f32> = vec4<f32>(normal_2, 1.0f);
    return max(vec3<f32>(dot(cell_1.sh_r_0, basis_0), dot(cell_1.sh_g_0, basis_0), dot(cell_1.sh_b_0, basis_0)), _S28);
}

struct FragmentOutput_0
{
    @location(0) lit_0 : vec4<f32>,
    @location(1) reflectivity_0 : vec4<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_8 : vec3<f32>,
    @location(2) world_normal_1 : vec3<f32>,
    @location(3) color_3 : vec4<f32>,
    @interpolate(flat) @location(4) material_2 : u32,
    @location(1) uv_2 : vec2<f32>,
};

@fragment
fn fragmentMain( _S39 : pixelInput_0, @builtin(position) position_3 : vec4<f32>) -> FragmentOutput_0
{
    var normal_3 : vec3<f32> = normalize(_S39.world_normal_1);
    if((frame_0.ambient_0.w) >= 1.5f)
    {
        var tint_0 : FragmentOutput_0;
        tint_0.lit_0 = vec4<f32>(_S39.color_3.xyz, 1.0f);
        tint_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
        return tint_0;
    }
    if((frame_0.ambient_0.w) >= 0.5f)
    {
        var normals_0 : FragmentOutput_0;
        var _S40 : vec3<f32> = vec3<f32>(0.5f);
        normals_0.lit_0 = vec4<f32>(normal_3 * _S40 + _S40, 1.0f);
        normals_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 0.0f);
        return normals_0;
    }
    var to_eye_0 : vec3<f32> = normalize(frame_0.camera_position_0.xyz - _S39.world_position_8);
    var _S41 : vec3<f32> = geometric_normal_of_0(_S39.world_position_8, normal_3);
    var material_3 : GpuMaterial_std430_0 = materials_0[_S39.material_2];
    var uv_3 : vec2<f32>;
    if((material_3.tiling_0) == u32(1))
    {
        uv_3 = physical_tile_uv_0(_S39.world_position_8, normal_3, material_3.tile_metres_0);
    }
    else
    {
        uv_3 = _S39.uv_2;
    }
    var _S42 : vec3<f32> = vec3<f32>(uv_3, f32(material_3.base_color_texture_0));
    var albedo_0 : vec4<f32> = _S39.color_3 * material_3.base_color_0 * (textureSample((base_color_textures_0), (base_color_sampler_0), ((_S42)).xy, i32(((_S42)).z)));
    var metallic_1 : f32 = saturate(material_3.metallic_0);
    var roughness_1 : f32 = clamp(material_3.roughness_0, 0.04500000178813934f, 1.0f);
    var alpha_0 : f32 = roughness_1 * roughness_1;
    var _S43 : f32 = alpha_0 * alpha_0;
    var _S44 : vec3<f32> = albedo_0.xyz;
    var f0_1 : vec3<f32> = mix(vec3<f32>(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S44, vec3<f32>(metallic_1));
    var diffuse_albedo_0 : vec3<f32> = _S44 * vec3<f32>((1.0f - metallic_1));
    var _S45 : f32 = max(dot(normal_3, to_eye_0), 0.00009999999747379f);
    var _S46 : vec2<f32> = position_3.xy;
    var _S47 : u32 = froxel_of_0(_S46, (((vec4<f32>(_S39.world_position_8, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)])))).w);
    var base_3 : u32 = _S47 * u32(17);
    var _S48 : u32 = min(cluster_lights_0[base_3], u32(16));
    const _S49 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slot_0 : u32 = u32(0);
    var direct_0 : vec3<f32> = _S49;
    var gloss_0 : vec3<f32> = _S49;
    for(;;)
    {
        if(slot_0 < _S48)
        {
        }
        else
        {
            break;
        }
        var _S50 : GpuLight_std430_0 = lights_0[cluster_lights_0[base_3 + u32(1) + slot_0]];
        var _S51 : u32 = _S50.kind_0;
        var _S52 : bool = (_S50.kind_0) == u32(0);
        var to_light_6 : vec3<f32>;
        var reach_0 : f32;
        if(_S52)
        {
            to_light_6 = normalize(_S50.direction_0.xyz);
            reach_0 = 1.0f;
        }
        else
        {
            var offset_0 : vec3<f32> = _S50.position_1.xyz - _S39.world_position_8;
            var distance_1 : f32 = length(offset_0);
            var to_light_7 : vec3<f32> = offset_0 / vec3<f32>(max(distance_1, 9.99999997475242708e-07f));
            var reach_1 : f32 = punctual_falloff_0(distance_1, _S50.position_1.w);
            if(_S51 == u32(2))
            {
                reach_0 = reach_1 * spot_cone_0(to_light_7, _S50.direction_0.xyz, _S50.direction_0.w, _S50.cos_inner_0);
            }
            else
            {
                reach_0 = reach_1;
            }
            to_light_6 = to_light_7;
        }
        var n_dot_l_5 : f32 = dot(normal_3, to_light_6);
        var _S53 : f32 = max(n_dot_l_5, 0.0f);
        var half_vector_0 : vec3<f32> = normalize(to_light_6 + to_eye_0);
        var specular_0 : vec3<f32> = ggx_lobe_0(_S43, f0_1, _S53, _S45, max(dot(normal_3, half_vector_0), 0.0f), max(dot(to_eye_0, half_vector_0), 0.0f)) * vec3<f32>(_S53);
        var reach_2 : f32;
        if(_S52)
        {
            reach_2 = sun_visibility_0(_S39.world_position_8, to_light_6, n_dot_l_5, _S41);
        }
        else
        {
            if(_S51 == u32(1))
            {
                var _S54 : u32 = _S50.shadow_tile_0;
                if((_S50.shadow_tile_0) <= u32(0))
                {
                    var _S55 : f32 = point_visibility_0(&(_S50), _S54, _S39.world_position_8, to_light_6, n_dot_l_5, _S41);
                    reach_2 = reach_0 * _S55;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
            else
            {
                var _S56 : u32 = _S50.shadow_tile_0;
                if((_S50.shadow_tile_0) < u32(6))
                {
                    var _S57 : f32 = spot_visibility_0(&(_S50), _S56, _S39.world_position_8, to_light_6, n_dot_l_5, _S41);
                    reach_2 = reach_0 * _S57;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
        }
        var _S58 : vec3<f32> = _S50.color_1.xyz;
        var direct_1 : vec3<f32> = direct_0 + _S58 * vec3<f32>((_S53 * reach_2));
        var gloss_1 : vec3<f32> = gloss_0 + _S58 * (specular_0 * vec3<f32>(reach_2));
        slot_0 = slot_0 + u32(1);
        direct_0 = direct_1;
        gloss_0 = gloss_1;
    }
    var occlusion_width_0 : u32;
    var occlusion_height_0 : u32;
    {var dim = textureDimensions((ambient_occlusion_0));((occlusion_width_0)) = dim.x;((occlusion_height_0)) = dim.y;};
    var _S59 : vec3<i32> = vec3<i32>(min(vec2<i32>(_S46), vec2<i32>(i32(occlusion_width_0), i32(occlusion_height_0)) - vec2<i32>(i32(1))), i32(0));
    var output_1 : FragmentOutput_0;
    output_1.lit_0 = vec4<f32>(diffuse_albedo_0 * ((frame_0.ambient_0.xyz + probe_irradiance_0(_S39.world_position_8, normal_3)) * vec3<f32>((textureLoad((ambient_occlusion_0), ((_S59)).xy, ((_S59)).z).x)) + direct_0) + gloss_0, albedo_0.w);
    output_1.reflectivity_0 = vec4<f32>(f0_1, saturate(1.0f - roughness_1 / 0.5f));
    return output_1;
}

