#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 43 "shaders/clear_counters.slang"
struct ClearParams_0
{
    uint args_words_0;
    uint counts_words_0;
    uint pad0_0;
    uint pad1_0;
};


#line 43
struct KernelContext_0
{
    uint device* visible_count_0;
    ClearParams_0 constant* clear_0;
    uint device* args_0;
    uint device* draw_counts_0;
};


#line 89
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], uint device* visible_count_1 [[buffer(1)]], ClearParams_0 constant* clear_1 [[buffer(0)]], uint device* args_1 [[buffer(2)]], uint device* draw_counts_1 [[buffer(3)]])
{

#line 89
    thread KernelContext_0 kernelContext_0;

#line 89
    (&kernelContext_0)->visible_count_0 = visible_count_1;

#line 89
    (&kernelContext_0)->clear_0 = clear_1;

#line 89
    (&kernelContext_0)->args_0 = args_1;

#line 89
    (&kernelContext_0)->draw_counts_0 = draw_counts_1;

    uint index_0 = thread_0.x;

    if(index_0 == 0U)
    {
        *((&kernelContext_0)->visible_count_0+int(0)) = 0U;

#line 93
    }



    if(index_0 < ((&kernelContext_0)->clear_0->args_words_0))
    {
        *((&kernelContext_0)->args_0+index_0) = 0U;

#line 97
    }



    if(index_0 < ((&kernelContext_0)->clear_0->counts_words_0))
    {
        *((&kernelContext_0)->draw_counts_0+index_0) = 0U;

#line 101
    }



    return;
}

