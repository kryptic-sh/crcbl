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
    uint pad0_0;
};


#line 99
struct KernelContext_0
{
    ClearParams_0 constant* clear_0;
    uint device* cull_stats_0;
    uint device* args_0;
    uint device* draw_counts_0;
};


#line 93
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], ClearParams_0 constant* clear_1 [[buffer(0)]], uint device* cull_stats_1 [[buffer(1)]], uint device* args_1 [[buffer(2)]], uint device* draw_counts_1 [[buffer(3)]])
{

#line 93
    thread KernelContext_0 kernelContext_0;

#line 93
    (&kernelContext_0)->clear_0 = clear_1;

#line 93
    (&kernelContext_0)->cull_stats_0 = cull_stats_1;

#line 93
    (&kernelContext_0)->args_0 = args_1;

#line 93
    (&kernelContext_0)->draw_counts_0 = draw_counts_1;

    uint index_0 = thread_0.x;

    if(index_0 < (clear_1->stats_words_0))
    {
        *((&kernelContext_0)->cull_stats_0+index_0) = 0U;

#line 97
    }



    if(index_0 < ((&kernelContext_0)->clear_0->args_words_0))
    {
        *((&kernelContext_0)->args_0+index_0) = 0U;

#line 101
    }



    if(index_0 < ((&kernelContext_0)->clear_0->counts_words_0))
    {
        *((&kernelContext_0)->draw_counts_0+index_0) = 0U;

#line 105
    }



    return;
}

