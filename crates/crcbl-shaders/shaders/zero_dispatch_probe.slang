// crcbl-targets: spirv, msl, dxil

// An amplification stage that dispatches **zero** mesh groups for half of its
// groups, feeding `mesh_shader.slang`'s mesh stage.
//
// `docs/backlog.md`'s D3D12 mesh-shading entry is being bisected: WARP reports
// `MeshShaderTier = TIER_1`, survives every combination of {direct
// `DispatchMesh`, indirect `ExecuteIndirect`} x {mesh only, amplified} that
// `crcbl-dx12`'s probes drive — including 1024 amplification groups through one
// `ExecuteIndirect` — and then loses the device inside `crcbl-render`'s frame.
// Pipeline shape and dispatch scale are both eliminated, so what is left is the
// difference in *content* between the toy shader those probes use and the real
// one, taken one feature at a time.
//
// This is the first of those features. `mesh_cluster.slang`'s `taskMain` ends
// with `DispatchMesh(keep, 1, 1, payload)`, where `keep` is `0` on a culled
// cluster — so the renderer asks D3D12 for a **zero-group** `DispatchMesh` on
// every frame that culls anything, and nothing in this workspace had ever asked
// for one. `mesh_shader.slang`'s `taskMain` always dispatches exactly one.
//
// # Why a file of its own rather than a fifth entry point in mesh_shader.slang
//
// **Slang 2026.14's Metal backend miscompiles any module with two
// amplification entry points.** Adding one to `mesh_shader.slang` and
// recompiling emits *every* `[[object]]` function in the module — the existing
// `taskMain` included — with its payload and `mesh_grid_properties` parameters
// listed twice:
//
//     [[object]] void taskMain(Amplification_0 object_data* _slang_mesh_payload
//       [[payload]], mesh_grid_properties _slang_mgp, Amplification_0
//       object_data* _slang_mesh_payload [[payload]], mesh_grid_properties
//       _slang_mgp, …)
//
// which is a redefined parameter and no longer Metal. Giving the second entry
// point a payload struct of its own drops the duplication and produces a
// different defect instead: both `[[object]]` functions declare the *first*
// payload type and the second one then assigns a value of the other type
// through it. Neither form is something `xcrun metal` will take, and the
// breakage lands on `taskMain`, which is the shader the bisect's four passing
// probes run. One amplification entry point per module is the constraint.
//
// # It carries the amplification stage alone
//
// The mesh and fragment stages come from `mesh_shader.slang` unchanged. A D3D12
// mesh pipeline takes one DXIL container per stage — `crcbl_dx12`'s
// `create_mesh_pipeline` resolves each of `task`, `mesh` and `fragment` from
// whichever module its own `ShaderEntry` names — and this script compiles one
// container per entry point anyway, so the task stage coming from a second
// source changes nothing about what the runtime is handed. It is also what
// keeps the bisect honest: the mesh and fragment containers are byte-identical
// to the ones the passing probes use, and the amplification container is the
// only thing that differs.

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

/// One amplification group, dispatching one mesh group or none by the parity of
/// its own group index.
///
/// The count is decided on the GPU from `SV_GroupID` rather than read from a
/// buffer, so the two arms need nothing bound and a dispatch extent above one
/// covers both; it is deterministic, so a caller reads back the same picture
/// every run.
///
/// The tint is `mesh_shader.slang`'s `taskMain`'s, which
/// `crcbl_shaders::mesh_shader::AMPLIFICATION_TINT` owns: the groups that do
/// dispatch draw the frame that shader's amplified probe already draws, so the
/// dispatched count is the only thing this changes.
///
/// The payload is a local for the reason `mesh_shader.slang` gives at length —
/// a module-scope `groupshared` gives Slang's Metal backend a `KernelContext`
/// that every entry point materialises, and a `threadgroup` variable cannot be
/// declared in a fragment function.
[shader("amplification")]
[numthreads(1, 1, 1)]
void culledTaskMain(uint3 group: SV_GroupID)
{
    Amplification amplification;
    amplification.tint = float4(0.0, 1.0, 1.0, 1.0);
    DispatchMesh(group.x & 1, 1, 1, amplification);
}
