struct ClearParams_std140_0
{
    @align(16) args_words_0 : u32,
    @align(4) counts_words_0 : u32,
    @align(8) stats_words_0 : u32,
    @align(4) pad0_0 : u32,
};

@binding(0) @group(0) var<uniform> clear_0 : ClearParams_std140_0;
@binding(1) @group(0) var<storage, read_write> cull_stats_0 : array<u32>;

@binding(2) @group(0) var<storage, read_write> args_0 : array<u32>;

@binding(3) @group(0) var<storage, read_write> draw_counts_0 : array<u32>;

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 < (clear_0.stats_words_0))
    {
        cull_stats_0[index_0] = u32(0);
    }
    if(index_0 < (clear_0.args_words_0))
    {
        args_0[index_0] = u32(0);
    }
    if(index_0 < (clear_0.counts_words_0))
    {
        draw_counts_0[index_0] = u32(0);
    }
    return;
}

