#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 48 "shaders/probe_octahedral.slang"
struct OctahedralParams_0
{
    uint probes_0;
    uint probe_base_0;
    uint extent_0;
    uint face_texels_0;
    uint atlas_columns_0;
    uint row_floats_0;
    uint layer_floats_0;
    uint reserved_0;
};


#line 48
struct _MatrixStorage_float4x4_ColMajornatural_0
{
    array<packed_float4, int(4)> data_0;
};


#line 146
struct KernelContext_0
{
    OctahedralParams_0 constant* params_0;
    packed_float4 device* directions_0;
    _MatrixStorage_float4x4_ColMajornatural_0 device* faces_0;
    texture2d<float, access::sample> distances_0;
    float device* moments_0;
};


#line 104
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], OctahedralParams_0 constant* params_1 [[buffer(0)]], packed_float4 device* directions_1 [[buffer(1)]], _MatrixStorage_float4x4_ColMajornatural_0 device* faces_1 [[buffer(2)]], texture2d<float, access::sample> distances_1 [[texture(0)]], float device* moments_1 [[buffer(3)]])
{

#line 104
    thread KernelContext_0 kernelContext_0;

#line 104
    (&kernelContext_0)->params_0 = params_1;

#line 104
    (&kernelContext_0)->directions_0 = directions_1;

#line 104
    (&kernelContext_0)->faces_0 = faces_1;

#line 104
    (&kernelContext_0)->distances_0 = distances_1;

#line 104
    (&kernelContext_0)->moments_0 = moments_1;

    uint texels_0 = params_1->extent_0 * params_1->extent_0;
    uint index_0 = thread_0.x;
    if(index_0 >= (params_1->probes_0 * texels_0))
    {
        return;
    }

    uint probe_0 = index_0 / texels_0;
    uint texel_0 = index_0 - probe_0 * texels_0;
    uint row_0 = texel_0 / params_1->extent_0;
    uint column_0 = texel_0 - row_0 * params_1->extent_0;

#line 116
    float4 _S1 = float4(*((&kernelContext_0)->directions_0+texel_0)) ;



    uint face_0 = uint(_S1.w);


    uint tile_0 = probe_0 * 6U + face_0;

#line 123
    _MatrixStorage_float4x4_ColMajornatural_0 device* _S2 = (&kernelContext_0)->faces_0+(((&kernelContext_0)->params_0->probe_base_0 + probe_0) * 6U + face_0);



    float4 clip_0 = (((float4(_S1.xyz, 0.0f)) * (matrix<float,int(4),int(4)> ((*_S2).data_0[int(0)][int(0)], (*_S2).data_0[int(1)][int(0)], (*_S2).data_0[int(2)][int(0)], (*_S2).data_0[int(3)][int(0)], (*_S2).data_0[int(0)][int(1)], (*_S2).data_0[int(1)][int(1)], (*_S2).data_0[int(2)][int(1)], (*_S2).data_0[int(3)][int(1)], (*_S2).data_0[int(0)][int(2)], (*_S2).data_0[int(1)][int(2)], (*_S2).data_0[int(2)][int(2)], (*_S2).data_0[int(3)][int(2)], (*_S2).data_0[int(0)][int(3)], (*_S2).data_0[int(1)][int(3)], (*_S2).data_0[int(2)][int(3)], (*_S2).data_0[int(3)][int(3)]))));
    float3 ndc_0 = clip_0.xyz / float3(clip_0.w) ;

#line 133
    float side_0 = float((&kernelContext_0)->params_0->face_texels_0);



    float _S3 = side_0 - 1.0f;

#line 137
    float2 inside_0 = clamp(float2(ndc_0.x * 0.5f + 0.5f, 0.5f - ndc_0.y * 0.5f) * float2(side_0) , float2(0.0f, 0.0f), float2(_S3, _S3));
    uint _S4 = tile_0 % (&kernelContext_0)->params_0->atlas_columns_0;

#line 138
    uint _S5 = _S4 * (&kernelContext_0)->params_0->face_texels_0;
    uint _S6 = tile_0 / (&kernelContext_0)->params_0->atlas_columns_0;

    int3 _S7 = int3(int2(uint2(_S5, _S6 * (&kernelContext_0)->params_0->face_texels_0) + uint2(inside_0)), int(0));

#line 141
    float reach_0 = (((&kernelContext_0)->distances_0).read(vec<uint,2>(((_S7)).xy), uint(((_S7)).z)).x);



    uint at_0 = ((&kernelContext_0)->params_0->probe_base_0 + probe_0) * (&kernelContext_0)->params_0->layer_floats_0 + row_0 * (&kernelContext_0)->params_0->row_floats_0 + column_0 * 2U;
    *((&kernelContext_0)->moments_0+at_0) = reach_0;
    *((&kernelContext_0)->moments_0+(at_0 + 1U)) = reach_0 * reach_0;
    return;
}

