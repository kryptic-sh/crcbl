#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 34 "shaders/compute_probe.slang"
struct Params_0
{
    uint count_0;
};


#line 72
struct KernelContext_0
{
    Params_0 constant* params_0;
    uint device* destination_0;
    uint device* source_0;
};


#line 64
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], Params_0 constant* params_1 [[buffer(0)]], uint device* destination_1 [[buffer(2)]], uint device* source_1 [[buffer(1)]])
{

#line 64
    thread KernelContext_0 kernelContext_0;

#line 64
    (&kernelContext_0)->params_0 = params_1;

#line 64
    (&kernelContext_0)->destination_0 = destination_1;

#line 64
    (&kernelContext_0)->source_0 = source_1;

    uint index_0 = thread_0.x;
    if(index_0 >= (params_1->count_0))
    {
        return;
    }

    *((&kernelContext_0)->destination_0+index_0) = (&kernelContext_0)->source_0[index_0] * (&kernelContext_0)->source_0[index_0];
    return;
}

