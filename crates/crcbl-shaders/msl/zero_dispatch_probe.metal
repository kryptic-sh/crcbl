#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 60 "shaders/zero_dispatch_probe.slang"
struct Amplification_0
{
    float4 tint_0;
};


#line 84
[[object]] void culledTaskMain(uint3 group_0 [[threadgroup_position_in_grid]], Amplification_0 object_data* _slang_mesh_payload [[payload]], mesh_grid_properties  _slang_mgp)
{
    thread Amplification_0 amplification_0;
    (&amplification_0)->tint_0 = float4(0.0f, 1.0f, 1.0f, 1.0f);
    *_slang_mesh_payload = *(&amplification_0); _slang_mgp.set_threadgroups_per_grid(uint3(((group_0.x) & 1U), (1U), (1U))); return;;
    return;
}

