# Material sampling on Metal — 2026-09-11

Returning early for the packed and emissive pages an untextured material does
not have reduces measured offscreen elapsed time by 0.7–1.8% on the tested M3
Pro workloads at 720p and 1080p. This is the half of `3ecad8c4` that the tiling
gate does not forbid; that commit's base-colour half stays reverted. Shared
goldens are unchanged, and this is a small measured improvement rather than a
claim about every scene, GPU or backend.

## Change

The forward shader sampled base-colour, metallic/roughness/occlusion and
emissive textures even when a material named `NO_PAGE`, then discarded the
sample, which kept implicit-LOD texture operations in uniform control flow for
WGSL validation.

`mro_texel` and `emissive_texel` now compute UV derivatives before their
material-dependent return and sample through `SampleGrad` only when a page
exists; a row naming no page returns the identity without a fetch. Both call
sites — `fragmentMain` and the reflective shadow map's `rsmFragmentMain` — sit
in uniform control flow, which is what the derivatives need.

**`base_color_texel` deliberately keeps the unconditional `Sample`.** Explicit
gradients lose anisotropic filtering on lavapipe: `tiling_e2e`'s grazing floor
draws 8.0 of far-floor contrast at 8× against 8.0 at 1× with `SampleGrad`, where
the gate wants at least 30 and twice the control, while Metal is unaffected
(70.0 against 7.0). The base-colour page is the one that gate measures, so its
fetch stays implicit and a row naming no page pays for it. `3ecad8c4` changed
that helper too and was reverted in `16f48813` for exactly this.

## Method and results

- Apple M3 Pro, macOS 26.5.2; Rust 1.97.0 release builds.
- Offscreen Metal, renderer geometry `IndirectPerBatch`, validation disabled,
  CPU tracing enabled. No concurrent local GPU work or compilation.
- Sandbox draws its deterministic rotating cube, sundial a plaza under a
  scripted moving sun with PCSS shadows, lantern textured surfaces, a live
  second-view monitor and volumetric fog.
- One warmup run per workload preceded the measured rounds. Four measured pairs
  followed, alternating which binary ran first; round 1 still shows a first-run
  effect on the before binary for three workloads, so the medians below are
  given both with it and without it.
- `/usr/bin/time -p` `real` — whole-process elapsed from a monotonic host clock.
  The headless simulation's fixed 60 Hz clock is not a performance measurement.

| Workload | Viewport  | Frames | Before median | After median | Less elapsed time |
| -------- | --------- | ------ | ------------- | ------------ | ----------------- |
| sandbox  | 1280x720  | 2400   | 4.800 s       | 4.760 s      | 0.83%             |
| sandbox  | 1920x1080 | 2400   | 9.420 s       | 9.350 s      | 0.74%             |
| sundial  | 1280x720  | 2400   | 6.420 s       | 6.340 s      | 1.25%             |
| sundial  | 1920x1080 | 2400   | 13.530 s      | 13.370 s     | 1.18%             |
| lantern  | 1280x720  | 1200   | 4.760 s       | 4.720 s      | 0.84%             |
| lantern  | 1920x1080 | 1200   | 9.110 s       | 8.950 s      | 1.76%             |

Medians of rounds 2–4. Counting round 1 as well, the same medians give 0.73%,
0.80%, 2.69%, 1.33%, 1.05% and 1.59%. **All 24 measured pairs were
non-regressing**, and the after build was faster in every one. These are
end-to-end timings, not isolated fragment-kernel measurements.

[Raw paired samples and binary hashes](metal-material-sampling-results.json)
record every warmup and measured sample. The before tree is `d2ac240d` and the
after tree is the commit that added this note; the after build differs from the
before only in `mesh.slang` and its regenerated artifacts.

```sh
cargo build --locked --release -p sandbox -p sundial -p lantern
CRCBL_TRACE=1 MTL_DEBUG_LAYER=0 MTL_SHADER_VALIDATION=0 \
  <preserved-binary> --headless --backend mtl --frames 2400 \
  --size 1280x720 --fps 0 --no-debug-overlay
```

Use 1200 frames for Lantern and repeat at 1920x1080, preserving separate before
and after binaries and alternating their order.

## Correctness and portability

- The fork's diagnostic workflow regenerated all four target sets with the
  pinned Slang 2026.14, DXC 1.9 `releasev1.9.2607` and SPIRV-Tools 2026.1
  ([run 34580744700](https://github.com/digitaloten/crcbl/actions/runs/34580744700)).
  Exactly six committed files differ — `msl/mesh.metal`, `spirv/mesh.spv`,
  `spirv/manifest.txt`, `wgsl/mesh.wgsl`, `dxil/mesh.fragment.dxil` and
  `dxil/mesh.rsm_fragment.dxil` — and every other artifact came back
  byte-identical. `dxil/mesh.depth_masked_fragment.dxil` is unchanged because
  `depthMaskedFragmentMain` reads only the base-colour helper this slice left
  alone, which is the check that the split is what it claims.
- Strict Metal validation on the M3 Pro: renderer goldens 59, mesh 94, forward
  37, tiling 2, lantern 8, quarry 25, `crcbl-mtl` mesh probes 6, `crcbl-mtl`
  hardware 74 — API errors and warnings asserted, shader reporting, stderr
  reporting and abort-on-fault enabled for the renderer, mesh, forward and
  tiling suites.
- The host gate passed with `appkit_session` excluded: formatting, all-target
  all-feature clippy with warnings denied, the no-default-features build, and
  6418 tests. `crcbl-shaders`' `every_committed_wgsl_artifact_validates` is naga
  over every committed `wgsl/*.wgsl`, so it re-parsed the changed module.
- **Not run here:** the lavapipe tiling gate (CI's `vk e2e` job; the page it
  measures is untouched), the browser tier and Dawn's positive and negative
  uniformity controls (the module changed, so this is the one leg with no local
  verdict), and the browser pixel goldens.
