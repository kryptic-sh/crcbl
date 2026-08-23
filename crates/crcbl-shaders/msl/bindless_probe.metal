#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 210 "shaders/bindless_probe.slang"
struct _Array_default_StructuredBufferx3Cuintx3E7_0
{
    array<uint device*, int(7)> data_0;
};


#line 210
struct Sources_default_0
{
    _Array_default_StructuredBufferx3Cuintx3E7_0 buffers_0;
};


#line 210
struct KernelContext_0
{
    uint device* destination_0;
    Sources_default_0 constant* sources_0;
};


#line 189
[[kernel]] void computeMain(uint3 sv_groupthreadid_0 [[thread_position_in_threadgroup]], uint3 group_0 [[threadgroup_position_in_grid]], uint device* destination_1 [[buffer(0)]], Sources_default_0 constant* sources_1 [[buffer(1)]])
{

#line 189
    thread KernelContext_0 kernelContext_0;

#line 189
    (&kernelContext_0)->destination_0 = destination_1;

#line 189
    (&kernelContext_0)->sources_0 = sources_1;

#line 189
    uint sv_groupindex_0 = (sv_groupthreadid_0[int(2)] + sv_groupthreadid_0[int(1)]) * 64U + sv_groupthreadid_0[int(0)];

    if(sv_groupindex_0 >= 4U)
    {
        return;
    }

#line 199
    uint source_0 = group_0.x;

#line 205
    if(source_0 >= 4U)
    {
        return;
    }

    *((&kernelContext_0)->destination_0+(source_0 * 4U + sv_groupindex_0)) = (&(&kernelContext_0)->sources_0->buffers_0)->data_0[source_0][sv_groupindex_0];
    return;
}

