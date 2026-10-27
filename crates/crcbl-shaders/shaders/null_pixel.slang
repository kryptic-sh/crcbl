// crcbl-targets: spirv, dxil

// A pixel stage that reads nothing, writes nothing and does nothing.
//
// `crcbl-dx12` puts it in place of a mesh pipeline's absent pixel shader on
// WARP, because WARP removes the device when a mesh pipeline draws with a null
// one (`docs/backlog.md`, the D3D12 mesh-shading entry: the same pipeline with
// any real pixel shader draws, and a vertex pipeline with a null one draws
// too). Hardware keeps the null shader and its depth-only fast path.
//
// No inputs, so it links behind any mesh stage's outputs; no outputs, so it
// changes nothing a depth-only pass writes. D3D12 alone uses it; the SPIR-V
// is compiled because the manifest reads every shader's entry points from it.

[shader("fragment")]
void fragmentMain()
{
}
