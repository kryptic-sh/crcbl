#include <metal_stdlib>
#include <metal_math>
#include <metal_texture>
using namespace metal;

#line 55 "shaders/indirect_count_args.slang"
struct PackParams_0
{
    uint count_word_0;
    uint first_word_0;
    uint source_stride_words_0;
    uint structure_words_0;
    uint instance_word_0;
    uint max_draw_count_0;
};


#line 126
struct KernelContext_0
{
    PackParams_0 constant* pack_0;
    uint device* count_0;
    uint device* packed_0;
    uint device* source_0;
};


#line 111
[[kernel]] void computeMain(uint3 thread_0 [[thread_position_in_grid]], PackParams_0 constant* pack_1 [[buffer(0)]], uint device* count_1 [[buffer(1)]], uint device* packed_1 [[buffer(3)]], uint device* source_1 [[buffer(2)]])
{

#line 111
    thread KernelContext_0 kernelContext_0;

#line 111
    (&kernelContext_0)->pack_0 = pack_1;

#line 111
    (&kernelContext_0)->count_0 = count_1;

#line 111
    (&kernelContext_0)->packed_0 = packed_1;

#line 111
    (&kernelContext_0)->source_0 = source_1;

    uint structure_0 = thread_0.x;
    if(structure_0 >= (pack_1->max_draw_count_0))
    {
        return;
    }



    uint _S1 = min((&kernelContext_0)->count_0[(&kernelContext_0)->pack_0->count_word_0], pack_1->max_draw_count_0);
    uint _S2 = (&kernelContext_0)->pack_0->first_word_0 + structure_0 * (&kernelContext_0)->pack_0->source_stride_words_0;
    uint to_0 = structure_0 * (&kernelContext_0)->pack_0->structure_words_0;

#line 123
    uint word_0 = 0U;
    for(;;)
    {

#line 124
        if(word_0 < ((&kernelContext_0)->pack_0->structure_words_0))
        {
        }
        else
        {

#line 124
            break;
        }
        *((&kernelContext_0)->packed_0+(to_0 + word_0)) = (&kernelContext_0)->source_0[_S2 + word_0];

#line 124
        word_0 = word_0 + 1U;

#line 124
    }

#line 130
    if(structure_0 >= _S1)
    {
        *((&kernelContext_0)->packed_0+(to_0 + (&kernelContext_0)->pack_0->instance_word_0)) = 0U;

#line 130
    }



    return;
}

