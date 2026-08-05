struct Params_std140_0
{
    @align(16) count_0 : u32,
};

@binding(0) @group(0) var<uniform> params_0 : Params_std140_0;
@binding(2) @group(0) var<storage, read_write> destination_0 : array<u32>;

@binding(1) @group(0) var<storage, read> source_0 : array<u32>;

@compute
@workgroup_size(64, 1, 1)
fn computeMain(@builtin(global_invocation_id) thread_0 : vec3<u32>)
{
    var index_0 : u32 = thread_0.x;
    if(index_0 >= (params_0.count_0))
    {
        return;
    }
    destination_0[index_0] = source_0[index_0] * source_0[index_0];
    return;
}

