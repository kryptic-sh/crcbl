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
    @align(16) previous_transform_0 : _MatrixStorage_float4x4_ColMajorstd430_0,
    @align(16) mesh_1 : u32,
    @align(4) material_0 : u32,
    @align(8) sector_0 : u32,
    @align(4) flags_0 : u32,
    @align(16) base_vertex_0 : u32,
    @align(4) previous_base_vertex_0 : u32,
    @align(8) pad1_1 : u32,
    @align(4) pad2_0 : u32,
};

@binding(2) @group(0) var<storage, read> instances_0 : array<GpuInstance_std430_0>;

struct GpuMesh_std430_0
{
    @align(4) base_vertex_1 : u32,
    @align(4) base_index_0 : u32,
    @align(4) index_count_0 : u32,
    @align(4) min_x_0 : f32,
    @align(4) min_y_0 : f32,
    @align(4) min_z_0 : f32,
    @align(4) max_x_0 : f32,
    @align(4) max_y_0 : f32,
    @align(4) max_z_0 : f32,
    @align(4) uv_scale_u_0 : f32,
    @align(4) uv_scale_v_0 : f32,
    @align(4) uv_offset_u_0 : f32,
    @align(4) uv_offset_v_0 : f32,
    @align(4) flags_1 : u32,
};

@binding(4) @group(0) var<storage, read> meshes_0 : array<GpuMesh_std430_0>;

struct _MatrixStorage_float4x4_ColMajorstd140_0
{
    @align(16) data_1 : array<vec4<f32>, i32(4)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E2_0
{
    @align(16) data_2 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(2)>,
};

struct _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0
{
    @align(16) data_3 : array<_MatrixStorage_float4x4_ColMajorstd140_0, i32(14)>,
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
    @align(16) light_view_proj_0 : _Array_std140_matrixx3Cfloatx2C4x2C4x3E14_0,
    @align(16) probe_origin_0 : vec4<f32>,
    @align(16) probe_inv_spacing_0 : vec4<f32>,
    @align(16) probe_counts_0 : vec4<u32>,
    @align(16) lod_params_0 : vec4<f32>,
    @align(16) fog_params_0 : vec4<f32>,
    @align(16) fog_color_0 : vec4<f32>,
    @align(16) sky_sh_r_0 : vec4<f32>,
    @align(16) sky_sh_g_0 : vec4<f32>,
    @align(16) sky_sh_b_0 : vec4<f32>,
    @align(16) previous_view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0,
    @align(16) vertex_pool_0 : vec4<u32>,
    @align(16) shadow_atlas_rect_0 : array<vec4<f32>, i32(16)>,
};

@binding(0) @group(0) var<uniform> frame_0 : FrameUniforms_std140_0;
@binding(1) @group(0) var<storage, read> vertices_0 : array<u32>;

@binding(22) @group(0) var ambient_occlusion_0 : texture_2d<f32>;

struct GpuMaterial_std430_0
{
    @align(16) base_color_0 : vec4<f32>,
    @align(16) color_normal_pages_0 : u32,
    @align(4) metallic_0 : f32,
    @align(8) roughness_0 : f32,
    @align(4) tiling_0 : u32,
    @align(16) tile_metres_0 : f32,
    @align(4) emissive_r_0 : f32,
    @align(8) emissive_g_0 : f32,
    @align(4) emissive_b_0 : f32,
    @align(16) mro_emissive_pages_0 : u32,
    @align(4) normal_scale_0 : f32,
    @align(8) alpha_cutoff_0 : f32,
    @align(4) flags_2 : u32,
};

@binding(6) @group(0) var<storage, read> materials_0 : array<GpuMaterial_std430_0>;

@binding(26) @group(0) var normal_textures_0 : texture_2d_array<f32>;

@binding(8) @group(0) var base_color_sampler_0 : sampler;

@binding(7) @group(0) var base_color_textures_0 : texture_2d_array<f32>;

@binding(21) @group(0) var<storage, read> cluster_lights_0 : array<u32>;

@binding(25) @group(0) var specular_dfg_0 : texture_2d<f32>;

struct GpuLight_std430_0
{
    @align(16) position_0 : vec4<f32>,
    @align(16) color_0 : vec4<f32>,
    @align(16) direction_0 : vec4<f32>,
    @align(16) tangent_0 : vec4<f32>,
    @align(16) kind_0 : u32,
    @align(4) cos_inner_0 : f32,
    @align(8) shadow_tile_0 : u32,
    @align(4) flags_3 : u32,
};

@binding(20) @group(0) var<storage, read> lights_0 : array<GpuLight_std430_0>;

@binding(27) @group(0) var ltc_matrix_0 : texture_2d<f32>;

@binding(15) @group(0) var shadow_atlas_0 : texture_depth_2d;

@binding(16) @group(0) var shadow_sampler_0 : sampler_comparison;

@binding(28) @group(0) var contact_shadow_0 : texture_2d<f32>;

struct GpuProbe_std430_0
{
    @align(16) sh_r_0 : vec4<f32>,
    @align(16) sh_g_0 : vec4<f32>,
    @align(16) sh_b_0 : vec4<f32>,
};

@binding(23) @group(0) var<storage, read> probes_0 : array<GpuProbe_std430_0>;

@binding(29) @group(0) var probe_visibility_0 : texture_2d_array<f32>;

var<private> FOG_RATIO_KERNEL_0 : array<f32, i32(5)> = array<f32, i32(5)>( 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f );
var<private> FOG_KERNEL_0 : array<f32, i32(8)> = array<f32, i32(8)>( 1.0f, 1.0f, 0.5f, 0.1666666716337204f, 0.0416666679084301f, 0.00833333376795053f, 0.00138888892251998f, 0.0001984127011383f );
var<private> SHADOW_DISC_0 : array<vec2<f32>, i32(32)> = array<vec2<f32>, i32(32)>( vec2<f32>(0.125f, 0.0f), vec2<f32>(-0.15964500606060028f, 0.14624799787998199f), vec2<f32>(0.02443600073456764f, -0.27843800187110901f), vec2<f32>(0.2012220025062561f, 0.26245900988578796f), vec2<f32>(-0.36926800012588501f, -0.06531800329685211f), vec2<f32>(0.34980198740959167f, -0.22251600027084351f), vec2<f32>(-0.11700200289487839f, 0.43524199724197388f), vec2<f32>(-0.22313599288463593f, -0.42963400483131409f), vec2<f32>(0.48411500453948975f, 0.17679800093173981f), vec2<f32>(-0.50364100933074951f, 0.20789599418640137f), vec2<f32>(0.24278800189495087f, -0.51882398128509521f), vec2<f32>(0.17941400408744812f, 0.57200098037719727f), vec2<f32>(-0.54075700044631958f, -0.31338000297546387f), vec2<f32>(0.63437002897262573f, -0.13946400582790375f), vec2<f32>(-0.38714599609375f, 0.55067497491836548f), vec2<f32>(-0.0894400030374527f, -0.69019997119903564f), vec2<f32>(0.5490720272064209f, 0.46275800466537476f), vec2<f32>(-0.73887801170349121f, 0.0305550005286932f), vec2<f32>(0.5389549732208252f, -0.53633201122283936f), vec2<f32>(-0.03605800122022629f, 0.77979201078414917f), vec2<f32>(-0.51281797885894775f, -0.61452698707580566f), vec2<f32>(0.81235998868942261f, 0.10930199921131134f), vec2<f32>(-0.68831098079681396f, 0.47890898585319519f), vec2<f32>(0.18808600306510925f, -0.83606100082397461f), vec2<f32>(0.43503299355506897f, 0.75919097661972046f), vec2<f32>(-0.85044801235198975f, -0.27131599187850952f), vec2<f32>(0.82610201835632324f, -0.38168001174926758f), vec2<f32>(-0.35788801312446594f, 0.85515600442886353f), vec2<f32>(-0.31940698623657227f, -0.88803398609161377f), vec2<f32>(0.84990900754928589f, 0.44668799638748169f), vec2<f32>(-0.94403499364852905f, 0.24884499609470367f), vec2<f32>(0.53659600019454956f, -0.83452999591827393f) );
var<private> SHADOW_PROBE_INDEX_0 : array<u32, i32(5)> = array<u32, i32(5)>( u32(0), u32(23), u32(25), u32(27), u32(29) );
var<private> SHADOW_SEARCH_DISC_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(0.17677700519561768f, 0.0f), vec2<f32>(-0.22577199339866638f, 0.20682600140571594f), vec2<f32>(0.0345579981803894f, -0.39377099275588989f), vec2<f32>(0.28457099199295044f, 0.37117299437522888f), vec2<f32>(-0.52222299575805664f, -0.09237399697303772f), vec2<f32>(0.49469500780105591f, -0.31468498706817627f), vec2<f32>(-0.16546599566936493f, 0.6155250072479248f), vec2<f32>(-0.31556099653244019f, -0.60759401321411133f), vec2<f32>(0.68464201688766479f, 0.25003001093864441f), vec2<f32>(-0.71225601434707642f, 0.2940090000629425f), vec2<f32>(0.3433539867401123f, -0.73372900485992432f), vec2<f32>(0.25372999906539917f, 0.80893200635910034f), vec2<f32>(-0.76474601030349731f, -0.44318601489067078f), vec2<f32>(0.89713400602340698f, -0.19723199307918549f), vec2<f32>(-0.54750698804855347f, 0.77877199649810791f), vec2<f32>(-0.12648700177669525f, -0.97609001398086548f) );
var<private> SHADOW_ROTATIONS_0 : array<vec2<f32>, i32(16)> = array<vec2<f32>, i32(16)>( vec2<f32>(1.0f, 0.0f), vec2<f32>(0.92387998104095459f, 0.38268300890922546f), vec2<f32>(0.70710700750350952f, 0.70710700750350952f), vec2<f32>(0.38268300890922546f, 0.92387998104095459f), vec2<f32>(0.0f, 1.0f), vec2<f32>(-0.38268300890922546f, 0.92387998104095459f), vec2<f32>(-0.70710700750350952f, 0.70710700750350952f), vec2<f32>(-0.92387998104095459f, 0.38268300890922546f), vec2<f32>(-1.0f, 0.0f), vec2<f32>(-0.92387998104095459f, -0.38268300890922546f), vec2<f32>(-0.70710700750350952f, -0.70710700750350952f), vec2<f32>(-0.38268300890922546f, -0.92387998104095459f), vec2<f32>(-0.0f, -1.0f), vec2<f32>(0.38268300890922546f, -0.92387998104095459f), vec2<f32>(0.70710700750350952f, -0.70710700750350952f), vec2<f32>(0.92387998104095459f, -0.38268300890922546f) );
var<private> SHADOW_DITHER_0 : array<u32, i32(16)> = array<u32, i32(16)>( u32(0), u32(8), u32(2), u32(10), u32(12), u32(4), u32(14), u32(6), u32(3), u32(11), u32(1), u32(9), u32(15), u32(7), u32(13), u32(5) );
fn rsqrt_0( x_0 : f32) -> f32
{
    return 1.0f / sqrt(x_0);
}

fn load_position_0( at_0 : u32) -> vec3<f32>
{
    var word_0 : u32 = at_0 * u32(3);
    return vec3<f32>((bitcast<f32>((vertices_0[word_0]))), (bitcast<f32>((vertices_0[word_0 + u32(1)]))), (bitcast<f32>((vertices_0[word_0 + u32(2)]))));
}

fn dequantise_snorm_0( lane_0 : i32) -> f32
{
    return max(f32(lane_0) / 32767.0f, -1.0f);
}

fn unpack_snorm16x4_0( low_0 : u32,  high_0 : u32) -> vec4<f32>
{
    return vec4<f32>(dequantise_snorm_0(((bitcast<i32>(((low_0 << (u32(16)))))) >> (u32(16)))), dequantise_snorm_0(((bitcast<i32>((low_0))) >> (u32(16)))), dequantise_snorm_0(((bitcast<i32>(((high_0 << (u32(16)))))) >> (u32(16)))), dequantise_snorm_0(((bitcast<i32>((high_0))) >> (u32(16)))));
}

fn rotate_by_0( q_0 : vec4<f32>,  v_0 : vec3<f32>) -> vec3<f32>
{
    var _S1 : vec3<f32> = q_0.xyz;
    var t_0 : vec3<f32> = vec3<f32>(2.0f) * cross(_S1, v_0);
    return v_0 + vec3<f32>(q_0.w) * t_0 + cross(_S1, t_0);
}

struct TangentFrame_0
{
     tangent_1 : vec3<f32>,
     bitangent_0 : vec3<f32>,
     normal_0 : vec3<f32>,
};

fn decode_qtangent_0( lanes_0 : vec4<f32>) -> TangentFrame_0
{
    var q_1 : vec4<f32> = normalize(lanes_0);
    var basis_0 : TangentFrame_0;
    var _S2 : vec3<f32> = rotate_by_0(q_1, vec3<f32>(1.0f, 0.0f, 0.0f));
    basis_0.tangent_1 = _S2;
    var _S3 : vec3<f32> = rotate_by_0(q_1, vec3<f32>(0.0f, 0.0f, 1.0f));
    basis_0.normal_0 = _S3;
    var _S4 : vec3<f32> = cross(_S3, _S2);
    var _S5 : f32;
    if((lanes_0.w) < 0.0f)
    {
        _S5 = -1.0f;
    }
    else
    {
        _S5 = 1.0f;
    }
    basis_0.bitangent_0 = _S4 * vec3<f32>(_S5);
    return basis_0;
}

fn unpack_unorm16x2_0( word_1 : u32) -> vec2<f32>
{
    return vec2<f32>(f32((word_1 & (u32(65535)))), f32((word_1 >> (u32(16))))) / vec2<f32>(65535.0f);
}

fn unpack_rgba8_0( word_2 : u32) -> vec4<f32>
{
    return vec4<f32>(f32((word_2 & (u32(255)))), f32((((word_2 >> (u32(8)))) & (u32(255)))), f32((((word_2 >> (u32(16)))) & (u32(255)))), f32((word_2 >> (u32(24))))) / vec4<f32>(255.0f);
}

struct MeshVertex_0
{
     position_1 : vec3<f32>,
     basis_1 : TangentFrame_0,
     uv0_0 : vec2<f32>,
     color_1 : vec4<f32>,
};

fn load_vertex_0( at_1 : u32,  range_0 : vec4<f32>) -> MeshVertex_0
{
    var word_3 : u32 = frame_0.vertex_pool_0.x + at_1 * u32(5);
    var vertex_0 : MeshVertex_0;
    vertex_0.position_1 = load_position_0(at_1);
    vertex_0.basis_1 = decode_qtangent_0(unpack_snorm16x4_0(vertices_0[word_3], vertices_0[word_3 + u32(1)]));
    vertex_0.uv0_0 = range_0.zw + range_0.xy * unpack_unorm16x2_0(vertices_0[word_3 + u32(2)]);
    vertex_0.color_1 = unpack_rgba8_0(vertices_0[word_3 + u32(4)]);
    return vertex_0;
}

fn normal_basis_0( basis_2 : mat3x3<f32>) -> mat3x3<f32>
{
    return mat3x3<f32>(cross(basis_2[i32(1)], basis_2[i32(2)]), cross(basis_2[i32(2)], basis_2[i32(0)]), cross(basis_2[i32(0)], basis_2[i32(1)]));
}

fn frame_word_0( mesh_flags_0 : u32,  basis_3 : TangentFrame_0) -> u32
{
    var word_4 : u32;
    if(((mesh_flags_0 & (u32(1)))) != u32(0))
    {
        word_4 = u32(1);
    }
    else
    {
        word_4 = u32(0);
    }
    if((dot(cross(basis_3.normal_0, basis_3.tangent_1), basis_3.bitangent_0)) < 0.0f)
    {
        word_4 = (word_4 | (u32(2)));
    }
    return word_4;
}

struct VertexOutput_0
{
    @builtin(position) position_2 : vec4<f32>,
    @location(0) world_position_0 : vec3<f32>,
    @location(6) world_normal_0 : vec3<f32>,
    @location(7) color_2 : vec4<f32>,
    @interpolate(flat) @location(8) material_1 : u32,
    @location(1) uv_0 : vec2<f32>,
    @location(2) clip_position_0 : vec4<f32>,
    @location(3) previous_clip_position_0 : vec4<f32>,
    @location(4) world_tangent_0 : vec3<f32>,
    @interpolate(flat) @location(5) frame_1 : u32,
};

@vertex
fn vertexMain(@builtin(vertex_index) index_0 : u32, @builtin(instance_index) instance_id_0 : u32) -> VertexOutput_0
{
    var mesh_2 : GpuMesh_std430_0 = meshes_0[draw_0.mesh_0];
    var _S6 : bool = (((instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].flags_0) & (u32(2)))) != u32(0);
    var base_vertex_2 : u32;
    if(_S6)
    {
        base_vertex_2 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].base_vertex_0;
    }
    else
    {
        base_vertex_2 = mesh_2.base_vertex_1;
    }
    var vertex_1 : MeshVertex_0 = load_vertex_0(index_0 + base_vertex_2, vec4<f32>(mesh_2.uv_scale_u_0, mesh_2.uv_scale_v_0, mesh_2.uv_offset_u_0, mesh_2.uv_offset_v_0));
    var previous_base_0 : u32;
    if(_S6)
    {
        previous_base_0 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_base_vertex_0;
    }
    else
    {
        previous_base_0 = base_vertex_2;
    }
    var previous_position_0 : vec3<f32> = load_position_0(index_0 + previous_base_0);
    var _S7 : mat4x4<f32> = mat4x4<f32>(instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(0)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(1)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(2)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].transform_0.data_0[i32(3)][i32(3)]);
    var world_0 : vec4<f32> = (((vec4<f32>(vertex_1.position_1, 1.0f)) * (_S7)));
    var output_0 : VertexOutput_0;
    output_0.position_2 = (((world_0) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)]))));
    output_0.world_position_0 = world_0.xyz;
    var _S8 : mat3x3<f32> = mat3x3<f32>(_S7[i32(0)].xyz, _S7[i32(1)].xyz, _S7[i32(2)].xyz);
    output_0.world_normal_0 = (((vertex_1.basis_1.normal_0) * (normal_basis_0(_S8))));
    output_0.world_tangent_0 = (((vertex_1.basis_1.tangent_1) * (_S8)));
    output_0.frame_1 = frame_word_0(mesh_2.flags_1, vertex_1.basis_1);
    var _S9 : vec4<f32>;
    if((frame_0.ambient_0.w) >= 1.5f)
    {
        _S9 = vec4<f32>(0.44999998807907104f, 0.44999998807907104f, 0.47999998927116394f, 1.0f);
    }
    else
    {
        _S9 = vertex_1.color_1;
    }
    output_0.color_2 = _S9;
    output_0.material_1 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].material_0;
    output_0.uv_0 = vertex_1.uv0_0;
    output_0.clip_position_0 = output_0.position_2;
    output_0.previous_clip_position_0 = ((((((vec4<f32>(previous_position_0, 1.0f)) * (mat4x4<f32>(instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(0)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(1)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(2)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_0]].previous_transform_0.data_0[i32(3)][i32(3)]))))) * (mat4x4<f32>(frame_0.previous_view_proj_0.data_1[i32(0)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(0)], frame_0.previous_view_proj_0.data_1[i32(0)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(1)], frame_0.previous_view_proj_0.data_1[i32(0)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(2)], frame_0.previous_view_proj_0.data_1[i32(0)][i32(3)], frame_0.previous_view_proj_0.data_1[i32(1)][i32(3)], frame_0.previous_view_proj_0.data_1[i32(2)][i32(3)], frame_0.previous_view_proj_0.data_1[i32(3)][i32(3)]))));
    return output_0;
}

struct vertexOutput_0
{
    @builtin(position) output_1 : vec4<f32>,
};

@vertex
fn depthVertexMain(@builtin(vertex_index) index_1 : u32, @builtin(instance_index) instance_id_1 : u32) -> vertexOutput_0
{
    var mesh_3 : GpuMesh_std430_0 = meshes_0[draw_0.mesh_0];
    var base_vertex_3 : u32;
    if((((instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].flags_0) & (u32(2)))) != u32(0))
    {
        base_vertex_3 = instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].base_vertex_0;
    }
    else
    {
        base_vertex_3 = mesh_3.base_vertex_1;
    }
    var _S10 : vertexOutput_0 = vertexOutput_0( ((((((vec4<f32>(load_position_0(index_1 + base_vertex_3), 1.0f)) * (mat4x4<f32>(instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(0)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(1)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(2)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(3)][i32(0)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(0)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(1)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(2)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(3)][i32(1)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(0)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(1)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(2)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(3)][i32(2)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(0)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(1)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(2)][i32(3)], instances_0[visible_instances_0[draw_0.base_0 + instance_id_1]].transform_0.data_0[i32(3)][i32(3)]))))) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)])))) );
    return _S10;
}

struct vertexOutput_1
{
    @builtin(position) output_2 : vec4<f32>,
};

@vertex
fn depthClearVertexMain(@builtin(vertex_index) index_2 : u32) -> vertexOutput_1
{
    var _S11 : vertexOutput_1 = vertexOutput_1( vec4<f32>(vec2<f32>(f32((((index_2 << (u32(1)))) & (u32(2)))), f32((index_2 & (u32(2))))) * vec2<f32>(2.0f, -2.0f) + vec2<f32>(-1.0f, 1.0f), 0.0f, 1.0f) );
    return _S11;
}

fn motion_vector_0( current_0 : vec4<f32>,  previous_0 : vec4<f32>) -> vec2<f32>
{
    var _S12 : f32 = previous_0.w;
    if(_S12 <= 0.0f)
    {
        return vec2<f32>(0.0f, 0.0f);
    }
    return (current_0.xy / vec2<f32>(current_0.w) - previous_0.xy / vec2<f32>(_S12)) * vec2<f32>(0.5f, -0.5f);
}

fn occlusion_at_0( position_3 : vec2<f32>) -> vec4<f32>
{
    var width_0 : u32;
    var height_0 : u32;
    {var dim = textureDimensions((ambient_occlusion_0));((width_0)) = dim.x;((height_0)) = dim.y;};
    var _S13 : vec3<i32> = vec3<i32>(min(vec2<i32>(position_3), vec2<i32>(i32(width_0), i32(height_0)) - vec2<i32>(i32(1))), i32(0));
    return (textureLoad((ambient_occlusion_0), ((_S13)).xy, ((_S13)).z));
}

fn physical_tile_uv_0( world_position_1 : vec3<f32>,  normal_1 : vec3<f32>,  tile_metres_1 : f32) -> vec2<f32>
{
    var axis_0 : vec3<f32> = abs(normal_1);
    var _S14 : f32 = axis_0.x;
    var _S15 : f32 = axis_0.y;
    var _S16 : bool;
    if(_S14 >= _S15)
    {
        _S16 = _S14 >= (axis_0.z);
    }
    else
    {
        _S16 = false;
    }
    var planar_0 : vec2<f32>;
    if(_S16)
    {
        planar_0 = world_position_1.zy;
    }
    else
    {
        if(_S15 >= (axis_0.z))
        {
            planar_0 = world_position_1.xz;
        }
        else
        {
            planar_0 = world_position_1.xy;
        }
    }
    return planar_0 / vec2<f32>(max(tile_metres_1, 0.00009999999747379f));
}

fn normal_layer_0( material_2 : ptr<function, GpuMaterial_std430_0>) -> u32
{
    return (((*material_2).color_normal_pages_0) >> (u32(16)));
}

fn orthonormal_tangent_0( normal_2 : vec3<f32>) -> vec3<f32>
{
    var _S17 : f32 = normal_2.z;
    var sign_z_0 : f32;
    if(_S17 >= 0.0f)
    {
        sign_z_0 = 1.0f;
    }
    else
    {
        sign_z_0 = -1.0f;
    }
    var a_0 : f32 = -1.0f / (sign_z_0 + _S17);
    var _S18 : f32 = normal_2.x;
    var _S19 : f32 = sign_z_0 * _S18;
    return vec3<f32>(1.0f + _S19 * _S18 * a_0, _S19 * normal_2.y * a_0, - sign_z_0 * _S18);
}

fn derivative_frame_0( dpdx_0 : vec3<f32>,  dpdy_0 : vec3<f32>,  duvdx_0 : vec2<f32>,  duvdy_0 : vec2<f32>,  normal_3 : vec3<f32>) -> TangentFrame_0
{
    var _S20 : f32 = duvdy_0.y;
    var _S21 : f32 = duvdx_0.y;
    var winding_0 : f32;
    if((duvdx_0.x * _S20 - duvdy_0.x * _S21) < 0.0f)
    {
        winding_0 = -1.0f;
    }
    else
    {
        winding_0 = 1.0f;
    }
    var tangent_2 : vec3<f32> = (vec3<f32>(_S20) * dpdx_0 - vec3<f32>(_S21) * dpdy_0) * vec3<f32>(winding_0);
    var basis_4 : TangentFrame_0;
    basis_4.normal_0 = normal_3;
    var tangent_3 : vec3<f32> = tangent_2 - normal_3 * vec3<f32>(dot(normal_3, tangent_2));
    var length_squared_0 : f32 = dot(tangent_3, tangent_3);
    var _S22 : vec3<f32>;
    if(length_squared_0 > 1.00000001686238353e-16f)
    {
        _S22 = tangent_3 * vec3<f32>(rsqrt_0(length_squared_0));
    }
    else
    {
        _S22 = orthonormal_tangent_0(normal_3);
    }
    basis_4.tangent_1 = _S22;
    basis_4.bitangent_0 = cross(normal_3, _S22);
    return basis_4;
}

fn shading_normal_of_0( layer_0 : u32,  normal_scale_1 : f32,  input_0 : VertexOutput_0,  normal_4 : vec3<f32>,  uv_1 : vec2<f32>) -> vec3<f32>
{
    var dpdx_1 : vec3<f32> = dpdx(input_0.world_position_0);
    var dpdy_1 : vec3<f32> = dpdy(input_0.world_position_0);
    var duvdx_1 : vec2<f32> = dpdx(uv_1);
    var duvdy_1 : vec2<f32> = dpdy(uv_1);
    if(layer_0 == u32(0))
    {
        return normal_4;
    }
    var basis_5 : TangentFrame_0;
    if((((input_0.frame_1) & (u32(1)))) != u32(0))
    {
        basis_5.normal_0 = normal_4;
        var tangent_4 : vec3<f32> = input_0.world_tangent_0 - normal_4 * vec3<f32>(dot(normal_4, input_0.world_tangent_0));
        var length_squared_1 : f32 = dot(tangent_4, tangent_4);
        var _S23 : vec3<f32>;
        if(length_squared_1 > 1.00000001686238353e-16f)
        {
            _S23 = tangent_4 * vec3<f32>(rsqrt_0(length_squared_1));
        }
        else
        {
            _S23 = orthonormal_tangent_0(normal_4);
        }
        basis_5.tangent_1 = _S23;
        var _S24 : vec3<f32> = cross(basis_5.normal_0, _S23);
        var _S25 : f32;
        if((((input_0.frame_1) & (u32(2)))) != u32(0))
        {
            _S25 = -1.0f;
        }
        else
        {
            _S25 = 1.0f;
        }
        basis_5.bitangent_0 = _S24 * vec3<f32>(_S25);
    }
    else
    {
        basis_5 = derivative_frame_0(dpdx_1, dpdy_1, duvdx_1, duvdy_1, normal_4);
    }
    var _S26 : vec3<f32> = vec3<f32>(uv_1, f32(layer_0));
    var _S27 : vec3<f32> = (textureSampleGrad((normal_textures_0), (base_color_sampler_0), ((_S26)).xy, i32(((_S26)).z), (duvdx_1), (duvdy_1))).xyz * vec3<f32>(2.0f) - vec3<f32>(1.0f);
    var tangent_space_0 : vec3<f32> = _S27;
    var _S28 : vec2<f32> = _S27.xy * vec2<f32>(normal_scale_1);
    tangent_space_0.x = _S28.x;
    tangent_space_0.y = _S28.y;
    var _S29 : vec3<f32> = normalize(tangent_space_0);
    tangent_space_0 = _S29;
    return normalize(vec3<f32>(_S29.x) * basis_5.tangent_1 + vec3<f32>(_S29.y) * basis_5.bitangent_0 + vec3<f32>(_S29.z) * basis_5.normal_0);
}

fn geometric_normal_of_0( world_position_2 : vec3<f32>,  shading_normal_0 : vec3<f32>) -> vec3<f32>
{
    var facet_0 : vec3<f32> = cross(dpdx(world_position_2), dpdy(world_position_2));
    var extent_0 : f32 = length(facet_0);
    if(extent_0 < 9.999999960041972e-13f)
    {
        return shading_normal_0;
    }
    var facet_1 : vec3<f32> = facet_0 / vec3<f32>(extent_0);
    var _S30 : vec3<f32>;
    if((dot(facet_1, shading_normal_0)) < 0.0f)
    {
        _S30 = (vec3<f32>(0) - facet_1);
    }
    else
    {
        _S30 = facet_1;
    }
    return _S30;
}

fn base_color_layer_0( material_3 : ptr<function, GpuMaterial_std430_0>) -> u32
{
    return (((*material_3).color_normal_pages_0) & (u32(65535)));
}

fn froxel_of_0( pixel_0 : vec2<f32>,  depth_0 : f32) -> u32
{
    var _S31 : u32 = max(frame_0.cluster_grid_0.x, u32(1));
    var _S32 : u32 = max(frame_0.cluster_grid_0.y, u32(1));
    var _S33 : u32 = max(frame_0.cluster_grid_0.z, u32(1));
    var _S34 : u32 = max(frame_0.cluster_grid_0.w, u32(1));
    var _S35 : u32 = u32(pixel_0.x) / _S34;
    var _S36 : u32 = min(_S35, _S31 - u32(1));
    var _S37 : u32 = u32(pixel_0.y) / _S34;
    var scale_0 : f32 = 24.0f / log2(10000.0f);
    return (u32(clamp(floor(log2(max(depth_0, 0.10000000149011612f)) * scale_0 + - scale_0 * log2(0.10000000149011612f)), 0.0f, f32(_S33 - u32(1)))) * _S32 + min(_S37, _S32 - u32(1))) * _S31 + _S36;
}

struct TableTap_0
{
     lo_0 : vec2<i32>,
     hi_0 : vec2<i32>,
     weight_0 : vec2<f32>,
};

fn table_tap_0( n_dot_v_0 : f32,  roughness_1 : f32) -> TableTap_0
{
    var width_1 : u32;
    var height_1 : u32;
    {var dim = textureDimensions((specular_dfg_0));((width_1)) = dim.x;((height_1)) = dim.y;};
    var extent_1 : vec2<f32> = vec2<f32>(f32(width_1), f32(height_1));
    var scaled_0 : vec2<f32> = vec2<f32>(saturate(n_dot_v_0), saturate(roughness_1)) * extent_1 - vec2<f32>(0.5f);
    var _S38 : vec2<f32> = vec2<f32>(1.0f);
    var _S39 : vec2<f32> = extent_1 - _S38;
    var low_1 : vec2<f32> = clamp(floor(scaled_0), vec2<f32>(0.0f, 0.0f), _S39);
    var high_1 : vec2<f32> = min(low_1 + _S38, _S39);
    var tap_0 : TableTap_0;
    tap_0.lo_0 = vec2<i32>(low_1);
    tap_0.hi_0 = vec2<i32>(high_1);
    tap_0.weight_0 = clamp(scaled_0 - low_1, vec2<f32>(0.0f), vec2<f32>(1.0f));
    return tap_0;
}

fn decode_dfg_pair_0( texel_0 : vec4<f32>) -> vec2<f32>
{
    return vec2<f32>(texel_0.x * 65280.0f + texel_0.y * 255.0f, texel_0.z * 65280.0f + texel_0.w * 255.0f) / vec2<f32>(65535.0f);
}

fn dfg_at_0( tap_1 : TableTap_0) -> vec2<f32>
{
    var _S40 : i32 = tap_1.lo_0.x;
    var _S41 : i32 = tap_1.lo_0.y;
    var _S42 : vec3<i32> = vec3<i32>(_S40, _S41, i32(0));
    var _S43 : i32 = tap_1.hi_0.x;
    var _S44 : vec3<i32> = vec3<i32>(_S43, _S41, i32(0));
    var _S45 : vec2<f32> = vec2<f32>(tap_1.weight_0.x);
    var _S46 : i32 = tap_1.hi_0.y;
    var _S47 : vec3<i32> = vec3<i32>(_S40, _S46, i32(0));
    var _S48 : vec3<i32> = vec3<i32>(_S43, _S46, i32(0));
    return mix(mix(decode_dfg_pair_0((textureLoad((specular_dfg_0), ((_S42)).xy, ((_S42)).z))), decode_dfg_pair_0((textureLoad((specular_dfg_0), ((_S44)).xy, ((_S44)).z))), _S45), mix(decode_dfg_pair_0((textureLoad((specular_dfg_0), ((_S47)).xy, ((_S47)).z))), decode_dfg_pair_0((textureLoad((specular_dfg_0), ((_S48)).xy, ((_S48)).z))), _S45), vec2<f32>(tap_1.weight_0.y));
}

fn range_window_0( distance_0 : f32,  radius_0 : f32) -> f32
{
    var ratio_0 : f32 = distance_0 / max(radius_0, 9.99999997475242708e-07f);
    var window_0 : f32 = saturate(1.0f - ratio_0 * ratio_0 * ratio_0 * ratio_0);
    return window_0 * window_0;
}

fn punctual_falloff_0( distance_1 : f32,  radius_1 : f32) -> f32
{
    return range_window_0(distance_1, radius_1) / (distance_1 * distance_1 + 1.0f);
}

fn spot_cone_0( to_light_0 : vec3<f32>,  axis_1 : vec3<f32>,  cos_outer_0 : f32,  cos_inner_1 : f32) -> f32
{
    return saturate((dot((vec3<f32>(0) - to_light_0), normalize(axis_1)) - cos_outer_0) / max(cos_inner_1 - cos_outer_0, 0.00009999999747379f));
}

fn rect_corners_0( light_0 : ptr<function, GpuLight_std430_0>,  world_position_3 : vec3<f32>,  corners_0 : ptr<function, array<vec3<f32>, i32(4)>>)
{
    var _S49 : vec3<f32> = (*light_0).tangent_0.xyz;
    var across_0 : vec3<f32> = _S49 * vec3<f32>((*light_0).tangent_0.w);
    var down_0 : vec3<f32> = cross(_S49, (*light_0).direction_0.xyz) * vec3<f32>((*light_0).direction_0.w);
    var centre_0 : vec3<f32> = (*light_0).position_0.xyz - world_position_3;
    var _S50 : vec3<f32> = centre_0 - across_0;
    (*corners_0)[i32(0)] = _S50 - down_0;
    var _S51 : vec3<f32> = centre_0 + across_0;
    (*corners_0)[i32(1)] = _S51 - down_0;
    (*corners_0)[i32(2)] = _S51 + down_0;
    (*corners_0)[i32(3)] = _S50 + down_0;
    return;
}

fn ltc_shading_frame_0( normal_5 : vec3<f32>,  to_eye_0 : vec3<f32>,  n_dot_v_1 : f32) -> mat3x3<f32>
{
    var across_1 : vec3<f32> = to_eye_0 - normal_5 * vec3<f32>(n_dot_v_1);
    var span_0 : f32 = length(across_1);
    var seed_0 : vec3<f32>;
    if((abs(normal_5.z)) < 0.89999997615814209f)
    {
        seed_0 = vec3<f32>(0.0f, 0.0f, 1.0f);
    }
    else
    {
        seed_0 = vec3<f32>(1.0f, 0.0f, 0.0f);
    }
    var tangent_5 : vec3<f32>;
    if(span_0 > 0.00009999999747379f)
    {
        tangent_5 = across_1 / vec3<f32>(span_0);
    }
    else
    {
        tangent_5 = normalize(cross(seed_0, normal_5));
    }
    return mat3x3<f32>(tangent_5, cross(normal_5, tangent_5), normal_5);
}

struct LtcPolygon_0
{
     corner_0 : array<vec3<f32>, i32(5)>,
     count_0 : i32,
};

fn ltc_clip_0( polygon_0 : LtcPolygon_0) -> LtcPolygon_0
{
    const _S52 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var _S53 : f32 = polygon_0.corner_0[i32(0)].z;
    var count_1 : i32;
    if(_S53 > 0.0f)
    {
        count_1 = i32(1);
    }
    else
    {
        count_1 = i32(0);
    }
    var _S54 : f32 = polygon_0.corner_0[i32(1)].z;
    var _S55 : i32;
    if(_S54 > 0.0f)
    {
        _S55 = i32(2);
    }
    else
    {
        _S55 = i32(0);
    }
    var config_0 : i32 = count_1 + _S55;
    var _S56 : f32 = polygon_0.corner_0[i32(2)].z;
    if(_S56 > 0.0f)
    {
        count_1 = i32(4);
    }
    else
    {
        count_1 = i32(0);
    }
    var config_1 : i32 = config_0 + count_1;
    var _S57 : f32 = polygon_0.corner_0[i32(3)].z;
    if(_S57 > 0.0f)
    {
        count_1 = i32(8);
    }
    else
    {
        count_1 = i32(0);
    }
    var config_2 : i32 = config_1 + count_1;
    var l0_0 : vec3<f32>;
    var l1_0 : vec3<f32>;
    var l2_0 : vec3<f32>;
    var l3_0 : vec3<f32>;
    var l4_0 : vec3<f32>;
    if(config_2 == i32(1))
    {
        var _S58 : vec3<f32> = vec3<f32>(_S53);
        var _S59 : vec3<f32> = vec3<f32>(- _S54) * polygon_0.corner_0[i32(0)] + _S58 * polygon_0.corner_0[i32(1)];
        var _S60 : vec3<f32> = vec3<f32>(- _S57) * polygon_0.corner_0[i32(0)] + _S58 * polygon_0.corner_0[i32(3)];
        count_1 = i32(3);
        l0_0 = polygon_0.corner_0[i32(0)];
        l1_0 = _S59;
        l2_0 = _S60;
        l3_0 = polygon_0.corner_0[i32(3)];
        l4_0 = _S52;
    }
    else
    {
        if(config_2 == i32(2))
        {
            var _S61 : vec3<f32> = vec3<f32>(_S54);
            var _S62 : vec3<f32> = vec3<f32>(- _S53) * polygon_0.corner_0[i32(1)] + _S61 * polygon_0.corner_0[i32(0)];
            var _S63 : vec3<f32> = vec3<f32>(- _S56) * polygon_0.corner_0[i32(1)] + _S61 * polygon_0.corner_0[i32(2)];
            count_1 = i32(3);
            l0_0 = _S62;
            l1_0 = polygon_0.corner_0[i32(1)];
            l2_0 = _S63;
            l3_0 = polygon_0.corner_0[i32(3)];
            l4_0 = _S52;
        }
        else
        {
            if(config_2 == i32(3))
            {
                var _S64 : vec3<f32> = vec3<f32>(- _S56) * polygon_0.corner_0[i32(1)] + vec3<f32>(_S54) * polygon_0.corner_0[i32(2)];
                var _S65 : vec3<f32> = vec3<f32>(- _S57) * polygon_0.corner_0[i32(0)] + vec3<f32>(_S53) * polygon_0.corner_0[i32(3)];
                count_1 = i32(4);
                l0_0 = polygon_0.corner_0[i32(0)];
                l1_0 = polygon_0.corner_0[i32(1)];
                l2_0 = _S64;
                l3_0 = _S65;
                l4_0 = _S52;
            }
            else
            {
                if(config_2 == i32(4))
                {
                    var _S66 : vec3<f32> = vec3<f32>(_S56);
                    var _S67 : vec3<f32> = vec3<f32>(- _S57) * polygon_0.corner_0[i32(2)] + _S66 * polygon_0.corner_0[i32(3)];
                    var _S68 : vec3<f32> = vec3<f32>(- _S54) * polygon_0.corner_0[i32(2)] + _S66 * polygon_0.corner_0[i32(1)];
                    count_1 = i32(3);
                    l0_0 = _S67;
                    l1_0 = _S68;
                    l2_0 = polygon_0.corner_0[i32(2)];
                    l3_0 = polygon_0.corner_0[i32(3)];
                    l4_0 = _S52;
                }
                else
                {
                    if(config_2 == i32(6))
                    {
                        var _S69 : vec3<f32> = vec3<f32>(- _S53) * polygon_0.corner_0[i32(1)] + vec3<f32>(_S54) * polygon_0.corner_0[i32(0)];
                        var _S70 : vec3<f32> = vec3<f32>(- _S57) * polygon_0.corner_0[i32(2)] + vec3<f32>(_S56) * polygon_0.corner_0[i32(3)];
                        count_1 = i32(4);
                        l0_0 = _S69;
                        l1_0 = polygon_0.corner_0[i32(1)];
                        l2_0 = polygon_0.corner_0[i32(2)];
                        l3_0 = _S70;
                        l4_0 = _S52;
                    }
                    else
                    {
                        if(config_2 == i32(7))
                        {
                            var _S71 : vec3<f32> = vec3<f32>(- _S57);
                            var _S72 : vec3<f32> = _S71 * polygon_0.corner_0[i32(0)] + vec3<f32>(_S53) * polygon_0.corner_0[i32(3)];
                            var _S73 : vec3<f32> = _S71 * polygon_0.corner_0[i32(2)] + vec3<f32>(_S56) * polygon_0.corner_0[i32(3)];
                            count_1 = i32(5);
                            l0_0 = polygon_0.corner_0[i32(0)];
                            l1_0 = polygon_0.corner_0[i32(1)];
                            l2_0 = polygon_0.corner_0[i32(2)];
                            l3_0 = _S73;
                            l4_0 = _S72;
                        }
                        else
                        {
                            if(config_2 == i32(8))
                            {
                                var _S74 : vec3<f32> = vec3<f32>(_S57);
                                var _S75 : vec3<f32> = vec3<f32>(- _S53) * polygon_0.corner_0[i32(3)] + _S74 * polygon_0.corner_0[i32(0)];
                                var _S76 : vec3<f32> = vec3<f32>(- _S56) * polygon_0.corner_0[i32(3)] + _S74 * polygon_0.corner_0[i32(2)];
                                count_1 = i32(3);
                                l0_0 = _S75;
                                l1_0 = _S76;
                                l2_0 = polygon_0.corner_0[i32(3)];
                                l3_0 = polygon_0.corner_0[i32(3)];
                                l4_0 = _S52;
                            }
                            else
                            {
                                if(config_2 == i32(9))
                                {
                                    var _S77 : vec3<f32> = vec3<f32>(- _S54) * polygon_0.corner_0[i32(0)] + vec3<f32>(_S53) * polygon_0.corner_0[i32(1)];
                                    var _S78 : vec3<f32> = vec3<f32>(- _S56) * polygon_0.corner_0[i32(3)] + vec3<f32>(_S57) * polygon_0.corner_0[i32(2)];
                                    count_1 = i32(4);
                                    l0_0 = polygon_0.corner_0[i32(0)];
                                    l1_0 = _S77;
                                    l2_0 = _S78;
                                    l3_0 = polygon_0.corner_0[i32(3)];
                                    l4_0 = _S52;
                                }
                                else
                                {
                                    if(config_2 == i32(11))
                                    {
                                        var _S79 : vec3<f32> = vec3<f32>(- _S57) * polygon_0.corner_0[i32(2)] + vec3<f32>(_S56) * polygon_0.corner_0[i32(3)];
                                        var _S80 : vec3<f32> = vec3<f32>(- _S56) * polygon_0.corner_0[i32(1)] + vec3<f32>(_S54) * polygon_0.corner_0[i32(2)];
                                        count_1 = i32(5);
                                        l0_0 = polygon_0.corner_0[i32(0)];
                                        l1_0 = polygon_0.corner_0[i32(1)];
                                        l2_0 = _S80;
                                        l3_0 = _S79;
                                        l4_0 = polygon_0.corner_0[i32(3)];
                                    }
                                    else
                                    {
                                        if(config_2 == i32(12))
                                        {
                                            var _S81 : vec3<f32> = vec3<f32>(- _S54) * polygon_0.corner_0[i32(2)] + vec3<f32>(_S56) * polygon_0.corner_0[i32(1)];
                                            var _S82 : vec3<f32> = vec3<f32>(- _S53) * polygon_0.corner_0[i32(3)] + vec3<f32>(_S57) * polygon_0.corner_0[i32(0)];
                                            count_1 = i32(4);
                                            l0_0 = _S82;
                                            l1_0 = _S81;
                                            l2_0 = polygon_0.corner_0[i32(2)];
                                            l3_0 = polygon_0.corner_0[i32(3)];
                                            l4_0 = _S52;
                                        }
                                        else
                                        {
                                            if(config_2 == i32(13))
                                            {
                                                var _S83 : vec3<f32> = vec3<f32>(- _S56) * polygon_0.corner_0[i32(1)] + vec3<f32>(_S54) * polygon_0.corner_0[i32(2)];
                                                var _S84 : vec3<f32> = vec3<f32>(- _S54) * polygon_0.corner_0[i32(0)] + vec3<f32>(_S53) * polygon_0.corner_0[i32(1)];
                                                count_1 = i32(5);
                                                l0_0 = polygon_0.corner_0[i32(0)];
                                                l1_0 = _S84;
                                                l2_0 = _S83;
                                                l3_0 = polygon_0.corner_0[i32(2)];
                                                l4_0 = polygon_0.corner_0[i32(3)];
                                            }
                                            else
                                            {
                                                if(config_2 == i32(14))
                                                {
                                                    var _S85 : vec3<f32> = vec3<f32>(- _S53);
                                                    var _S86 : vec3<f32> = _S85 * polygon_0.corner_0[i32(3)] + vec3<f32>(_S57) * polygon_0.corner_0[i32(0)];
                                                    var _S87 : vec3<f32> = _S85 * polygon_0.corner_0[i32(1)] + vec3<f32>(_S54) * polygon_0.corner_0[i32(0)];
                                                    count_1 = i32(5);
                                                    l0_0 = _S87;
                                                    l1_0 = _S86;
                                                }
                                                else
                                                {
                                                    if(config_2 == i32(15))
                                                    {
                                                        count_1 = i32(4);
                                                    }
                                                    else
                                                    {
                                                        count_1 = i32(0);
                                                    }
                                                    l0_0 = polygon_0.corner_0[i32(0)];
                                                    l1_0 = _S52;
                                                }
                                                var _S88 : vec3<f32> = l1_0;
                                                l1_0 = polygon_0.corner_0[i32(1)];
                                                l2_0 = polygon_0.corner_0[i32(2)];
                                                l3_0 = polygon_0.corner_0[i32(3)];
                                                l4_0 = _S88;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if(count_1 <= i32(3))
    {
        l3_0 = l0_0;
        l4_0 = l0_0;
    }
    else
    {
        if(count_1 == i32(4))
        {
            l4_0 = l0_0;
        }
    }
    var clipped_0 : LtcPolygon_0;
    clipped_0.corner_0[i32(0)] = l0_0;
    clipped_0.corner_0[i32(1)] = l1_0;
    clipped_0.corner_0[i32(2)] = l2_0;
    clipped_0.corner_0[i32(3)] = l3_0;
    clipped_0.corner_0[i32(4)] = l4_0;
    clipped_0.count_0 = count_1;
    return clipped_0;
}

fn ltc_edge_0( first_0 : vec3<f32>,  second_0 : vec3<f32>) -> f32
{
    var cosine_0 : f32 = clamp(dot(first_0, second_0), -1.0f, 1.0f);
    var y_0 : f32 = abs(cosine_0);
    var fit_0 : f32 = (0.85439848899841309f + (0.49651551246643066f + 0.01452060043811798f * y_0) * y_0) / (3.41759395599365234f + (4.16167259216308594f + y_0) * y_0);
    var weight_1 : f32;
    if(cosine_0 > 0.0f)
    {
        weight_1 = fit_0;
    }
    else
    {
        weight_1 = 0.5f / sqrt(max(1.0f - cosine_0 * cosine_0, 1.00000001168609742e-07f)) - fit_0;
    }
    return (first_0.x * second_0.y - first_0.y * second_0.x) * weight_1;
}

fn ltc_irradiance_0( transform_1 : mat3x3<f32>,  corners_1 : array<vec3<f32>, i32(4)>) -> f32
{
    var polygon_1 : LtcPolygon_0;
    var corner_1 : i32 = i32(0);
    for(;;)
    {
        if(corner_1 < i32(4))
        {
        }
        else
        {
            break;
        }
        polygon_1.corner_0[corner_1] = (((corners_1[corner_1]) * (transform_1)));
        corner_1 = corner_1 + i32(1);
    }
    polygon_1.corner_0[i32(4)] = vec3<f32>(0.0f, 0.0f, 0.0f);
    polygon_1.count_0 = i32(4);
    polygon_1 = ltc_clip_0(polygon_1);
    if((polygon_1.count_0) == i32(0))
    {
        return 0.0f;
    }
    var at_2 : i32 = i32(0);
    for(;;)
    {
        if(at_2 < i32(5))
        {
        }
        else
        {
            break;
        }
        polygon_1.corner_0[at_2] = normalize(polygon_1.corner_0[at_2]);
        at_2 = at_2 + i32(1);
    }
    var sum_0 : f32 = ltc_edge_0(polygon_1.corner_0[i32(0)], polygon_1.corner_0[i32(1)]) + ltc_edge_0(polygon_1.corner_0[i32(1)], polygon_1.corner_0[i32(2)]) + ltc_edge_0(polygon_1.corner_0[i32(2)], polygon_1.corner_0[i32(3)]);
    var sum_1 : f32;
    if((polygon_1.count_0) >= i32(4))
    {
        sum_1 = sum_0 + ltc_edge_0(polygon_1.corner_0[i32(3)], polygon_1.corner_0[i32(4)]);
    }
    else
    {
        sum_1 = sum_0;
    }
    if((polygon_1.count_0) == i32(5))
    {
        sum_1 = sum_1 + ltc_edge_0(polygon_1.corner_0[i32(4)], polygon_1.corner_0[i32(0)]);
    }
    return max(sum_1, 0.0f) * 3.14159274101257324f;
}

fn ltc_at_0( tap_2 : TableTap_0) -> vec4<f32>
{
    var _S89 : i32 = tap_2.lo_0.x;
    var _S90 : i32 = tap_2.lo_0.y;
    var _S91 : vec3<i32> = vec3<i32>(_S89, _S90, i32(0));
    var _S92 : i32 = tap_2.hi_0.x;
    var _S93 : vec3<i32> = vec3<i32>(_S92, _S90, i32(0));
    var _S94 : vec4<f32> = vec4<f32>(tap_2.weight_0.x);
    var _S95 : i32 = tap_2.hi_0.y;
    var _S96 : vec3<i32> = vec3<i32>(_S89, _S95, i32(0));
    var _S97 : vec3<i32> = vec3<i32>(_S92, _S95, i32(0));
    return mix(mix((textureLoad((ltc_matrix_0), ((_S91)).xy, ((_S91)).z)), (textureLoad((ltc_matrix_0), ((_S93)).xy, ((_S93)).z)), _S94), mix((textureLoad((ltc_matrix_0), ((_S96)).xy, ((_S96)).z)), (textureLoad((ltc_matrix_0), ((_S97)).xy, ((_S97)).z)), _S94), vec4<f32>(tap_2.weight_0.y));
}

fn ltc_transform_0( entry_0 : vec4<f32>) -> mat3x3<f32>
{
    return mat3x3<f32>(entry_0.x, 0.0f, entry_0.y, 0.0f, 1.0f, 0.0f, entry_0.z, 0.0f, entry_0.w);
}

fn ggx_lobe_0( alpha2_0 : f32,  f0_0 : vec3<f32>,  n_dot_l_0 : f32,  n_dot_v_2 : f32,  n_dot_h_0 : f32,  v_dot_h_0 : f32) -> vec3<f32>
{
    var shape_0 : f32 = n_dot_h_0 * n_dot_h_0 * (alpha2_0 - 1.0f) + 1.0f;
    var _S98 : f32 = 1.0f - alpha2_0;
    var grazing_0 : f32 = 1.0f - v_dot_h_0;
    var grazing2_0 : f32 = grazing_0 * grazing_0;
    return vec3<f32>((alpha2_0 / max(shape_0 * shape_0, 9.99999993922529029e-09f) * (0.5f / max(n_dot_l_0 * sqrt(n_dot_v_2 * n_dot_v_2 * _S98 + alpha2_0) + n_dot_v_2 * sqrt(n_dot_l_0 * n_dot_l_0 * _S98 + alpha2_0), 9.99999997475242708e-07f)))) * (f0_0 + (vec3<f32>(1.0f, 1.0f, 1.0f) - f0_0) * vec3<f32>((grazing2_0 * grazing2_0 * grazing_0)));
}

fn atlas_rect_0( tile_0 : u32) -> vec4<f32>
{
    return frame_0.shadow_atlas_rect_0[tile_0];
}

fn atlas_rect_is_empty_0( rect_0 : vec4<f32>) -> bool
{
    return !((rect_0.x) > 0.0f);
}

fn tile_texels_0( rect_1 : vec4<f32>) -> f32
{
    return rect_1.x / frame_0.shadow_params_0.x;
}

fn shadow_normal_offset_0( geometric_normal_0 : vec3<f32>,  to_light_1 : vec3<f32>) -> f32
{
    var cosine_1 : f32 = saturate(dot(geometric_normal_0, to_light_1));
    return sqrt(saturate(1.0f - cosine_1 * cosine_1));
}

fn shadow_rotation_0( pixel_1 : vec2<f32>) -> vec2<f32>
{
    var cell_0 : vec2<u32> = (vec2<u32>(pixel_1) & (vec2<u32>(u32(3))));
    return SHADOW_ROTATIONS_0[SHADOW_DITHER_0[cell_0.y * u32(4) + cell_0.x]];
}

fn atlas_step_0( rect_2 : vec4<f32>) -> vec2<f32>
{
    return frame_0.shadow_params_0.xy / rect_2.xy;
}

fn atlas_uv_0( rect_3 : vec4<f32>,  tile_uv_0 : vec2<f32>) -> vec2<f32>
{
    return rect_3.zw + tile_uv_0 * rect_3.xy;
}

fn sun_penumbra_texels_0( cascade_0 : u32,  tile_uv_1 : vec2<f32>,  reference_0 : f32,  rotation_0 : vec2<f32>) -> f32
{
    var rect_4 : vec4<f32> = atlas_rect_0(cascade_0);
    var texel_step_0 : vec2<f32> = atlas_step_0(rect_4);
    var _S99 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_step_0;
    const _S100 : vec2<f32> = vec2<f32>(1.0f, 1.0f);
    var _S101 : vec2<f32> = _S100 / frame_0.shadow_params_0.xy;
    var index_3 : u32 = u32(0);
    var sum_2 : f32 = 0.0f;
    var found_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_3 < u32(16))
        {
        }
        else
        {
            break;
        }
        var spoke_0 : vec2<f32> = SHADOW_SEARCH_DISC_0[index_3] * vec2<f32>(8.0f);
        var _S102 : f32 = spoke_0.x;
        var _S103 : f32 = rotation_0.x;
        var _S104 : f32 = spoke_0.y;
        var _S105 : f32 = rotation_0.y;
        var _S106 : vec3<i32> = vec3<i32>(vec2<i32>(min(atlas_uv_0(rect_4, clamp(tile_uv_1 + vec2<f32>(_S102 * _S103 - _S104 * _S105, _S102 * _S105 + _S104 * _S103) * texel_step_0, _S99, vec2<f32>(1.0f) - _S99)) * _S101, _S101 - _S100)), i32(0));
        var depth_1 : f32 = (textureLoad((shadow_atlas_0), ((_S106)).xy, ((_S106)).z));
        if(depth_1 > reference_0)
        {
            var found_1 : f32 = found_0 + 1.0f;
            sum_2 = sum_2 + depth_1;
            found_0 = found_1;
        }
        index_3 = index_3 + u32(1);
    }
    if(found_0 <= 0.0f)
    {
        return 2.0f;
    }
    var _S107 : f32 = 2.0f * frame_0.cascade_far_0[cascade_0];
    return clamp((sum_2 / found_0 - reference_0) * (_S107 + 40.0f) * 0.01999999955296516f / (_S107 / tile_texels_0(rect_4)), 2.0f, 8.0f);
}

fn tile_tap_0( rect_5 : vec4<f32>,  texel_step_1 : vec2<f32>,  tile_uv_2 : vec2<f32>,  spoke_1 : vec2<f32>,  rotation_1 : vec2<f32>,  reference_1 : f32) -> f32
{
    var tile_min_0 : vec2<f32> = vec2<f32>(0.5f, 0.5f) * texel_step_1;
    var _S108 : f32 = spoke_1.x;
    var _S109 : f32 = rotation_1.x;
    var _S110 : f32 = spoke_1.y;
    var _S111 : f32 = rotation_1.y;
    return (textureSampleCompareLevel((shadow_atlas_0), (shadow_sampler_0), (atlas_uv_0(rect_5, clamp(tile_uv_2 + vec2<f32>(_S108 * _S109 - _S110 * _S111, _S108 * _S111 + _S110 * _S109) * texel_step_1, tile_min_0, vec2<f32>(1.0f) - tile_min_0))), (reference_1)));
}

fn tile_pcf_0( tile_1 : u32,  tile_uv_3 : vec2<f32>,  reference_2 : f32,  pixel_2 : vec2<f32>,  radius_2 : f32) -> f32
{
    var _S112 : vec2<f32> = shadow_rotation_0(pixel_2);
    var rect_6 : vec4<f32> = atlas_rect_0(tile_1);
    if(atlas_rect_is_empty_0(rect_6))
    {
        return 1.0f;
    }
    var _S113 : vec2<f32> = atlas_step_0(rect_6);
    var spot_0 : u32 = u32(0);
    var probe_0 : f32 = 0.0f;
    for(;;)
    {
        if(spot_0 < u32(5))
        {
        }
        else
        {
            break;
        }
        var probe_1 : f32 = probe_0 + tile_tap_0(rect_6, _S113, tile_uv_3, SHADOW_DISC_0[SHADOW_PROBE_INDEX_0[spot_0]] * vec2<f32>(radius_2), _S112, reference_2);
        spot_0 = spot_0 + u32(1);
        probe_0 = probe_1;
    }
    if(probe_0 <= 0.0f)
    {
        return 0.0f;
    }
    if(probe_0 >= 5.0f)
    {
        return 1.0f;
    }
    var index_4 : u32 = u32(0);
    var visibility_0 : f32 = 0.0f;
    for(;;)
    {
        if(index_4 < u32(32))
        {
        }
        else
        {
            break;
        }
        var visibility_1 : f32 = visibility_0 + tile_tap_0(rect_6, _S113, tile_uv_3, SHADOW_DISC_0[index_4] * vec2<f32>(radius_2), _S112, reference_2);
        index_4 = index_4 + u32(1);
        visibility_0 = visibility_1;
    }
    return visibility_0 / 32.0f;
}

fn cascade_visibility_0( cascade_1 : u32,  world_position_4 : vec3<f32>,  to_light_2 : vec3<f32>,  geometric_normal_1 : vec3<f32>,  pixel_3 : vec2<f32>) -> f32
{
    var rect_7 : vec4<f32> = atlas_rect_0(cascade_1);
    if(atlas_rect_is_empty_0(rect_7))
    {
        return 1.0f;
    }
    var texel_world_0 : f32 = 2.0f * frame_0.cascade_far_0[cascade_1] / tile_texels_0(rect_7);
    var clip_0 : vec4<f32> = (((vec4<f32>(world_position_4 + geometric_normal_1 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.w * shadow_normal_offset_0(geometric_normal_1, to_light_2))) + to_light_2 * vec3<f32>((texel_world_0 * frame_0.shadow_params_0.z)), 1.0f)) * (mat4x4<f32>(frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(0)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(1)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(2)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(0)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(1)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(2)][i32(3)], frame_0.shadow_view_proj_0.data_2[cascade_1].data_1[i32(3)][i32(3)]))));
    var ndc_0 : vec3<f32> = clip_0.xyz / vec3<f32>(clip_0.w);
    var _S114 : bool;
    if((any(((abs(ndc_0.xy)) > vec2<f32>(1.0f)))))
    {
        _S114 = true;
    }
    else
    {
        _S114 = (ndc_0.z) <= 0.0f;
    }
    if(_S114)
    {
        return 1.0f;
    }
    var tile_uv_4 : vec2<f32> = vec2<f32>(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f);
    var _S115 : f32 = ndc_0.z;
    return tile_pcf_0(cascade_1, tile_uv_4, _S115, pixel_3, sun_penumbra_texels_0(cascade_1, tile_uv_4, _S115, shadow_rotation_0(pixel_3)));
}

fn sun_visibility_0( world_position_5 : vec3<f32>,  to_light_3 : vec3<f32>,  n_dot_l_1 : f32,  geometric_normal_2 : vec3<f32>,  pixel_4 : vec2<f32>) -> f32
{
    var cascade_2 : u32;
    if(n_dot_l_1 <= 0.0f)
    {
        return 1.0f;
    }
    var eye_distance_0 : f32 = length(world_position_5 - frame_0.camera_position_0.xyz);
    var index_5 : u32 = u32(0);
    for(;;)
    {
        if(index_5 < u32(2))
        {
        }
        else
        {
            cascade_2 = u32(1);
            break;
        }
        if(eye_distance_0 < (frame_0.cascade_far_0[index_5]))
        {
            cascade_2 = index_5;
            break;
        }
        index_5 = index_5 + u32(1);
    }
    var visibility_2 : f32 = cascade_visibility_0(cascade_2, world_position_5, to_light_3, geometric_normal_2, pixel_4);
    var _S116 : u32 = cascade_2 + u32(1);
    if(_S116 >= u32(2))
    {
        return visibility_2;
    }
    var band_0 : f32 = frame_0.cascade_far_0[cascade_2] * 0.10000000149011612f;
    var blend_0 : f32 = saturate((eye_distance_0 - (frame_0.cascade_far_0[cascade_2] - band_0)) / band_0);
    if(blend_0 <= 0.0f)
    {
        return visibility_2;
    }
    return mix(visibility_2, cascade_visibility_0(_S116, world_position_5, to_light_3, geometric_normal_2, pixel_4), blend_0);
}

fn contact_at_0( position_4 : vec2<f32>) -> f32
{
    var width_2 : u32;
    var height_2 : u32;
    {var dim = textureDimensions((contact_shadow_0));((width_2)) = dim.x;((height_2)) = dim.y;};
    var _S117 : vec3<i32> = vec3<i32>(min(vec2<i32>(position_4), vec2<i32>(i32(width_2), i32(height_2)) - vec2<i32>(i32(1))), i32(0));
    return (textureLoad((contact_shadow_0), ((_S117)).xy, ((_S117)).z).x);
}

fn point_face_0( from_light_0 : vec3<f32>) -> u32
{
    var axis_2 : vec3<f32> = abs(from_light_0);
    var _S118 : f32 = axis_2.x;
    var _S119 : f32 = axis_2.y;
    var _S120 : bool;
    if(_S118 >= _S119)
    {
        _S120 = _S118 >= (axis_2.z);
    }
    else
    {
        _S120 = false;
    }
    var _S121 : u32;
    if(_S120)
    {
        if((from_light_0.x) >= 0.0f)
        {
            _S121 = u32(0);
        }
        else
        {
            _S121 = u32(1);
        }
        return _S121;
    }
    if(_S119 >= (axis_2.z))
    {
        if((from_light_0.y) >= 0.0f)
        {
            _S121 = u32(2);
        }
        else
        {
            _S121 = u32(3);
        }
        return _S121;
    }
    if((from_light_0.z) >= 0.0f)
    {
        _S121 = u32(4);
    }
    else
    {
        _S121 = u32(5);
    }
    return _S121;
}

fn light_tile_0( tile_2 : u32) -> u32
{
    return u32(2) + tile_2;
}

fn punctual_visibility_0( tile_3 : u32,  world_position_6 : vec3<f32>,  to_light_4 : vec3<f32>,  n_dot_l_2 : f32,  map_world_0 : f32,  geometric_normal_3 : vec3<f32>,  pixel_5 : vec2<f32>) -> f32
{
    var atlas_0 : u32 = light_tile_0(tile_3);
    var rect_8 : vec4<f32> = atlas_rect_0(atlas_0);
    if(atlas_rect_is_empty_0(rect_8))
    {
        return 1.0f;
    }
    var texel_world_1 : f32 = map_world_0 / tile_texels_0(rect_8);
    var clip_1 : vec4<f32> = (((vec4<f32>(world_position_6 + geometric_normal_3 * vec3<f32>((texel_world_1 * 4.0f * shadow_normal_offset_0(geometric_normal_3, to_light_4))) + to_light_4 * vec3<f32>((texel_world_1 * 2.0f)), 1.0f)) * (mat4x4<f32>(frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(0)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(1)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(2)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(0)][i32(3)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(1)][i32(3)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(2)][i32(3)], frame_0.light_view_proj_0.data_3[tile_3].data_1[i32(3)][i32(3)]))));
    var _S122 : f32 = clip_1.w;
    if(_S122 <= 0.0f)
    {
        return 1.0f;
    }
    var ndc_1 : vec3<f32> = clip_1.xyz / vec3<f32>(_S122);
    var _S123 : bool;
    if((any(((abs(ndc_1.xy)) > vec2<f32>(1.0f)))))
    {
        _S123 = true;
    }
    else
    {
        _S123 = (ndc_1.z) <= 0.0f;
    }
    if(_S123)
    {
        _S123 = true;
    }
    else
    {
        _S123 = (ndc_1.z) > 1.0f;
    }
    if(_S123)
    {
        return 1.0f;
    }
    return tile_pcf_0(atlas_0, vec2<f32>(ndc_1.x * 0.5f + 0.5f, 0.5f - ndc_1.y * 0.5f), ndc_1.z, pixel_5, 2.0f);
}

fn point_visibility_0( light_1 : ptr<function, GpuLight_std430_0>,  base_1 : u32,  world_position_7 : vec3<f32>,  to_light_5 : vec3<f32>,  n_dot_l_3 : f32,  geometric_normal_4 : vec3<f32>,  pixel_6 : vec2<f32>) -> f32
{
    if(n_dot_l_3 <= 0.0f)
    {
        return 1.0f;
    }
    var from_light_1 : vec3<f32> = world_position_7 - (*light_1).position_0.xyz;
    return punctual_visibility_0(base_1 + point_face_0(from_light_1), world_position_7, to_light_5, n_dot_l_3, 2.0f * max(max(abs(from_light_1.x), abs(from_light_1.y)), abs(from_light_1.z)), geometric_normal_4, pixel_6);
}

fn spot_visibility_0( light_2 : ptr<function, GpuLight_std430_0>,  tile_4 : u32,  world_position_8 : vec3<f32>,  to_light_6 : vec3<f32>,  n_dot_l_4 : f32,  geometric_normal_5 : vec3<f32>,  pixel_7 : vec2<f32>) -> f32
{
    if(n_dot_l_4 <= 0.0f)
    {
        return 1.0f;
    }
    var cos_outer_1 : f32 = (*light_2).direction_0.w;
    return punctual_visibility_0(tile_4, world_position_8, to_light_6, n_dot_l_4, 2.0f * (sqrt(saturate(1.0f - cos_outer_1 * cos_outer_1)) / max(cos_outer_1, 0.00009999999747379f)) * max(dot(world_position_8 - (*light_2).position_0.xyz, normalize((*light_2).direction_0.xyz)), 0.0f), geometric_normal_5, pixel_7);
}

fn specular_compensation_0( f0_1 : vec3<f32>,  directional_albedo_0 : f32) -> vec3<f32>
{
    return vec3<f32>(1.0f, 1.0f, 1.0f) + f0_1 * vec3<f32>((1.0f / clamp(directional_albedo_0, 0.00009999999747379f, 1.0f) - 1.0f));
}

fn bent_normal_at_0( occlusion_0 : vec4<f32>,  shading_normal_1 : vec3<f32>) -> vec3<f32>
{
    var decoded_0 : vec3<f32> = occlusion_0.yzw * vec3<f32>(2.0f) - vec3<f32>(1.0f);
    var _S124 : vec3<f32>;
    if((length(decoded_0)) < 0.5f)
    {
        _S124 = shading_normal_1;
    }
    else
    {
        _S124 = normalize(decoded_0);
    }
    return _S124;
}

fn sky_irradiance_0( normal_6 : vec3<f32>) -> vec3<f32>
{
    var basis_6 : vec4<f32> = vec4<f32>(normal_6, 1.0f);
    return max(vec3<f32>(dot(frame_0.sky_sh_r_0, basis_6), dot(frame_0.sky_sh_g_0, basis_6), dot(frame_0.sky_sh_b_0, basis_6)), vec3<f32>(0.0f, 0.0f, 0.0f));
}

fn probe_row_0( cell_1 : vec3<u32>) -> u32
{
    return min((cell_1.z * frame_0.probe_counts_0.y + cell_1.y) * frame_0.probe_counts_0.x + cell_1.x, max(frame_0.probe_counts_0.w, u32(1)) - u32(1));
}

fn sign_not_zero_0( value_0 : f32) -> f32
{
    var _S125 : f32;
    if(value_0 >= 0.0f)
    {
        _S125 = 1.0f;
    }
    else
    {
        _S125 = -1.0f;
    }
    return _S125;
}

fn oct_encode_0( direction_1 : vec3<f32>) -> vec2<f32>
{
    var _S126 : f32 = direction_1.y;
    var p_0 : vec2<f32> = direction_1.xz / vec2<f32>(max(abs(direction_1.x) + abs(_S126) + abs(direction_1.z), 9.99999968265522539e-21f));
    var p_1 : vec2<f32>;
    if(_S126 < 0.0f)
    {
        var _S127 : f32 = p_0.y;
        var _S128 : f32 = p_0.x;
        p_1 = vec2<f32>((1.0f - abs(_S127)) * sign_not_zero_0(_S128), (1.0f - abs(_S128)) * sign_not_zero_0(_S127));
    }
    else
    {
        p_1 = p_0;
    }
    return p_1;
}

fn probe_moments_0( index_6 : u32,  direction_2 : vec3<f32>) -> vec2<f32>
{
    var width_3 : u32;
    var height_3 : u32;
    var layers_0 : u32;
    {var dim = textureDimensions((probe_visibility_0));((width_3)) = dim.x;((height_3)) = dim.y;((layers_0)) = textureNumLayers((probe_visibility_0));};
    var _S129 : vec2<f32> = vec2<f32>(0.5f);
    var _S130 : vec2<f32> = vec2<f32>(1.0f);
    var scaled_1 : vec2<f32> = (oct_encode_0(direction_2) * _S129 + _S129) * vec2<f32>(16.0f) + _S130 - _S129;
    var _S131 : vec2<f32> = vec2<f32>(f32(width_3), f32(height_3)) - _S130;
    var low_2 : vec2<f32> = clamp(floor(scaled_1), vec2<f32>(0.0f, 0.0f), _S131);
    var high_2 : vec2<f32> = min(low_2 + _S130, _S131);
    var weight_2 : vec2<f32> = clamp(scaled_1 - low_2, vec2<f32>(0.0f), vec2<f32>(1.0f));
    var layer_1 : i32 = i32(min(index_6, max(layers_0, u32(1)) - u32(1)));
    var _S132 : i32 = i32(low_2.x);
    var _S133 : i32 = i32(low_2.y);
    var _S134 : vec4<i32> = vec4<i32>(_S132, _S133, layer_1, i32(0));
    var _S135 : i32 = i32(high_2.x);
    var _S136 : vec4<i32> = vec4<i32>(_S135, _S133, layer_1, i32(0));
    var _S137 : i32 = i32(high_2.y);
    var _S138 : vec4<i32> = vec4<i32>(_S132, _S137, layer_1, i32(0));
    var _S139 : vec4<i32> = vec4<i32>(_S135, _S137, layer_1, i32(0));
    var _S140 : vec2<f32> = vec2<f32>(weight_2.x);
    return mix(mix((textureLoad((probe_visibility_0), ((_S134)).xy, i32(((_S134)).z), ((_S134)).w)).xy, (textureLoad((probe_visibility_0), ((_S136)).xy, i32(((_S136)).z), ((_S136)).w)).xy, _S140), mix((textureLoad((probe_visibility_0), ((_S138)).xy, i32(((_S138)).z), ((_S138)).w)).xy, (textureLoad((probe_visibility_0), ((_S139)).xy, i32(((_S139)).z), ((_S139)).w)).xy, _S140), vec2<f32>(weight_2.y));
}

fn probe_weight_0( index_7 : u32,  probe_position_0 : vec3<f32>,  world_position_9 : vec3<f32>,  normal_7 : vec3<f32>) -> f32
{
    var to_probe_0 : vec3<f32> = probe_position_0 - (world_position_9 + normal_7 * vec3<f32>(0.05000000074505806f));
    var to_surface_0 : f32 = length(to_probe_0);
    var moments_0 : vec2<f32> = probe_moments_0(index_7, (vec3<f32>(0) - to_probe_0));
    var _S141 : f32 = moments_0.x;
    var _S142 : f32 = max(moments_0.y - _S141 * _S141, 0.0f);
    var behind_0 : f32 = to_surface_0 - _S141;
    var bound_0 : f32 = _S142 / (_S142 + behind_0 * behind_0);
    var visible_0 : f32;
    if(to_surface_0 <= _S141)
    {
        visible_0 = 1.0f;
    }
    else
    {
        visible_0 = bound_0 * bound_0 * bound_0;
    }
    return max(visible_0, 0.00009999999747379f);
}

struct GpuProbe_0
{
     sh_r_0 : vec4<f32>,
     sh_g_0 : vec4<f32>,
     sh_b_0 : vec4<f32>,
};

struct WeightedProbe_0
{
     sh_0 : GpuProbe_0,
     weight_3 : f32,
};

fn probe_corner_0( cell_2 : vec3<u32>,  spacing_0 : vec3<f32>,  world_position_10 : vec3<f32>,  normal_8 : vec3<f32>) -> WeightedProbe_0
{
    var row_0 : u32 = probe_row_0(cell_2);
    var stored_0 : GpuProbe_std430_0 = probes_0[row_0];
    var weight_4 : f32 = probe_weight_0(row_0, frame_0.probe_origin_0.xyz + vec3<f32>(cell_2) * spacing_0, world_position_10, normal_8);
    var corner_2 : WeightedProbe_0;
    var _S143 : vec4<f32> = vec4<f32>(weight_4);
    corner_2.sh_0.sh_r_0 = stored_0.sh_r_0 * _S143;
    corner_2.sh_0.sh_g_0 = stored_0.sh_g_0 * _S143;
    corner_2.sh_0.sh_b_0 = stored_0.sh_b_0 * _S143;
    corner_2.weight_3 = weight_4;
    return corner_2;
}

fn lerp_probe_0( a_1 : WeightedProbe_0,  b_0 : WeightedProbe_0,  t_1 : f32) -> WeightedProbe_0
{
    var blended_0 : WeightedProbe_0;
    var _S144 : vec4<f32> = vec4<f32>(t_1);
    blended_0.sh_0.sh_r_0 = mix(a_1.sh_0.sh_r_0, b_0.sh_0.sh_r_0, _S144);
    blended_0.sh_0.sh_g_0 = mix(a_1.sh_0.sh_g_0, b_0.sh_0.sh_g_0, _S144);
    blended_0.sh_0.sh_b_0 = mix(a_1.sh_0.sh_b_0, b_0.sh_0.sh_b_0, _S144);
    blended_0.weight_3 = mix(a_1.weight_3, b_0.weight_3, t_1);
    return blended_0;
}

fn probe_irradiance_0( world_position_11 : vec3<f32>,  normal_9 : vec3<f32>) -> vec3<f32>
{
    var _S145 : vec3<f32> = vec3<f32>(1.0f);
    const _S146 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var last_0 : vec3<f32> = max(vec3<f32>(frame_0.probe_counts_0.xyz) - _S145, _S146);
    var grid_0 : vec3<f32> = clamp((world_position_11 - frame_0.probe_origin_0.xyz) * frame_0.probe_inv_spacing_0.xyz, _S146, last_0);
    var base_2 : vec3<f32> = floor(grid_0);
    var f_0 : vec3<f32> = grid_0 - base_2;
    var _S147 : vec3<u32> = vec3<u32>(base_2);
    var _S148 : vec3<u32> = vec3<u32>(min(base_2 + _S145, last_0));
    var inv_0 : vec3<f32> = frame_0.probe_inv_spacing_0.xyz;
    var _S149 : f32 = inv_0.x;
    var _S150 : f32;
    if(_S149 != 0.0f)
    {
        _S150 = 1.0f / _S149;
    }
    else
    {
        _S150 = 0.0f;
    }
    var _S151 : f32 = inv_0.y;
    var _S152 : f32;
    if(_S151 != 0.0f)
    {
        _S152 = 1.0f / _S151;
    }
    else
    {
        _S152 = 0.0f;
    }
    var _S153 : f32 = inv_0.z;
    var _S154 : f32;
    if(_S153 != 0.0f)
    {
        _S154 = 1.0f / _S153;
    }
    else
    {
        _S154 = 0.0f;
    }
    var spacing_1 : vec3<f32> = vec3<f32>(_S150, _S152, _S154);
    var _S155 : u32 = _S147.x;
    var _S156 : u32 = _S147.y;
    var _S157 : u32 = _S147.z;
    var _S158 : u32 = _S148.x;
    var _S159 : f32 = f_0.x;
    var _S160 : u32 = _S148.y;
    var _S161 : u32 = _S148.z;
    var _S162 : f32 = f_0.y;
    var cell_3 : WeightedProbe_0 = lerp_probe_0(lerp_probe_0(lerp_probe_0(probe_corner_0(vec3<u32>(_S155, _S156, _S157), spacing_1, world_position_11, normal_9), probe_corner_0(vec3<u32>(_S158, _S156, _S157), spacing_1, world_position_11, normal_9), _S159), lerp_probe_0(probe_corner_0(vec3<u32>(_S155, _S160, _S157), spacing_1, world_position_11, normal_9), probe_corner_0(vec3<u32>(_S158, _S160, _S157), spacing_1, world_position_11, normal_9), _S159), _S162), lerp_probe_0(lerp_probe_0(probe_corner_0(vec3<u32>(_S155, _S156, _S161), spacing_1, world_position_11, normal_9), probe_corner_0(vec3<u32>(_S158, _S156, _S161), spacing_1, world_position_11, normal_9), _S159), lerp_probe_0(probe_corner_0(vec3<u32>(_S155, _S160, _S161), spacing_1, world_position_11, normal_9), probe_corner_0(vec3<u32>(_S158, _S160, _S161), spacing_1, world_position_11, normal_9), _S159), _S162), f_0.z);
    var basis_7 : vec4<f32> = vec4<f32>(normal_9, 1.0f);
    return max(vec3<f32>(dot(cell_3.sh_0.sh_r_0, basis_7), dot(cell_3.sh_0.sh_g_0, basis_7), dot(cell_3.sh_0.sh_b_0, basis_7)) / vec3<f32>(cell_3.weight_3), _S146);
}

fn multi_bounce_occlusion_0( visibility_3 : f32,  albedo_0 : vec3<f32>) -> vec3<f32>
{
    var _S163 : vec3<f32> = vec3<f32>(visibility_3);
    return min(vec3<f32>(1.0f), max(_S163, ((_S163 * (vec3<f32>(2.04040002822875977f) * albedo_0 - vec3<f32>(0.33239999413490295f)) + (vec3<f32>(-4.79510021209716797f) * albedo_0 + vec3<f32>(0.64170002937316895f))) * _S163 + (vec3<f32>(2.75519990921020508f) * albedo_0 + vec3<f32>(0.69029998779296875f))) * _S163));
}

fn emissive_of_0( material_4 : ptr<function, GpuMaterial_std430_0>) -> vec3<f32>
{
    return vec3<f32>((*material_4).emissive_r_0, (*material_4).emissive_g_0, (*material_4).emissive_b_0);
}

fn fog_exp_neg_0( x_1 : f32) -> f32
{
    var clamped_0 : f32 = clamp(x_1, -87.0f, 87.0f);
    var n_0 : f32 = floor(clamped_0 * 1.4426950216293335f + 0.5f);
    var _S164 : f32 = - (clamped_0 - n_0 * 0.693115234375f - n_0 * 0.00003194618329871f);
    var kernel_0 : f32 = 0.0001984127011383f;
    var term_0 : i32 = i32(6);
    for(;;)
    {
        if(term_0 >= i32(0))
        {
        }
        else
        {
            break;
        }
        var _S165 : f32 = kernel_0 * _S164 + FOG_KERNEL_0[term_0];
        var term_1 : i32 = term_0 - i32(1);
        kernel_0 = _S165;
        term_0 = term_1;
    }
    return kernel_0 * (bitcast<f32>(((u32(i32(127) - i32(n_0)) << (u32(23))))));
}

fn fog_one_minus_exp_over_0( d_0 : f32) -> f32
{
    if((abs(d_0)) < 0.125f)
    {
        var _S166 : f32 = - d_0;
        var series_0 : f32 = 0.00833333376795053f;
        var term_2 : i32 = i32(3);
        for(;;)
        {
            if(term_2 >= i32(0))
            {
            }
            else
            {
                break;
            }
            var _S167 : f32 = series_0 * _S166 + FOG_RATIO_KERNEL_0[term_2];
            var term_3 : i32 = term_2 - i32(1);
            series_0 = _S167;
            term_2 = term_3;
        }
        return series_0;
    }
    return (1.0f - fog_exp_neg_0(d_0)) / d_0;
}

fn fog_optical_depth_0( density_0 : f32,  falloff_0 : f32,  height_a_0 : f32,  height_b_0 : f32,  distance_2 : f32) -> f32
{
    if(falloff_0 <= 0.0f)
    {
        return clamp(density_0 * distance_2, 0.0f, 32.0f);
    }
    return clamp(density_0 * distance_2 * fog_exp_neg_0(height_a_0 / falloff_0) * fog_one_minus_exp_over_0((height_b_0 - height_a_0) / falloff_0), 0.0f, 32.0f);
}

fn fog_transmittance_0( optical_depth_0 : f32) -> f32
{
    return fog_exp_neg_0(max(optical_depth_0, 0.0f));
}

struct FragmentOutput_0
{
    @location(0) lit_0 : vec4<f32>,
    @location(1) reflectivity_0 : vec4<f32>,
    @location(2) motion_0 : vec2<f32>,
};

struct pixelInput_0
{
    @location(0) world_position_12 : vec3<f32>,
    @location(6) world_normal_1 : vec3<f32>,
    @location(7) color_3 : vec4<f32>,
    @interpolate(flat) @location(8) material_5 : u32,
    @location(1) uv_2 : vec2<f32>,
    @location(2) clip_position_1 : vec4<f32>,
    @location(3) previous_clip_position_1 : vec4<f32>,
    @location(4) world_tangent_1 : vec3<f32>,
    @interpolate(flat) @location(5) frame_2 : u32,
};

@fragment
fn fragmentMain( _S168 : pixelInput_0, @builtin(position) position_5 : vec4<f32>) -> FragmentOutput_0
{
    var _S169 : VertexOutput_0 = VertexOutput_0( position_5, _S168.world_position_12, _S168.world_normal_1, _S168.color_3, _S168.material_5, _S168.uv_2, _S168.clip_position_1, _S168.previous_clip_position_1, _S168.world_tangent_1, _S168.frame_2 );
    var vertex_normal_0 : vec3<f32> = normalize(_S168.world_normal_1);
    var motion_1 : vec2<f32> = motion_vector_0(_S168.clip_position_1, _S168.previous_clip_position_1);
    if((frame_0.ambient_0.w) >= 5.5f)
    {
        var bent_0 : FragmentOutput_0;
        bent_0.lit_0 = vec4<f32>(occlusion_at_0(position_5.xy).yzw, 1.0f);
        bent_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        bent_0.motion_0 = motion_1;
        return bent_0;
    }
    if((frame_0.ambient_0.w) >= 4.5f)
    {
        var moved_0 : FragmentOutput_0;
        moved_0.lit_0 = vec4<f32>(motion_1 * vec2<f32>(8.0f) + vec2<f32>(0.5f), 0.0f, 1.0f);
        moved_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        moved_0.motion_0 = motion_1;
        return moved_0;
    }
    if((frame_0.ambient_0.w) >= 3.5f)
    {
        var value_1 : f32 = occlusion_at_0(position_5.xy).x;
        var occlusion_1 : FragmentOutput_0;
        occlusion_1.lit_0 = vec4<f32>(value_1, value_1, value_1, 1.0f);
        occlusion_1.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        occlusion_1.motion_0 = motion_1;
        return occlusion_1;
    }
    if((frame_0.ambient_0.w) >= 1.5f)
    {
        var tint_0 : FragmentOutput_0;
        tint_0.lit_0 = vec4<f32>(_S168.color_3.xyz, 1.0f);
        tint_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        tint_0.motion_0 = motion_1;
        return tint_0;
    }
    var _S170 : GpuMaterial_std430_0 = materials_0[_S168.material_5];
    var uv_3 : vec2<f32>;
    if((_S170.tiling_0) == u32(1))
    {
        uv_3 = physical_tile_uv_0(_S168.world_position_12, vertex_normal_0, _S170.tile_metres_0);
    }
    else
    {
        uv_3 = _S168.uv_2;
    }
    var _S171 : u32 = normal_layer_0(&(_S170));
    var normal_10 : vec3<f32> = shading_normal_of_0(_S171, _S170.normal_scale_0, _S169, vertex_normal_0, uv_3);
    if((frame_0.ambient_0.w) >= 0.5f)
    {
        var normals_0 : FragmentOutput_0;
        var _S172 : vec3<f32> = vec3<f32>(0.5f);
        normals_0.lit_0 = vec4<f32>(normal_10 * _S172 + _S172, 1.0f);
        normals_0.reflectivity_0 = vec4<f32>(0.0f, 0.0f, 0.0f, 1.0f);
        normals_0.motion_0 = motion_1;
        return normals_0;
    }
    var to_eye_1 : vec3<f32> = normalize(frame_0.camera_position_0.xyz - _S168.world_position_12);
    var _S173 : vec3<f32> = geometric_normal_of_0(_S168.world_position_12, vertex_normal_0);
    var _S174 : u32 = base_color_layer_0(&(_S170));
    var _S175 : vec3<f32> = vec3<f32>(uv_3, f32(_S174));
    var albedo_1 : vec4<f32> = _S168.color_3 * _S170.base_color_0 * (textureSample((base_color_textures_0), (base_color_sampler_0), ((_S175)).xy, i32(((_S175)).z)));
    var metallic_1 : f32 = saturate(_S170.metallic_0);
    var roughness_2 : f32 = clamp(_S170.roughness_0, 0.04500000178813934f, 1.0f);
    var alpha_0 : f32 = roughness_2 * roughness_2;
    var _S176 : f32 = alpha_0 * alpha_0;
    var _S177 : vec3<f32> = albedo_1.xyz;
    var f0_2 : vec3<f32> = mix(vec3<f32>(0.03999999910593033f, 0.03999999910593033f, 0.03999999910593033f), _S177, vec3<f32>(metallic_1));
    var diffuse_albedo_0 : vec3<f32> = _S177 * vec3<f32>((1.0f - metallic_1));
    var _S178 : f32 = max(dot(normal_10, to_eye_1), 0.00009999999747379f);
    var _S179 : vec2<f32> = position_5.xy;
    var _S180 : u32 = froxel_of_0(_S179, (((vec4<f32>(_S168.world_position_12, 1.0f)) * (mat4x4<f32>(frame_0.view_proj_0.data_1[i32(0)][i32(0)], frame_0.view_proj_0.data_1[i32(1)][i32(0)], frame_0.view_proj_0.data_1[i32(2)][i32(0)], frame_0.view_proj_0.data_1[i32(3)][i32(0)], frame_0.view_proj_0.data_1[i32(0)][i32(1)], frame_0.view_proj_0.data_1[i32(1)][i32(1)], frame_0.view_proj_0.data_1[i32(2)][i32(1)], frame_0.view_proj_0.data_1[i32(3)][i32(1)], frame_0.view_proj_0.data_1[i32(0)][i32(2)], frame_0.view_proj_0.data_1[i32(1)][i32(2)], frame_0.view_proj_0.data_1[i32(2)][i32(2)], frame_0.view_proj_0.data_1[i32(3)][i32(2)], frame_0.view_proj_0.data_1[i32(0)][i32(3)], frame_0.view_proj_0.data_1[i32(1)][i32(3)], frame_0.view_proj_0.data_1[i32(2)][i32(3)], frame_0.view_proj_0.data_1[i32(3)][i32(3)])))).w);
    var base_3 : u32 = _S180 * u32(17);
    var _S181 : u32 = min(cluster_lights_0[base_3], u32(16));
    var table_0 : TableTap_0 = table_tap_0(_S178, roughness_2);
    var dfg_0 : vec2<f32> = dfg_at_0(table_0);
    var _S182 : f32 = dfg_0.x;
    var _S183 : f32 = dfg_0.y;
    var _S184 : vec3<f32> = f0_2 * vec3<f32>(_S182) + vec3<f32>(_S183);
    const _S185 : vec3<f32> = vec3<f32>(0.0f, 0.0f, 0.0f);
    var slot_0 : u32 = u32(0);
    var direct_0 : vec3<f32> = _S185;
    var gloss_0 : vec3<f32> = _S185;
    for(;;)
    {
        if(slot_0 < _S181)
        {
        }
        else
        {
            break;
        }
        var _S186 : GpuLight_std430_0 = lights_0[cluster_lights_0[base_3 + u32(1) + slot_0]];
        var _S187 : u32 = _S186.kind_0;
        var _S188 : bool = (_S186.kind_0) == u32(0);
        var to_light_7 : vec3<f32>;
        var reach_0 : f32;
        if(_S188)
        {
            to_light_7 = normalize(_S186.direction_0.xyz);
            reach_0 = 1.0f;
        }
        else
        {
            if(_S187 == u32(3))
            {
                var offset_0 : vec3<f32> = _S186.position_0.xyz - _S168.world_position_12;
                var distance_3 : f32 = length(offset_0);
                var _S189 : f32 = range_window_0(distance_3, _S186.position_0.w);
                to_light_7 = offset_0 / vec3<f32>(max(distance_3, 9.99999997475242708e-07f));
                reach_0 = _S189;
            }
            else
            {
                var offset_1 : vec3<f32> = _S186.position_0.xyz - _S168.world_position_12;
                var distance_4 : f32 = length(offset_1);
                var to_light_8 : vec3<f32> = offset_1 / vec3<f32>(max(distance_4, 9.99999997475242708e-07f));
                var reach_1 : f32 = punctual_falloff_0(distance_4, _S186.position_0.w);
                if(_S187 == u32(2))
                {
                    reach_0 = reach_1 * spot_cone_0(to_light_8, _S186.direction_0.xyz, _S186.direction_0.w, _S186.cos_inner_0);
                }
                else
                {
                    reach_0 = reach_1;
                }
                to_light_7 = to_light_8;
            }
        }
        var n_dot_l_5 : f32 = dot(normal_10, to_light_7);
        var specular_0 : vec3<f32>;
        var diffuse_0 : f32;
        if(_S187 == u32(3))
        {
            var corners_2 : array<vec3<f32>, i32(4)>;
            rect_corners_0(&(_S186), _S168.world_position_12, &(corners_2));
            var to_local_0 : mat3x3<f32> = ltc_shading_frame_0(normal_10, to_eye_1, _S178);
            var _S190 : vec3<f32> = vec3<f32>(ltc_irradiance_0((((to_local_0) * (ltc_transform_0(ltc_at_0(table_0))))), corners_2)) * _S184;
            diffuse_0 = ltc_irradiance_0(to_local_0, corners_2);
            specular_0 = _S190;
        }
        else
        {
            var _S191 : f32 = max(n_dot_l_5, 0.0f);
            var half_vector_0 : vec3<f32> = normalize(to_light_7 + to_eye_1);
            var specular_1 : vec3<f32> = ggx_lobe_0(_S176, f0_2, _S191, _S178, max(dot(normal_10, half_vector_0), 0.0f), max(dot(to_eye_1, half_vector_0), 0.0f)) * vec3<f32>(_S191);
            diffuse_0 = _S191;
            specular_0 = specular_1;
        }
        var specular_2 : vec3<f32>;
        if((((_S186.flags_3) & (u32(1)))) != u32(0))
        {
            specular_2 = _S185;
        }
        else
        {
            specular_2 = specular_0;
        }
        var reach_2 : f32;
        if(_S188)
        {
            reach_2 = sun_visibility_0(_S168.world_position_12, to_light_7, n_dot_l_5, _S173, _S179) * contact_at_0(_S179);
        }
        else
        {
            if(_S187 == u32(1))
            {
                var _S192 : u32 = _S186.shadow_tile_0;
                if((_S186.shadow_tile_0) <= u32(8))
                {
                    var _S193 : f32 = point_visibility_0(&(_S186), _S192, _S168.world_position_12, to_light_7, n_dot_l_5, _S173, _S179);
                    reach_2 = reach_0 * _S193;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
            else
            {
                var _S194 : u32 = _S186.shadow_tile_0;
                if((_S186.shadow_tile_0) < u32(14))
                {
                    var _S195 : f32 = spot_visibility_0(&(_S186), _S194, _S168.world_position_12, to_light_7, n_dot_l_5, _S173, _S179);
                    reach_2 = reach_0 * _S195;
                }
                else
                {
                    reach_2 = reach_0;
                }
            }
        }
        var _S196 : vec3<f32> = _S186.color_0.xyz;
        var direct_1 : vec3<f32> = direct_0 + _S196 * vec3<f32>((diffuse_0 * reach_2));
        var gloss_1 : vec3<f32> = gloss_0 + _S196 * (specular_2 * vec3<f32>(reach_2));
        slot_0 = slot_0 + u32(1);
        direct_0 = direct_1;
        gloss_0 = gloss_1;
    }
    var occlusion_texel_0 : vec4<f32> = occlusion_at_0(_S179);
    var bent_normal_0 : vec3<f32> = bent_normal_at_0(occlusion_texel_0, normal_10);
    var lit_1 : vec3<f32> = diffuse_albedo_0 * ((frame_0.ambient_0.xyz + sky_irradiance_0(bent_normal_0) + probe_irradiance_0(_S168.world_position_12, bent_normal_0)) * multi_bounce_occlusion_0(occlusion_texel_0.x, diffuse_albedo_0) + direct_0) + gloss_0 * specular_compensation_0(f0_2, _S182 + _S183);
    var _S197 : vec3<f32> = emissive_of_0(&(_S170));
    var fog_survives_0 : f32 = fog_transmittance_0(fog_optical_depth_0(frame_0.fog_params_0.x, frame_0.fog_params_0.y, frame_0.camera_position_0.y - frame_0.fog_params_0.z, _S168.world_position_12.y - frame_0.fog_params_0.z, length(frame_0.camera_position_0.xyz - _S168.world_position_12)));
    var output_3 : FragmentOutput_0;
    output_3.lit_0 = vec4<f32>((lit_1 + _S197) * vec3<f32>(fog_survives_0) + frame_0.fog_color_0.xyz * vec3<f32>((1.0f - fog_survives_0)), albedo_1.w);
    output_3.reflectivity_0 = vec4<f32>(f0_2, floor(roughness_2 * 255.0f + 0.5f) / 255.0f);
    output_3.motion_0 = motion_1;
    return output_3;
}

