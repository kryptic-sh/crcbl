#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 83 "shaders/push_constant_probe.slang"
struct ProbeConstants_0
{
    uint4 values_0;
};


#line 5522 "core.meta.slang"
struct KernelContext_0
{
    uint device* destination_0;
    ProbeConstants_0 constant* constants_0;
};


#line 104 "shaders/push_constant_probe.slang"
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], uint device* destination_1 [[buffer(0)]], ProbeConstants_0 constant* constants_1 [[buffer(1)]])
{

#line 104
    thread KernelContext_0 kernelContext_0;

#line 104
    (&kernelContext_0)->destination_0 = destination_1;

#line 104
    (&kernelContext_0)->constants_0 = constants_1;

    uint index_0 = thread_0.x;
    if(index_0 >= 4U)
    {
        return;
    }

#line 129
    switch(index_0)
    {
    case 0U:
        {

#line 132
            *((&kernelContext_0)->destination_0+index_0) = (&kernelContext_0)->constants_0->values_0.x;
            break;
        }
    case 1U:
        {

#line 135
            *((&kernelContext_0)->destination_0+index_0) = (&kernelContext_0)->constants_0->values_0.y;
            break;
        }
    case 2U:
        {

#line 138
            *((&kernelContext_0)->destination_0+index_0) = (&kernelContext_0)->constants_0->values_0.z;
            break;
        }
    default:
        {

#line 141
            *((&kernelContext_0)->destination_0+index_0) = (&kernelContext_0)->constants_0->values_0.w;
            break;
        }
    }

#line 144
    return;
}

