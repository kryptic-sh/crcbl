// crcbl-targets: spirv, msl, dxil

// An amplification stage that **writes to storage buffers** before it
// dispatches, feeding `mesh_shader.slang`'s mesh stage.
//
// `docs/backlog.md`'s D3D12 mesh-shading entry is being bisected: WARP reports
// `MeshShaderTier = TIER_1`, survives every combination of {direct
// `DispatchMesh`, indirect `ExecuteIndirect`} x {mesh only, amplified} that
// `crcbl-dx12`'s probes drive — including 1024 amplification groups through one
// `ExecuteIndirect`, and including a zero-group `DispatchMesh` — and then loses
// the device inside `crcbl-render`'s frame. Pipeline shape, dispatch scale and
// a zero dispatch count are all eliminated, so what is left is what the
// amplification stage *does*.
//
// This is the next feature off that list. `mesh_cluster.slang`'s `taskMain`
// performs **two writes to storage buffers** before its `DispatchMesh`, and
// every probe that has passed on WARP writes nothing at all from that stage:
//
// * an **atomic** — `cull_stats[word].add(1u)` on an
//   `RWStructuredBuffer<Atomic<uint> >`, under a branch, so only the groups
//   that actually tested a cluster count;
// * a **plain indexed store** — `cluster_selection[index] = selected` on an
//   ordinary `RWStructuredBuffer<uint>`, under a *different* branch, so one
//   instance slot records the cut and the others leave the entry alone.
//
// Both are here, in that order, on two buffers of their own and each under its
// own branch. A UAV write from an amplification stage — an atomic one above all
// — is a much less travelled path than the dispatch it precedes, and it is the
// next difference in content between the toy shader the passing probes run and
// the real one.
//
// # What the writes say when they are read back
//
// The dispatch is one-dimensional, so `SV_GroupID.x` is the group's index in
// the whole dispatch and the parity of that index decides both branches: an odd
// group adds one to the single counter, an even group stores its own index plus
// one into its own slot. A caller therefore knows the exact contents of both
// buffers from the dispatch extent alone, with nothing bound to configure it
// and nothing read back to interpret — which is what makes a pass mean *the
// writes landed* rather than that the device survived them.
//
// The stored value is the index **plus one** so that no slot's expected value
// is zero: a buffer a driver quietly left alone, or one whose write went to the
// wrong resource, reads as zero or as the caller's own sentinel, and neither
// can be mistaken for a group's own answer. The untouched odd slots keep that
// sentinel, which is what shows the branch really branched instead of every
// group writing.
//
// # `DispatchMesh` is `mesh_shader.slang`'s, unchanged
//
// One mesh group, unbranched, with that file's tint in the payload — so this
// draws the picture
// `many_indirect_amplification_groups_do_not_remove_the_device` already draws
// and the writes are the only thing that differs. Bisecting one feature at a
// time is the point: the zero-group dispatch has its own probe and passed.
//
// # Why a file of its own rather than another entry point in an existing module
//
// **Slang 2026.14's Metal backend miscompiles any module with two
// amplification entry points** — it emits every `[[object]]` function in the
// module, including ones the edit never touched, with its payload and
// `mesh_grid_properties` parameters listed twice, which is a redefined
// parameter and no longer Metal. `zero_dispatch_probe.slang`'s header records
// the shape of that defect and the second one that appears when the two entry
// points are given payload structs of their own. One amplification entry point
// per module is the constraint.
//
// # It carries the amplification stage alone
//
// The mesh and fragment stages come from `mesh_shader.slang` unchanged. A D3D12
// mesh pipeline takes one DXIL container per stage — `crcbl_dx12`'s
// `create_mesh_pipeline` resolves each of `task`, `mesh` and `fragment` from
// whichever module its own `ShaderEntry` names — so the task stage coming from
// a second source changes nothing about what the runtime is handed, and it is
// what keeps the bisect honest: the mesh and fragment containers are
// byte-identical to the ones the passing probes use, and the amplification
// container is the only thing that differs.

/// What this stage hands the mesh stage: the layout `mesh_shader.slang`'s
/// `Amplification` declares, and `amplifiedMeshMain` there is what reads it.
///
/// Declared again rather than shared through a header for the reason that file
/// gives about its own `Vertex` — one source, one layout — and D3D12 requires
/// only that the two agree in size, which one `float4` does by construction.
struct Amplification
{
    float4 tint;
};

/// The counter the atomic write below adds into, and the whole of it: one word,
/// contended by every group whose branch is taken, exactly as
/// `mesh_cluster.slang`'s three words are contended by every task group of a
/// frame.
///
/// `Atomic<uint>` rather than `InterlockedAdd` for `cull.slang`'s reason:
/// spelled as a type it lowers on every target.
///
/// **The binding number is the slot in the probe's set, not this file's own
/// count.** `crcbl-dx12`'s `MeshProbe` binds `mesh_shader.slang`'s geometry at
/// binding 0 and appends these two, and only SPIR-V reads the numbers at all:
/// `dxc` numbers each register class from zero in declaration order, so the
/// container declares this `u0`, and Slang's Metal target gives it the first
/// index in this module's own buffer table. Nothing on Metal loads this probe
/// — `crcbl-mtl` would index the two by ascending `(set, binding)` and reach 1
/// and 2 — so the D3D12 numbering is the one that has to agree, and it does.
[[vk::binding(1, 0)]]
RWStructuredBuffer<Atomic<uint> > count;

/// Where the plain indexed store lands: one word per amplification group, the
/// shape `cluster_selection` has for one word per resident cluster.
///
/// Not `Atomic<uint>`, deliberately — the second write `mesh_cluster.slang`
/// performs is an ordinary store, and an atomic one would be the same
/// instruction as the counter above rather than a second thing to eliminate.
[[vk::binding(2, 0)]]
RWStructuredBuffer<uint> slots;

/// One amplification group: two storage writes, then one mesh group.
///
/// The two branches are the parity of the group's own index, so the arms need
/// nothing bound to configure them, a dispatch extent above one covers both,
/// and the result is deterministic — a caller reads back the same two buffers
/// every run. See the header for what they hold.
///
/// The payload is a local for the reason `mesh_shader.slang` gives at length —
/// a module-scope `groupshared` gives Slang's Metal backend a `KernelContext`
/// that every entry point materialises, and a `threadgroup` variable cannot be
/// declared in a fragment function.
[shader("amplification")]
[numthreads(1, 1, 1)]
void writingTaskMain(uint3 group: SV_GroupID)
{
    uint odd = group.x & 1u;

    if (odd == 1u)
    {
        count[0].add(1u);
    }

    if (odd == 0u)
    {
        slots[group.x] = group.x + 1u;
    }

    Amplification amplification;
    amplification.tint = float4(0.0, 1.0, 1.0, 1.0);
    DispatchMesh(1, 1, 1, amplification);
}
