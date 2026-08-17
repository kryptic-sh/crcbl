#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 42 "shaders/clear_counters.slang"
struct ClearParams_0
{
    uint args_words_0;
    uint counts_words_0;
    uint stats_words_0;
    uint mesh_args_words_0;
};


#line 111
struct KernelContext_0
{
    ClearParams_0 constant* clear_0;
    uint device* cull_stats_0;
    uint device* args_0;
    uint device* counts_and_mesh_args_0;
};


#line 105
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], ClearParams_0 constant* clear_1 [[buffer(0)]], uint device* cull_stats_1 [[buffer(1)]], uint device* args_1 [[buffer(2)]], uint device* counts_and_mesh_args_1 [[buffer(3)]])
{

#line 105
    thread KernelContext_0 kernelContext_0;

#line 105
    (&kernelContext_0)->clear_0 = clear_1;

#line 105
    (&kernelContext_0)->cull_stats_0 = cull_stats_1;

#line 105
    (&kernelContext_0)->args_0 = args_1;

#line 105
    (&kernelContext_0)->counts_and_mesh_args_0 = counts_and_mesh_args_1;

    uint index_0 = thread_0.x;

    if(index_0 < (clear_1->stats_words_0))
    {
        *((&kernelContext_0)->cull_stats_0+index_0) = 0U;

#line 109
    }



    if(index_0 < ((&kernelContext_0)->clear_0->args_words_0))
    {
        *((&kernelContext_0)->args_0+index_0) = 0U;

#line 113
    }



    if(index_0 < ((&kernelContext_0)->clear_0->counts_words_0))
    {
        *((&kernelContext_0)->counts_and_mesh_args_0+index_0) = 0U;

#line 117
    }

#line 123
    if(index_0 < ((&kernelContext_0)->clear_0->mesh_args_words_0))
    {
        *((&kernelContext_0)->counts_and_mesh_args_0+((&kernelContext_0)->clear_0->counts_words_0 + index_0)) = 0U;

#line 123
    }



    return;
}

