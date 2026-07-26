# Stage 6 — Assets + Scenes

Stop hardcoding cubes. Grounded open formats in, engine-native scene format,
stable asset ids, hot reload for the dev loop.

**Format decisions are LOCKED** (settled 2026-07-27, supersedes the earlier
"revisitable" flag):

## Format matrix

Pattern: **source formats = grounded open standards; cooked formats = ours**
(produced by `crcbl import` / `crcbl bake`, topic 11). We never invent a source
format where an open standard suffices — own formats only where we own the
semantics (scenes) or the runtime layout (cooked output).

| Asset     | Source (tracked in git)                   | Cooked (shipped/web)                       | Notes                                |
| --------- | ----------------------------------------- | ------------------------------------------ | ------------------------------------ |
| Mesh      | glTF 2.0 (.gltf/.glb)                     | packed binary matching GPU pool layout     | git LFS — mesh diffs are meaningless |
| Skeleton  | glTF skins                                | cooked joint tables                        | same file/pipeline as meshes         |
| Animation | glTF animation channels                   | cooked sampled/compressed curves           | curve compression = cook step        |
| Texture   | PNG                                       | KTX2/BasisU post-MVP (PNG passthrough MVP) | LFS; sRGB/linear correct from start  |
| Audio SFX | WAV (PCM)                                 | QOA (open one-page spec, own decoder)      | LFS for WAV; see topic 13            |
| Music     | WAV/FLAC                                  | Vorbis or Opus (post-MVP, decoder seam)    | streaming                            |
| Scene     | own: `.scn/` directory of RON chunk files | packed scene blob (`crcbl bake`)           | design below                         |
| Config    | TOML (`crcbl.toml`, tuning tables)        | as-is                                      | flat data, familiarity wins          |

RON vs TOML split: **RON for scene chunks + entity data** (nested, enum-heavy —
collider shapes, component variants map natively to Rust enums; TOML forces
stringly `type = "..."` conventions and verbose nesting). **TOML for flat
config** where ubiquity and editor support win. Both get comments.

`.gitattributes` in the workspace root (P0): LFS for
`*.glb *.gltf *.png *.wav *.flac`; cooked outputs are build artifacts, never
tracked.

## Scene format: directory of chunk files

A scene is a **directory with an extension**, chunks are files:

```
scenes/level1.scn/
  scene.ron          # header: format version, metadata, system manifest
  env.ron            # camera defaults, lighting, ambience
  sys/
    creeps.ron       # one file per system: that system's entity array
    towers.ron
    props.ron
```

This resolves the two constraints that fight inside any single file:

- **Chunk persistence**: the editor tracks dirty systems (same dirty-set
  machinery replication already needs). Save = rewrite only dirty chunk files,
  each small. A full-scene rewrite never happens.
- **Git-friendliness**: small text files, per-system diffs ("moved 3 towers" =
  3-line diff in `sys/towers.ron`), and two people editing different systems
  merge with zero conflicts.

Load semantics unchanged: entities as `(system → data)` arrays mirroring ECS
registration — scene ↔ ECS ↔ replication stay the same shape. Assets referenced
by canonical path (hashed to `AssetId` on load). Versioned header + serde
defaults for forward-compat; no migration machinery in MVP.

### Deterministic writer (90% of git-friendliness)

- Canonical field order; entities sorted by stable ID.
- **Stable entity IDs persisted in the file, never regenerated on save** (the
  classic Unity/Godot diff-noise bug — banned by test).
- Shortest-roundtrip float formatting (Rust f64 `Display` semantics):
  deterministic and exact.
- No timestamps, no editor-session state in scene files.
- Invariant, enforced as a topic-12 property test: load → save → byte-identical
  file; edit one entity → diff touches only its lines.

### Command journal (complement, not format)

Editor autosave = append-only log of edit commands (`.scn.autosave.log`,
gitignored — the stage 8 command stream serialized). Crash recovery replays the
journal; explicit save = flush dirty chunk files canonically + truncate journal.
Git only ever sees canonical form.

### Scaling

If a system's array grows huge, shard its chunk by sector:
`sys/props/{sector}.ron` — same mechanism, finer chunks, and it aligns with
physics sector streaming (stage 5) so a sector's scene data and its physics load
unit coincide.

### Bake

`crcbl bake` packs a `.scn/` directory (+ referenced cooked assets) into one
binary blob for shipping and wasm (solves the many-small-fetches problem;
`FetchSource` serves the blob). Cooked output = build artifact.

## Asset model

- `AssetId` = 64-bit hash of canonical path (stable across machines);
  `AssetHandle<T>` = runtime generational handle (from `crcbl-core`).
- Load states: `Unloaded → Loading → Ready | Failed` — **async from day one**,
  because wasm has no blocking filesystem: all IO goes through an `AssetSource`
  trait (`DirSource` native; `FetchSource` wasm in stage 10; `PackSource` for
  baked blobs). Physics sector streaming (stage 5) rides the same trait.
- Dependency tracking: scene → meshes → materials → textures. Refcounted
  release; the GPU pools get retire calls through the stage 2 deletion queue.

## Import pipeline

- **glTF 2.0** (`gltf` crate): meshes (positions/normals/tangents/UVs),
  PBR-metallic-roughness materials, textures, node hierarchy flattened into
  instances. Skins/animations parsed but unused until the animation feature
  lands (post-MVP) — the format choice already covers them.
- Import happens at load time in MVP (no offline bake step required); the
  `AssetSource` seam is where `crcbl bake` output slots in without touching
  consumers.
- Textures: sRGB/linear handling correct from the start; mip generation on
  upload (compute pass — GPU-side, per the round-trip principle).

## Hot reload (dev builds)

- Native: notify-based file watcher → asset reimport → in-place GPU pool update
  (mesh range swap; stale ranges retire through deletion queue).
- Shaders: Slang recompile → pipeline rebuild keyed by shader hash (stage 2 left
  the runtime-recompile path open).
- Scene reload: chunk-file granularity — a changed `sys/towers.ron` tears down
  and re-instantiates only that system's scene entities (server-side;
  replication propagates). Editor "revert" reuses the same path.

## Tasks

1. `.gitattributes` + LFS setup (P0, foundations).
2. `AssetId`/handle/state machine + `AssetSource` trait + `DirSource`.
3. glTF import → GPU pools (meshes, materials, textures, mips-on-GPU).
4. Scene-dir format: deterministic RON writer, chunk load/save, dirty-chunk
   tracking, roundtrip property tests.
5. Watcher + reload paths (assets, shaders, per-chunk scene reload).
6. `crcbl import` CLI wiring; `crcbl bake` (may land later, pre-Pages-demo of a
   scene-heavy sample).
7. Sandbox: load a real glTF scene (Sponza or similar) via a `.scn/` dir, fly
   through it at stage 3 performance targets.

## Exit criteria

- Sponza-class scene loads through the full path (scene dir → server →
  replication → client → GPU pools) and renders at target perf.
- Deterministic-writer property tests green: save is byte-stable, single- entity
  edit produces minimal diff, editing one chunk file hot-reloads only that
  system.
- Editing a texture/shader/scene chunk on disk reflects in the running sandbox
  without restart.
- No synchronous IO anywhere in engine crates (CI: deny `std::fs` outside
  `DirSource` + tooling).

## Risks

- **Import scope explosion (full glTF spec).** MVP importer supports exactly
  what the sandbox scene needs; unsupported features log-and-skip loudly.
- **Hot reload edge cases eat time.** Reload is dev-only: correctness bar is
  "doesn't crash, usually works", not production-grade.
- **Writer determinism erosion.** Any new serialized type must keep the
  byte-stable-save property; the property test is the gate, not review
  vigilance.

## Corrections (design review, 2026-07-27)

- **`AssetId` = hash-of-path breaks on rename/move.** Renaming `props/crate.glb`
  silently orphans every scene chunk, material instance, and effect referencing
  it — the exact problem Unity's `.meta` GUIDs and Godot's `.import` UIDs exist
  to solve. **Corrected model**: every asset gets a **sidecar meta file**
  (`crate.glb.meta.ron`) carrying a stable random 128-bit GUID, created on first
  import and committed to git; references use the GUID. Path hashing survives
  only as a CLI/debug convenience lookup. Cheap now, painful after P9 content
  exists.
- **The sidecar also carries import settings** that a bare file can't express:
  texture `color_space` (sRGB vs linear — a standalone normal-map PNG has no way
  to declare itself), `usage`, compression target, LOD overrides (25), ragdoll
  asset link (35).
- **sRGB mip generation**: Vulkan has **no sRGB storage-image format**, so
  "mipgen in a compute pass" cannot write sRGB directly. Corrected: create a
  `UNORM` **image view alias** over the sRGB image and do the encode/decode
  manually in the compute shader (the standard approach), or fall back to
  render-pass downsampling. Stated so it isn't discovered at P9.
