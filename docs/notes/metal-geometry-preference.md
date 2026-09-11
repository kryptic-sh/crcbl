# Metal geometry preference — 2026-09-11

The Metal backend now reports `Features::MESH_SHADER | Features::TASK_SHADER` on
devices that satisfy its Metal 3 + macOS 13 gate, and the two `Unrun` parity
rows for Metal are retired. Reporting the flag moves
`GeometryPath::from_features` — and therefore
`Device::preferred_geometry_path`'s default — onto `MeshShader`, so the sample
set's unforced tail became a decision rather than an assumption. It measured no
faster, so `crcbl_mtl::device` keeps `IndirectPerBatch` as its preference while
the capability stays truthful.

## Measurement

Headless, uncapped (`--fps 0`), 2400 frames, deterministic camera and scene.
Round 0 is a warm-up and is excluded; six measured rounds alternate the order of
the two tails so a slow drift cannot favour either. The same binary draws both
tails per app, selected exactly with `--force-geometry`; quarry runs at
`--lod-budget 0.001` so both tails draw the finest comparable geometry.

```sh
CRCBL_TRACE=1 MTL_DEBUG_LAYER=0 MTL_SHADER_VALIDATION=0 \
  target/release/quarry --headless --backend mtl --frames 2400 --size 1280x720 \
  --fps 0 --no-debug-overlay --camera fixed --lod-budget 0.001 \
  --force-geometry mesh-shader     # and: indirect-per-batch
```

Environment: Apple M3 Pro (18 GPU cores), macOS 26.5.2, rustc 1.97.0. No
compilation or other GPU work ran during the pairs.

| case              | mesh mean (s) | per-batch mean (s) | median mesh−pb | min    | max    |
| ----------------- | ------------- | ------------------ | -------------- | ------ | ------ |
| quarry 1280x720   | 5.412         | 5.404              | +0.16%         | −0.18% | +0.38% |
| quarry 1920x1080  | 10.492        | 10.448             | +0.10%         | −0.10% | +2.09% |
| sundial 1280x720  | 6.370         | 6.329              | +0.62%         | +0.47% | +0.83% |
| sundial 1920x1080 | 13.250        | 13.220             | +0.27%         | −0.04% | +0.42% |

`wall_seconds` is process wall time, so it includes startup; the deltas are
small and one round (quarry 1080p, +2.09%) is an outlier rather than a trend.
The direction is consistent and not in mesh's favour: the mesh tail is at best
level and on Sundial at 720p a reliable half a percent behind.

## Decision

`MetalDevice::preferred_geometry_path` answers `IndirectPerBatch` for both the
count ceiling and the mesh one. The capability is not withheld: the flags are
reported, the parity rows are retired, and the mesh tail stays selectable
exactly through `ForwardRenderer::with_scene_on_path` and the samples'
`--force-geometry mesh-shader`. The golden and device suites exercise it that
way; only the unforced default stays on the cheaper tail.

This is one machine and two mesh-heavy samples. It decides the default; it is
not a claim that no scene ever favours the mesh tail, and the preference is a
single match arm to revisit if one does.
