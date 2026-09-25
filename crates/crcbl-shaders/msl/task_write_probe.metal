#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 98 "shaders/task_write_probe.slang"
struct Amplification_0
{
    float4 tint_0;
};


#line 129
struct KernelContext_0
{
    atomic<uint> device* count_0;
    uint device* slots_0;
};


#line 144
[[object]] void writingTaskMain(uint3 group_0 [[threadgroup_position_in_grid]], Amplification_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp, atomic<uint> device* count_1 [[buffer(0)]], uint device* slots_1 [[buffer(1)]])
{

#line 144
    thread KernelContext_0 kernelContext_0;

#line 144
    (&kernelContext_0)->count_0 = count_1;

#line 144
    (&kernelContext_0)->slots_0 = slots_1;

    uint _S1 = group_0.x;

#line 146
    uint odd_0 = _S1 & 1U;

    if(odd_0 == 1U)
    {
        uint _S2 = atomic_fetch_add_explicit((&kernelContext_0)->count_0+int(0), 1U, memory_order_relaxed);

#line 148
    }

#line 153
    if(odd_0 == 0U)
    {
        *((&kernelContext_0)->slots_0+_S1) = _S1 + 1U;

#line 153
    }

#line 158
    thread Amplification_0 amplification_0;
    (&amplification_0)->tint_0 = float4(0.0f, 1.0f, 1.0f, 1.0f);
    *_slang_mesh_payload = *(&amplification_0); _slang_mgp.set_threadgroups_per_grid(uint3((1U), (1U), (1U))); return;;
    return;
}

